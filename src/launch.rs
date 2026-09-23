//! floo-launch: the entrypoint floo wraps around every customer container
//! (getfloo/floo#3133).
//!
//! At start it decrypts each `FLOO_SEALED_<KEY>` with the app's Cloud KMS key
//! (`FLOO_KMS_KEY`), authenticating as the container's own service account
//! through the metadata server, and exports the plaintext as `<KEY>`. It then
//! runs the customer's command as its child, masks every secret value in the
//! child's stdout and stderr, forwards SIGTERM/SIGINT, reaps orphans (it runs
//! as PID 1), and exits with the child's code.
//!
//! If anything sealed cannot be unsealed it exits 1 before the command runs:
//! an app never starts without its secrets.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::thread;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

const SEALED_PREFIX: &str = "FLOO_SEALED_";
const KMS_KEY_VAR: &str = "FLOO_KMS_KEY";
const METADATA_TOKEN_URL: &str =
    "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token";
const KMS_API: &str = "https://cloudkms.googleapis.com/v1";
/// Shorter values would mask ordinary words and numbers in logs.
const MIN_MASKED_LEN: usize = 8;

static CHILD_PID: AtomicI32 = AtomicI32::new(0);

fn main() {
    let command: Vec<OsString> = std::env::args_os().skip(1).collect();
    if command.is_empty() {
        fail("usage: floo-launch <command> [args...]");
    }
    let sealed = sealed_vars(std::env::vars());
    let secrets = if sealed.is_empty() {
        Vec::new()
    } else {
        let key = std::env::var(KMS_KEY_VAR)
            .unwrap_or_else(|_| fail(&format!("{SEALED_PREFIX}* is set but {KMS_KEY_VAR} is not")));
        unseal(&sealed, &key, METADATA_TOKEN_URL, KMS_API).unwrap_or_else(|e| fail(&e))
    };
    let code = supervise(
        &command,
        &secrets,
        Box::new(std::io::stdout()),
        Box::new(std::io::stderr()),
    );
    std::process::exit(code);
}

fn fail(message: &str) -> ! {
    eprintln!("floo-launch: {message}");
    std::process::exit(1);
}

/// `(KEY, ciphertext)` for every `FLOO_SEALED_<KEY>` variable.
fn sealed_vars(vars: impl Iterator<Item = (String, String)>) -> Vec<(String, String)> {
    vars.filter_map(|(name, value)| {
        let key = name.strip_prefix(SEALED_PREFIX)?;
        (!key.is_empty()).then(|| (key.to_string(), value))
    })
    .collect()
}

/// Decrypt every sealed value with the app's KMS key. Returns `(KEY, plaintext)`.
fn unseal(
    sealed: &[(String, String)],
    kms_key: &str,
    token_url: &str,
    kms_api: &str,
) -> Result<Vec<(String, String)>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let token: serde_json::Value = client
        .get(token_url)
        .header("Metadata-Flavor", "Google")
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.json())
        .map_err(|e| format!("metadata token: {e}"))?;
    let token = token["access_token"]
        .as_str()
        .ok_or("metadata token: response has no access_token")?;

    sealed
        .iter()
        .map(|(key, ciphertext)| {
            let body: serde_json::Value = client
                .post(format!("{kms_api}/{kms_key}:decrypt"))
                .bearer_auth(token)
                .json(&serde_json::json!({ "ciphertext": ciphertext }))
                .send()
                .and_then(|r| r.error_for_status())
                .and_then(|r| r.json())
                .map_err(|e| format!("unseal {key}: {e}"))?;
            let plaintext = body["plaintext"]
                .as_str()
                .and_then(|p| BASE64.decode(p).ok())
                .and_then(|p| String::from_utf8(p).ok())
                .ok_or_else(|| format!("unseal {key}: response has no UTF-8 plaintext"))?;
            Ok((key.clone(), plaintext))
        })
        .collect()
}

