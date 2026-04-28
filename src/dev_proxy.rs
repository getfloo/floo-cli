//! Local fixture-user proxy for `floo dev` against accounts-mode apps.
//!
//! In production, the floo gateway authenticates each request and injects
//! `X-Floo-User-Email`, `X-Floo-User-Id`, `X-Floo-User-Name`, and
//! `X-Floo-User-Role` headers before forwarding to the app. In local dev the
//! gateway is not in the request path, so apps that read those headers see
//! nothing.
//!
//! When the user passes `--fixture-user EMAIL` to `floo dev`, this module
//! starts an HTTP/1.1 reverse proxy in front of each accounts-mode service.
//! The proxy injects the four identity headers and forwards the rest of the
//! request to the real dev_command.
//!
//! Pure stdlib — no async runtime, no new dependencies. One thread per accepted
//! connection. Suitable for dev traffic; not a hardened production proxy.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct FixtureUser {
    pub email: String,
    pub id: String,
    pub name: String,
    pub role: String,
}

impl FixtureUser {
    /// Headers that the floo gateway would inject in production.
    pub fn header_block(&self) -> String {
        format!(
            "X-Floo-User-Email: {}\r\nX-Floo-User-Id: {}\r\nX-Floo-User-Name: {}\r\nX-Floo-User-Role: {}\r\n",
            sanitize(&self.email),
            sanitize(&self.id),
            sanitize(&self.name),
            sanitize(&self.role),
        )
    }
}

/// Strip any character that would break the HTTP header line (CR, LF, NUL).
/// Header injection prevention: a fixture user value with `\r\n` in it must
/// not be able to forge additional headers.
fn sanitize(value: &str) -> String {
    value
        .chars()
        .filter(|c| *c != '\r' && *c != '\n' && *c != '\0')
        .collect()
}

/// Bind a proxy on `listen_port` (loopback) forwarding to `127.0.0.1:backend_port`,
/// injecting the fixture user's identity headers into every request.
///
/// Pass `listen_port = 0` to let the OS choose. Returns the actually-bound port
/// alongside the listener thread handle. The thread runs until the listener is
/// dropped or the process exits — there is no graceful shutdown signal because
/// `floo dev` always exits the whole process at the end of the session.
pub fn start_proxy(
    listen_port: u16,
    backend_port: u16,
    user: FixtureUser,
) -> std::io::Result<(JoinHandle<()>, u16)> {
    let listener = TcpListener::bind(("127.0.0.1", listen_port))?;
    let bound_port = listener.local_addr()?.port();
    let user = Arc::new(user);
    let handle = thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(client) => {
                    let user = Arc::clone(&user);
                    thread::spawn(move || {
                        let _ = handle_connection(client, backend_port, &user);
                    });
                }
                Err(_) => {
                    // Accept errors are usually transient; sleep briefly and keep going.
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
    });
    Ok((handle, bound_port))
}

fn handle_connection(
    mut client: TcpStream,
    backend_port: u16,
    user: &FixtureUser,
) -> std::io::Result<()> {
    client.set_read_timeout(Some(Duration::from_secs(60)))?;

    // Read until end-of-headers (\r\n\r\n) or buffer cap.
    let mut headers = Vec::with_capacity(4096);
    let mut chunk = [0u8; 1024];
    let header_end = loop {
        let n = client.read(&mut chunk)?;
        if n == 0 {
            return Ok(());
        }
        headers.extend_from_slice(&chunk[..n]);
        if let Some(idx) = find_subsequence(&headers, b"\r\n\r\n") {
            break idx;
        }
        if headers.len() > 64 * 1024 {
            // Headers too large; drop the connection.
            return Ok(());
        }
    };

    let body_prefix = headers.split_off(header_end + 4);
    let modified_headers = inject_identity_headers(&headers, user);

    let mut backend = TcpStream::connect(("127.0.0.1", backend_port))?;
    backend.set_read_timeout(Some(Duration::from_secs(60)))?;
    backend.write_all(&modified_headers)?;
    if !body_prefix.is_empty() {
        backend.write_all(&body_prefix)?;
    }
    backend.flush()?;

    pipe_bidirectional(client, backend);
    Ok(())
}

