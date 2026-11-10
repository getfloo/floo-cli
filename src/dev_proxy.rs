//! Local proxy for `floo dev`: fixture-user identity headers for accounts-mode
//! apps, and `/api/auth` forwarded to dev's sign-in service for User accounts apps.
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
//! For a User accounts app, requests under `/api/auth` go to the dev
//! environment's sign-in service instead (`FLOO_AUTH_URL`), so local pages sign
//! in with first-party cookies on localhost against dev's users.
//!
//! Pure stdlib (plus the CLI's blocking HTTP client for the sign-in hop). One thread per accepted
//! connection. Suitable for dev traffic; not a hardened production proxy.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
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

/// Bind a proxy on `listen_port` (loopback) forwarding to the local service on `backend_port`,
/// injecting the fixture user's identity headers into every request.
///
/// Pass `listen_port = 0` to let the OS choose. Returns the actually-bound port
/// alongside the listener thread handle. The thread runs until the listener is
/// dropped or the process exits — there is no graceful shutdown signal because
/// `floo dev` always exits the whole process at the end of the session.
/// What the proxy adds in front of one local service.
#[derive(Clone, Debug, Default)]
pub struct ProxyRole {
    /// Identity headers injected on every request (accounts-mode + `--fixture-user`).
    pub user: Option<FixtureUser>,
    /// The dev environment's `FLOO_AUTH_URL`; requests under its path go there.
    pub sign_in: Option<String>,
}