/// Run `command` with the secrets exported, mask them in its output, and
/// return its exit code.
fn supervise(
    command: &[OsString],
    secrets: &[(String, String)],
    out: Box<dyn Write + Send>,
    err: Box<dyn Write + Send>,
) -> i32 {
    let mut cmd = Command::new(&command[0]);
    cmd.args(&command[1..])
        .env_remove(KMS_KEY_VAR)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in secrets {
        cmd.env_remove(format!("{SEALED_PREFIX}{key}"))
            .env(key, value);
    }
    #[expect(
        clippy::zombie_processes,
        reason = "reap_until reaps it with waitpid(-1)"
    )]
    let mut child = cmd
        .spawn()
        .unwrap_or_else(|e| fail(&format!("start {:?}: {e}", command[0])));
    let child_pid = child.id() as i32;
    CHILD_PID.store(child_pid, Ordering::SeqCst);
    forward_signals();

    let needles = mask_needles(secrets);
    let pumps = [
        pump(
            child.stdout.take().expect("piped stdout"),
            out,
            needles.clone(),
        ),
        pump(child.stderr.take().expect("piped stderr"), err, needles),
    ];
    let code = reap_until(child_pid);
    for pump in pumps {
        let _ = pump.join();
    }
    code
}

/// Copy `input` to `output` line by line with every secret value replaced.
fn pump(
    input: impl Read + Send + 'static,
    mut output: Box<dyn Write + Send>,
    needles: Vec<(Vec<u8>, Vec<u8>)>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(input);
        let mut line = Vec::new();
        while reader.read_until(b'\n', &mut line).unwrap_or(0) > 0 {
            let _ = output.write_all(&mask(&line, &needles));
            let _ = output.flush();
            line.clear();
        }
    })
}