/// Insert identity headers immediately before the final `\r\n\r\n`.
///
/// Input is the request bytes up to and INCLUDING the terminating `\r\n\r\n`.
fn inject_identity_headers(request_with_terminator: &[u8], user: &FixtureUser) -> Vec<u8> {
    let block = user.header_block();
    // The buffer ends with `\r\n\r\n`. Splice the new headers between the last
    // existing header line's `\r\n` and the empty line.
    let split_at = request_with_terminator.len() - 2; // before the empty-line CRLF
    let mut out = Vec::with_capacity(request_with_terminator.len() + block.len());
    out.extend_from_slice(&request_with_terminator[..split_at]);
    out.extend_from_slice(block.as_bytes());
    out.extend_from_slice(&request_with_terminator[split_at..]);
    out
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn pipe_bidirectional(client: TcpStream, backend: TcpStream) {
    let client_to_backend = client.try_clone();
    let backend_to_client = backend.try_clone();
    if let (Ok(mut c1), Ok(mut b1), mut c2, mut b2) =
        (client_to_backend, backend_to_client, client, backend)
    {
        let t1 = thread::spawn(move || {
            let _ = std::io::copy(&mut c1, &mut b2);
            let _ = b2.shutdown(Shutdown::Write);
        });
        let _ = std::io::copy(&mut b1, &mut c2);
        let _ = c2.shutdown(Shutdown::Write);
        let _ = t1.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> FixtureUser {
        FixtureUser {
            email: "jane@acme.com".into(),
            id: "01HQK4XYZ".into(),
            name: "Jane Doe".into(),
            role: "member".into(),
        }
    }

    #[test]
    fn header_block_emits_all_four_identity_headers() {
        let block = fixture().header_block();
        assert!(block.contains("X-Floo-User-Email: jane@acme.com\r\n"));
        assert!(block.contains("X-Floo-User-Id: 01HQK4XYZ\r\n"));
        assert!(block.contains("X-Floo-User-Name: Jane Doe\r\n"));
        assert!(block.contains("X-Floo-User-Role: member\r\n"));
    }

    #[test]
    fn sanitize_strips_crlf_so_user_input_cannot_forge_headers() {
        // Header-injection guard: a fixture value with embedded \r\n must not
        // be able to insert another header line. The evil substring stays as
        // part of the value (a single line), and no second header is created.
        let user = FixtureUser {
            email: "evil\r\nX-Admin: yes".into(),
            id: "id".into(),
            name: "Eve".into(),
            role: "member".into(),
        };
        let block = user.header_block();
        // The value is flattened onto one line — no extra CRLF inserted.
        assert!(block.contains("X-Floo-User-Email: evilX-Admin: yes\r\n"));
        // Each header appears exactly once, on its own line.
        let line_count = block
            .split("\r\n")
            .filter(|l| l.starts_with("X-Floo-User-"))
            .count();
        assert_eq!(line_count, 4, "should produce exactly 4 identity headers");
    }

    #[test]
    fn inject_headers_splices_before_empty_line() {
        let req = b"GET /dashboard HTTP/1.1\r\nHost: localhost:13000\r\nUser-Agent: curl\r\n\r\n";
        let modified = inject_identity_headers(req, &fixture());
        let s = std::str::from_utf8(&modified).unwrap();

        // Original headers preserved
        assert!(s.starts_with("GET /dashboard HTTP/1.1\r\n"));
        assert!(s.contains("Host: localhost:13000\r\n"));
        assert!(s.contains("User-Agent: curl\r\n"));

        // Identity headers injected
        assert!(s.contains("X-Floo-User-Email: jane@acme.com\r\n"));
        assert!(s.contains("X-Floo-User-Id: 01HQK4XYZ\r\n"));
        assert!(s.contains("X-Floo-User-Name: Jane Doe\r\n"));
        assert!(s.contains("X-Floo-User-Role: member\r\n"));

        // Still terminated by the empty-line CRLF
        assert!(s.ends_with("\r\n\r\n"));
    }

    #[test]
    fn inject_headers_preserves_original_header_order() {
        // The request line must remain first; identity headers go AFTER existing
        // headers (just before the empty terminator), not between them.
        let req = b"POST /api HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n";
        let modified = inject_identity_headers(req, &fixture());
        let s = std::str::from_utf8(&modified).unwrap();
        let host_idx = s.find("Host: x").unwrap();
        let cl_idx = s.find("Content-Length: 0").unwrap();
        let email_idx = s.find("X-Floo-User-Email").unwrap();
        assert!(host_idx < cl_idx);
        assert!(cl_idx < email_idx);
    }

    #[test]
    fn find_subsequence_returns_first_match() {
        assert_eq!(find_subsequence(b"abc\r\n\r\nrest", b"\r\n\r\n"), Some(3));
        assert_eq!(find_subsequence(b"abc", b"xyz"), None);
        assert_eq!(find_subsequence(b"", b"\r\n\r\n"), None);
    }

    #[test]
    fn end_to_end_proxy_injects_headers_to_backend() {
        // Stand up a tiny "backend" on an ephemeral port that captures the
        // first request bytes, then start the proxy in front of it on
        // another ephemeral port and assert the forwarded request contains
        // identity headers.
        use std::sync::mpsc;

        let backend_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let backend_port = backend_listener.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        thread::spawn(move || {
            if let Ok((mut s, _)) = backend_listener.accept() {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
                loop {
                    match s.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&chunk[..n]);
                            if find_subsequence(&buf, b"\r\n\r\n").is_some() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
                let _ = tx.send(buf);
            }
        });

        let (_proxy_handle, proxy_port) = start_proxy(0, backend_port, fixture()).unwrap();

        let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).unwrap();
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        let mut response = Vec::new();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let _ = client.read_to_end(&mut response);

        let received = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let received_str = std::str::from_utf8(&received).unwrap();
        assert!(received_str.contains("X-Floo-User-Email: jane@acme.com\r\n"));
        assert!(received_str.contains("X-Floo-User-Role: member\r\n"));
        assert!(received_str.contains("GET / HTTP/1.1\r\n"));
    }
}