pub fn start_proxy(
    listen_port: u16,
    backend_port: u16,
    role: ProxyRole,
) -> std::io::Result<(JoinHandle<()>, u16)> {
    let listener = TcpListener::bind(("127.0.0.1", listen_port))?;
    let bound_port = listener.local_addr()?.port();
    let role = Arc::new(role);
    let handle = thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(client) => {
                    let role = Arc::clone(&role);
                    thread::spawn(move || {
                        let _ = handle_connection(client, backend_port, &role);
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
    role: &ProxyRole,
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
    if let Some(sign_in) = &role.sign_in {
        if let Some(target) = sign_in_target(&headers, sign_in) {
            return forward_sign_in(client, &headers, body_prefix, &target);
        }
    }
    let modified_headers = match &role.user {
        Some(user) => inject_identity_headers(&headers, user),
        None => headers,
    };

    let mut backend = match connect_local(backend_port) {
        Ok(backend) => backend,
        Err(error) => {
            return bad_gateway(
                client,
                &format!(
                    "floo dev: nothing answered on localhost:{backend_port} (tried 127.0.0.1 and ::1): {error}. \
Is the service's dev_command running on port {backend_port}?"
                ),
            );
        }
    };
    backend.set_read_timeout(Some(Duration::from_secs(60)))?;
    backend.write_all(&modified_headers)?;
    if !body_prefix.is_empty() {
        backend.write_all(&body_prefix)?;
    }
    backend.flush()?;

    pipe_bidirectional(client, backend);
    Ok(())
}

/// The sign-in URL for a request under the auth path, or None for any other path.
fn sign_in_target(request_head: &[u8], auth_url: &str) -> Option<String> {
    let line =
        std::str::from_utf8(&request_head[..find_subsequence(request_head, b"\r\n")?]).ok()?;
    let path = line.split(' ').nth(1)?;
    let after_scheme = auth_url.find("://")? + 3;
    let origin_end = auth_url[after_scheme..]
        .find('/')
        .map_or(auth_url.len(), |i| after_scheme + i);
    let auth_path = &auth_url[origin_end..];
    let rest = path.strip_prefix(auth_path)?;
    (rest.is_empty() || rest.starts_with('/') || rest.starts_with('?'))
        .then(|| format!("{}{path}", &auth_url[..origin_end]))
}

/// Send one request to dev's sign-in service and write its answer back. Cookies
/// come back without a Domain, so the browser keeps them for localhost.
fn forward_sign_in(
    mut client: TcpStream,
    request_head: &[u8],
    mut body: Vec<u8>,
    target: &str,
) -> std::io::Result<()> {
    let head = String::from_utf8_lossy(request_head);
    let mut lines = head.split("\r\n");
    let method = lines
        .next()
        .and_then(|line| line.split(' ').next())
        .unwrap_or("GET")
        .to_string();
    let mut headers = reqwest::header::HeaderMap::new();
    let mut length = 0usize;
    for line in lines.filter(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
        if name == "content-length" {
            length = value.parse().unwrap_or(0);
        }
        if matches!(
            name.as_str(),
            "host" | "connection" | "content-length" | "accept-encoding"
        ) {
            continue;
        }
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(name.as_bytes()),
            reqwest::header::HeaderValue::from_str(value),
        ) {
            headers.append(name, value);
        }
    }
    while body.len() < length {
        let mut chunk = vec![0u8; length - body.len()];
        let n = client.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    let response = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .and_then(|http| {
            http.request(method.parse().unwrap_or(reqwest::Method::GET), target)
                .headers(headers)
                .body(body)
                .send()
        });
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            return bad_gateway(
                client,
                &format!("floo dev could not reach dev's sign-in service: {error}"),
            );
        }
    };
    let status = response.status();
    let mut out = format!(
        "HTTP/1.1 {} {}\r\n",
        status.as_u16(),
        status.canonical_reason().unwrap_or("")
    );
    for (name, value) in response.headers() {
        if matches!(
            name.as_str(),
            "transfer-encoding" | "connection" | "content-length"
        ) {
            continue;
        }
        out.push_str(&format!(
            "{name}: {}\r\n",
            String::from_utf8_lossy(value.as_bytes())
        ));
    }
    let body = response.bytes().unwrap_or_default();
    out.push_str(&format!(
        "content-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    ));
    client.write_all(out.as_bytes())?;
    client.write_all(&body)?;
    client.flush()
}

/// Connect to a service on this machine. Dev servers listen on 127.0.0.1 or,
/// like Vite and Nuxt on macOS, only on ::1, so try both.
pub fn connect_local(port: u16) -> std::io::Result<TcpStream> {
    TcpStream::connect(
        &[
            SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            SocketAddr::from((Ipv6Addr::LOCALHOST, port)),
        ][..],
    )
}

fn bad_gateway(mut client: TcpStream, message: &str) -> std::io::Result<()> {
    write!(
        client,
        "HTTP/1.1 502 Bad Gateway\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{message}",
        message.len()
    )
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
    fn sign_in_target_matches_only_the_auth_path() {
        let auth = "https://shop.getfloo.app/api/auth";
        let target =
            |line: &str| sign_in_target(format!("{line}\r\nHost: x\r\n\r\n").as_bytes(), auth);
        assert_eq!(
            target("POST /api/auth/sign-in/email HTTP/1.1").as_deref(),
            Some("https://shop.getfloo.app/api/auth/sign-in/email")
        );
        assert_eq!(
            target("GET /api/auth?x=1 HTTP/1.1").as_deref(),
            Some("https://shop.getfloo.app/api/auth?x=1")
        );
        assert_eq!(target("GET /api/authors HTTP/1.1"), None);
        assert_eq!(target("GET / HTTP/1.1"), None);
    }

    #[test]
    fn sign_in_requests_go_to_the_sign_in_service_with_every_cookie_back() {
        let service = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let service_port = service.local_addr().unwrap().port();
        thread::spawn(move || {
            if let Ok((mut s, _)) = service.accept() {
                let mut buf = [0u8; 4096];
                let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = s.read(&mut buf);
                let _ = s.write_all(
                    b"HTTP/1.1 200 OK\r\nSet-Cookie: a=1; Path=/\r\nSet-Cookie: b=2; Path=/\r\n\
                      Content-Length: 4\r\nConnection: close\r\n\r\nnull",
                );
            }
        });
        // No backend: a sign-in request never reaches the local service.
        let role = ProxyRole {
            user: None,
            sign_in: Some(format!("http://127.0.0.1:{service_port}/api/auth")),
        };
        let (_proxy_handle, proxy_port) = start_proxy(0, 9, role).unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).unwrap();
        client
            .write_all(b"GET /api/auth/get-session HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut response = Vec::new();
        let _ = client.read_to_end(&mut response);
        let response = String::from_utf8_lossy(&response).to_lowercase();
        assert!(response.starts_with("http/1.1 200"), "{response}");
        assert!(response.contains("set-cookie: a=1"), "{response}");
        assert!(response.contains("set-cookie: b=2"), "{response}");
        assert!(response.ends_with("null"), "{response}");
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

        let role = ProxyRole {
            user: Some(fixture()),
            sign_in: None,
        };
        let (_proxy_handle, proxy_port) = start_proxy(0, backend_port, role).unwrap();

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

    fn get_through_proxy(backend_port: u16) -> String {
        let (_handle, proxy_port) = start_proxy(0, backend_port, ProxyRole::default()).unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        let _ = client.read_to_string(&mut response);
        response
    }

    #[test]
    fn proxy_reaches_a_backend_listening_only_on_ipv6_loopback() {
        // Vite and Nuxt on macOS bind ::1 only. Skip where the host has no IPv6 loopback.
        let Ok(backend) = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)) else {
            return;
        };
        let backend_port = backend.local_addr().unwrap().port();
        thread::spawn(move || {
            let (mut s, _) = backend.accept().unwrap();
            let _ = s.read(&mut [0u8; 1024]);
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
        });
        assert!(get_through_proxy(backend_port).starts_with("HTTP/1.1 200 OK"));
    }

    #[test]
    fn proxy_answers_502_when_nothing_listens_on_the_service_port() {
        let port = TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let response = get_through_proxy(port);
        assert!(
            response.starts_with("HTTP/1.1 502 Bad Gateway"),
            "{response}"
        );
        assert!(response.contains(&format!("nothing answered on localhost:{port}")));
    }
}
