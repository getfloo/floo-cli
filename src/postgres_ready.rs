//! Verify that a dev session's Postgres authorization is actually in effect.
//!
//! `POST /v1/apps/{id}/dev-session` returns `postgres_authorized: true` when the
//! API's Cloud SQL patch was **accepted**. Cloud SQL applies it asynchronously —
//! seconds on a quiet instance, tens of seconds under load, unbounded per
//! Google's docs. So "accepted" is not "you can connect".
//!
//! We do not ask the control plane whether it thinks the rule is applied. Google
//! publishes no guarantee about when a settings change becomes visible in
//! `instances.get`, nor read-after-write once its Operation is DONE, and a
//! control-plane read can never speak for the data plane anyway. The
//! authoritative answer to "can this machine reach Postgres?" is observable at
//! exactly one place: this machine, by opening a socket.
//!
//! Measured against prod on 2026-07-10: from a non-allow-listed IP a connect to
//! the instance's public endpoint **times out** (the authorized-networks
//! firewall silently drops the packets, so there is no RST); from an
//! allow-listed IP it completes. A successful TCP connect is therefore a
//! sufficient, premise-free signal, and it subsumes propagation delay for free.
//!
//! We connect and immediately drop. No TLS, no startup packet, no credentials:
//! reaching the listener is the whole question. Postgres logs an aborted
//! connection at most.

use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// How long a single connect attempt may block before we call it "not yet".
///
/// A blocked connect does not fail fast — the firewall drops packets, so the
/// client waits out its own timeout. Keep it short enough that the overall
/// budget is checked often, long enough that a slow network path is not
/// mistaken for a firewall drop.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(3);

/// Delay between attempts. The authorized-networks operation is the thing we are
/// waiting on; polling faster than it can possibly complete just burns syscalls.
const RETRY_DELAY: Duration = Duration::from_secs(2);

/// Total wall-clock budget. Generous: observed applies range from ~5s (quiet
/// instance) to ~21s (under load), and Google documents no upper bound. Past
/// this we stop blocking the developer and let their services start.
pub const DEFAULT_BUDGET: Duration = Duration::from_secs(90);

#[derive(Debug, PartialEq, Eq)]
pub enum Readiness {
    /// A TCP connection to the instance succeeded — the IP is allow-listed.
    Authorized { waited: Duration },
    /// The budget elapsed without a successful connection.
    TimedOut { waited: Duration },
}

/// The Postgres endpoint the API rewrote into this session's env vars.
///
/// The API rewrites `PGHOST`/`PGPORT` (and `DATABASE_URL`) to the Cloud SQL
/// public IP only when it accepted the authorized-network patch, so their
/// presence is what marks a session as having direct-Postgres intent. Any one
/// service's map carries them; they are identical across services.
pub fn endpoint_from_env(
    services: &std::collections::HashMap<String, std::collections::HashMap<String, String>>,
) -> Option<(String, u16)> {
    for env in services.values() {
        let host = match env.get("PGHOST") {
            Some(h) if !h.is_empty() => h,
            _ => continue,
        };
        // Loopback means the API did not rewrite for direct access (no managed
        // Postgres, or the rewrite was skipped) — there is nothing to verify.
        if host == "127.0.0.1" || host == "localhost" {
            continue;
        }
        let port = env
            .get("PGPORT")
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(5432);
        return Some((host.clone(), port));
    }
    None
}