/// `(value, "[REDACTED:KEY]")` pairs, longest value first. A multi-line
/// secret (a PEM key) also masks each of its lines, since output is
/// processed a line at a time.
fn mask_needles(secrets: &[(String, String)]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut needles: Vec<(Vec<u8>, Vec<u8>)> = secrets
        .iter()
        .flat_map(|(key, value)| {
            let replacement = format!("[REDACTED:{key}]").into_bytes();
            std::iter::once(value.as_str())
                .chain(value.lines())
                .map(|part| part.trim())
                .filter(|part| part.len() >= MIN_MASKED_LEN)
                .map(move |part| (part.as_bytes().to_vec(), replacement.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    needles.sort_by_key(|needle| std::cmp::Reverse(needle.0.len()));
    needles.dedup_by(|a, b| a.0 == b.0);
    needles
}

fn mask(line: &[u8], needles: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut masked = line.to_vec();
    for (needle, replacement) in needles {
        let mut result = Vec::with_capacity(masked.len());
        let mut rest = masked.as_slice();
        while let Some(at) = rest
            .windows(needle.len())
            .position(|w| w == needle.as_slice())
        {
            result.extend_from_slice(&rest[..at]);
            result.extend_from_slice(replacement);
            rest = &rest[at + needle.len()..];
        }
        result.extend_from_slice(rest);
        masked = result;
    }
    masked
}

extern "C" fn forward(signal: libc::c_int) {
    let pid = CHILD_PID.load(Ordering::SeqCst);
    if pid > 0 {
        // SAFETY: kill is async-signal-safe.
        unsafe { libc::kill(pid, signal) };
    }
}

fn forward_signals() {
    for signal in [libc::SIGTERM, libc::SIGINT] {
        // SAFETY: `forward` only loads an atomic and calls kill, both
        // async-signal-safe.
        unsafe { libc::signal(signal, forward as *const () as libc::sighandler_t) };
    }
}

/// Reap every exited process until the child exits (as PID 1 the launcher
/// inherits the child's orphans), then return the child's exit code, using
/// 128 + signal when it was killed.
fn reap_until(child_pid: i32) -> i32 {
    loop {
        let mut status = 0;
        // SAFETY: waitpid writes only to `status`.
        let pid = unsafe { libc::waitpid(-1, &mut status, 0) };
        if pid == child_pid {
            if libc::WIFEXITED(status) {
                return libc::WEXITSTATUS(status);
            }
            return 128 + libc::WTERMSIG(status);
        }
        if pid == -1 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            fail("lost track of the child process");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn secrets(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn sealed_vars_strips_the_prefix_and_ignores_the_rest() {
        let vars = secrets(&[
            ("FLOO_SEALED_API_KEY", "c1"),
            ("PATH", "/bin"),
            ("FLOO_SEALED_", "empty-name"),
        ]);
        assert_eq!(sealed_vars(vars.into_iter()), secrets(&[("API_KEY", "c1")]));
    }

    #[test]
    fn mask_replaces_secret_values_and_leaves_short_ones() {
        let needles = mask_needles(&secrets(&[("API_KEY", "sk_live_abc123"), ("FLAG", "true")]));
        assert_eq!(
            mask(b"key=sk_live_abc123 flag=true\n", &needles),
            b"key=[REDACTED:API_KEY] flag=true\n"
        );
    }

    #[test]
    fn mask_covers_each_line_of_a_multi_line_secret() {
        let pem = "-----BEGIN KEY-----\nMIIEvQIBADANBgkq\n-----END KEY-----";
        let needles = mask_needles(&secrets(&[("PEM", pem)]));
        assert_eq!(mask(b"MIIEvQIBADANBgkq\n", &needles), b"[REDACTED:PEM]\n");
    }

    #[test]
    fn unseal_decrypts_with_the_metadata_token() {
        let mut server = mockito::Server::new();
        let _token = server
            .mock("GET", "/token")
            .match_header("Metadata-Flavor", "Google")
            .with_body(r#"{"access_token":"tok"}"#)
            .create();
        let _decrypt = server
            .mock("POST", "/projects/p/cryptoKeys/app:decrypt")
            .match_header("authorization", "Bearer tok")
            .match_body(mockito::Matcher::Json(
                serde_json::json!({"ciphertext": "c1"}),
            ))
            .with_body(format!(
                r#"{{"plaintext":"{}"}}"#,
                BASE64.encode("sk_live_abc123")
            ))
            .create();

        let unsealed = unseal(
            &secrets(&[("API_KEY", "c1")]),
            "projects/p/cryptoKeys/app",
            &format!("{}/token", server.url()),
            &server.url(),
        );
        assert_eq!(unsealed, Ok(secrets(&[("API_KEY", "sk_live_abc123")])));
    }

    #[test]
    fn unseal_fails_when_kms_refuses() {
        let mut server = mockito::Server::new();
        let _token = server
            .mock("GET", "/token")
            .with_body(r#"{"access_token":"tok"}"#)
            .create();
        let _decrypt = server
            .mock("POST", "/projects/p/cryptoKeys/app:decrypt")
            .with_status(403)
            .create();

        let unsealed = unseal(
            &secrets(&[("API_KEY", "c1")]),
            "projects/p/cryptoKeys/app",
            &format!("{}/token", server.url()),
            &server.url(),
        );
        assert!(unsealed.unwrap_err().starts_with("unseal API_KEY:"));
    }

    #[test]
    fn supervise_exports_masks_and_returns_the_exit_code() {
        let out = Captured::default();
        let err = Captured::default();
        let command: Vec<OsString> = [
            "sh",
            "-c",
            "echo \"$API_KEY\"; echo \"$API_KEY\" >&2; exit 3",
        ]
        .iter()
        .map(OsString::from)
        .collect();

        let code = supervise(
            &command,
            &secrets(&[("API_KEY", "sk_live_abc123")]),
            Box::new(out.clone()),
            Box::new(err.clone()),
        );

        assert_eq!(code, 3);
        assert_eq!(out.0.lock().unwrap().as_slice(), b"[REDACTED:API_KEY]\n");
        assert_eq!(err.0.lock().unwrap().as_slice(), b"[REDACTED:API_KEY]\n");
    }
}