/// True when a TCP connection to `host:port` completes within `ATTEMPT_TIMEOUT`.
///
/// Resolution failure and refusal are both "not reachable" — never a hard error.
/// A refusal (RST) would mean something is listening but declining, which the
/// authorized-networks firewall does not do; we treat it as not-yet regardless.
fn tcp_reachable(host: &str, port: u16) -> bool {
    let Ok(mut addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    addrs.any(|addr| TcpStream::connect_timeout(&addr, ATTEMPT_TIMEOUT).is_ok())
}

/// Poll until the endpoint accepts a connection, or the budget elapses.
///
/// `probe` is injected so the retry/budget logic is testable without sockets.
/// `sleep` likewise, so tests never actually wait.
pub fn wait_until_reachable<P, S>(budget: Duration, mut probe: P, mut sleep: S) -> Readiness
where
    P: FnMut() -> bool,
    S: FnMut(Duration),
{
    let start = Instant::now();
    loop {
        if probe() {
            return Readiness::Authorized {
                waited: start.elapsed(),
            };
        }
        // Check the budget BEFORE sleeping: a probe that returns right at the
        // deadline should not buy itself another RETRY_DELAY of the user's time.
        if start.elapsed() >= budget {
            return Readiness::TimedOut {
                waited: start.elapsed(),
            };
        }
        sleep(RETRY_DELAY);
    }
}

/// Wait for a dev session's Postgres authorization to take effect.
///
/// Returns `None` when the session has no direct-Postgres endpoint to verify.
pub fn verify(
    services: &std::collections::HashMap<String, std::collections::HashMap<String, String>>,
    budget: Duration,
) -> Option<Readiness> {
    let (host, port) = endpoint_from_env(services)?;
    Some(wait_until_reachable(
        budget,
        || tcp_reachable(&host, port),
        std::thread::sleep,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn services(pairs: &[(&str, &[(&str, &str)])]) -> HashMap<String, HashMap<String, String>> {
        pairs
            .iter()
            .map(|(name, env)| {
                let map = env
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                    .collect();
                ((*name).to_string(), map)
            })
            .collect()
    }

    #[test]
    fn endpoint_is_read_from_the_rewritten_pg_vars() {
        let svc = services(&[("api", &[("PGHOST", "35.202.217.249"), ("PGPORT", "5432")])]);
        assert_eq!(
            endpoint_from_env(&svc),
            Some((String::from("35.202.217.249"), 5432))
        );
    }

    #[test]
    fn endpoint_defaults_the_port_when_absent() {
        let svc = services(&[("api", &[("PGHOST", "35.202.217.249")])]);
        assert_eq!(
            endpoint_from_env(&svc),
            Some((String::from("35.202.217.249"), 5432))
        );
    }

    #[test]
    fn loopback_endpoint_is_not_verifiable() {
        // The API only rewrites PGHOST to the public IP when it accepted the
        // authorized-network patch; a loopback value means there is nothing to wait on.
        let svc = services(&[("api", &[("PGHOST", "127.0.0.1"), ("PGPORT", "5432")])]);
        assert_eq!(endpoint_from_env(&svc), None);
        let svc = services(&[("api", &[("PGHOST", "localhost")])]);
        assert_eq!(endpoint_from_env(&svc), None);
    }

    #[test]
    fn no_pg_vars_means_nothing_to_verify() {
        let svc = services(&[("api", &[("MY_VAR", "hello")])]);
        assert_eq!(endpoint_from_env(&svc), None);
        assert!(verify(&svc, DEFAULT_BUDGET).is_none());
    }

    #[test]
    fn empty_pghost_is_skipped() {
        let svc = services(&[("api", &[("PGHOST", ""), ("PGPORT", "5432")])]);
        assert_eq!(endpoint_from_env(&svc), None);
    }

    #[test]
    fn a_service_without_pg_vars_does_not_shadow_one_with_them() {
        let svc = services(&[
            ("web", &[("MY_VAR", "hello")]),
            ("api", &[("PGHOST", "35.202.217.249"), ("PGPORT", "6543")]),
        ]);
        assert_eq!(
            endpoint_from_env(&svc),
            Some((String::from("35.202.217.249"), 6543))
        );
    }

    #[test]
    fn succeeds_on_the_first_probe_without_sleeping() {
        let mut slept = 0u32;
        let out = wait_until_reachable(DEFAULT_BUDGET, || true, |_| slept += 1);
        assert!(matches!(out, Readiness::Authorized { .. }));
        assert_eq!(slept, 0, "a ready endpoint must not delay the developer");
    }

    #[test]
    fn retries_until_the_endpoint_comes_up() {
        let mut attempts = 0;
        let mut slept = 0u32;
        let out = wait_until_reachable(
            DEFAULT_BUDGET,
            || {
                attempts += 1;
                attempts >= 3
            },
            |_| slept += 1,
        );
        assert!(matches!(out, Readiness::Authorized { .. }));
        assert_eq!(attempts, 3);
        assert_eq!(slept, 2, "one sleep between each pair of attempts");
    }

    #[test]
    fn gives_up_at_the_budget_instead_of_blocking_forever() {
        // A zero budget still probes once: the endpoint may already be up, and a
        // developer who set no patience should not be told "timed out" without a look.
        let mut attempts = 0;
        let out = wait_until_reachable(
            Duration::ZERO,
            || {
                attempts += 1;
                false
            },
            |_| panic!("must not sleep once the budget is spent"),
        );
        assert!(matches!(out, Readiness::TimedOut { .. }));
        assert_eq!(attempts, 1);
    }

    #[test]
    fn unresolvable_host_is_unreachable_not_a_panic() {
        assert!(!tcp_reachable("this-host-does-not-exist.invalid", 5432));
    }
}
