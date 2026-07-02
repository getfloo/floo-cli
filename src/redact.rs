//! Secret redaction for `--json` output.
//!
//! Agents pipe `--json` stdout into transcripts and logs by default, so
//! anything the CLI emits there must assume it will be persisted. This
//! module walks every JSON payload heading to stdout and replaces
//! credential-shaped values with `***REDACTED***` unless the caller
//! explicitly passes `--reveal-secrets`.
//!
//! Detection has three layers, applied in order:
//!
//! 1. **Field name** — JSON object keys whose lowercase form matches
//!    `SECRET_FIELD_NAMES` (e.g. `password`, `api_key`, `database_url`).
//! 2. **Env-var-shaped key** — UPPER_SNAKE_CASE keys whose name contains
//!    a secret token (`PASSWORD`, `SECRET`, `TOKEN`, `KEY`, …) and isn't
//!    on the `ENV_VAR_ALLOWLIST`. Catches the
//!    `services.web.DATABASE_URL` shape that `floo dev --json` emits.
//! 3. **Value content** — strings whose body matches a known credential
//!    pattern (URI userinfo, floo API key, AWS access key, bearer
//!    token, JWT). Last-resort net for surprise leaks from API
//!    passthroughs like `floo deploys watch` audit payloads.
//!
//! When ANY of these fire, the top-level payload also gets a
//! `"contains_secrets": true` marker so agent harnesses can detect-and-
//! refuse before the response hits a transcript — this fires whether or
//! not redaction was applied, so `--reveal-secrets` doesn't strip the
//! signal.
//!
//! Mirrors the API-side redactor in `api/app/services/logs.py`. When
//! adding patterns, update both.

use std::sync::atomic::{AtomicBool, Ordering};

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{Map, Value};

static REVEAL_SECRETS: AtomicBool = AtomicBool::new(false);

pub fn set_reveal_secrets(enabled: bool) {
    REVEAL_SECRETS.store(enabled, Ordering::SeqCst);
}

pub fn is_reveal_secrets() -> bool {
    REVEAL_SECRETS.load(Ordering::SeqCst)
}

pub const REDACTED_PLACEHOLDER: &str = "***REDACTED***";
pub const CONTAINS_SECRETS_KEY: &str = "contains_secrets";

/// Lower-case JSON field names whose string value is always a secret.
///
/// Mirrors `_SECRET_KEY_PATTERN` in `api/app/services/logs.py` plus the
/// CLI-specific response field names (`generated_password`, etc.). Keep
/// these as exact lowercase matches — substring matching here would
/// false-positive on innocuous fields like `api_endpoint` or
/// `private_key_id`.
const SECRET_FIELD_NAMES: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "secret",
    "client_secret",
    "session_secret",
    "secret_key",
    "secret_key_base",
    "api_key",
    "apikey",
    "auth_token",
    "access_token",
    "refresh_token",
    "token",
    "private_key",
    "encryption_key",
    "jwt_secret",
    "database_url",
    "redis_url",
    "connection_string",
    "connection_url",
    "webhook_url",
    "invite_url",
    "generated_password",
    "encrypted_value",
];

/// Substring tokens that mark an env-var-style key (UPPER_SNAKE_CASE) as
/// secret-bearing. Uppercase, matched case-sensitively after the key has
/// been confirmed as env-var-shaped.
const ENV_VAR_SECRET_TOKENS: &[&str] = &[
    "PASSWORD",
    "PASSWD",
    "SECRET",
    "TOKEN",
    "PRIVATE",
    "CREDENTIAL",
    "CERT",
    "AUTH",
    // KEY is broad but high-signal — DATABASE_URL_KEY, STRIPE_KEY,
    // SECRET_KEY_BASE etc. are real. The allowlist below catches the
    // common false positives (PUBLIC_KEY, AWS_REGION, …).
    "KEY",
    "DSN",
    "DATABASE_URL",
    "REDIS_URL",
    "MONGO_URL",
    "MONGODB_URL",
];

/// Env-var names that LOOK secret (match a token above) but aren't.
const ENV_VAR_ALLOWLIST: &[&str] = &[
    "PORT",
    "HOST",
    "PUBLIC_KEY",
    "AWS_REGION",
    "AWS_DEFAULT_REGION",
    "LOG_LEVEL",
    "ENABLE_AUTH",
    "AUTH_REQUIRED",
    "REQUIRE_AUTH",
];

static URI_CREDENTIAL_RE: Lazy<Regex> = Lazy::new(|| {
    // postgres://user:pass@host, mysql://user:pass@..., redis://:pass@... .
    // Mirrors api/app/services/logs.py::_URI_CREDENTIAL_RE.
    Regex::new(r"(?i)\b[a-z][a-z0-9+.\-]*://[^@\s/]+:[^@\s/]+@").unwrap()
});

static FLOO_API_KEY_RE: Lazy<Regex> = Lazy::new(|| {
    // floo_<urlsafe-token-32+>. Mirrors api `_FLOO_API_KEY_RE`.
    Regex::new(r"\bfloo_[A-Za-z0-9_\-]{20,}\b").unwrap()
});

static AWS_ACCESS_KEY_RE: Lazy<Regex> = Lazy::new(|| {
    // AKIA / ASIA / AGPA / AIDA / A3T<X> followed by 16 alphanumeric
    // chars. Mirrors api `_AWS_ACCESS_KEY_RE`.
    Regex::new(r"\b(?:A3T[A-Z0-9]|AKIA|ASIA|AGPA|AIDA)[A-Z0-9]{16}\b").unwrap()
});

static BEARER_TOKEN_RE: Lazy<Regex> = Lazy::new(|| {
    // `Bearer <token>` literal as it appears in Authorization headers
    // that occasionally make it into log lines.
    Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9._~+/=\-]{12,}").unwrap()
});

static JWT_RE: Lazy<Regex> = Lazy::new(|| {
    // header.payload.signature in base64url. Conservative length floor
    // avoids matching short opaque IDs that happen to contain dots.
    Regex::new(r"\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\b").unwrap()
});

/// Walk `value` and redact any credential-shaped fields in place. Returns
/// `true` if the payload contained at least one secret-shaped value. A
/// `true` return is what triggers the top-level `contains_secrets`
/// marker — it fires regardless of whether `is_reveal_secrets()` is on,
/// so agents can refuse the payload even when the user opted in.
pub fn process_in_place(value: &mut Value) -> bool {
    let reveal = is_reveal_secrets();
    walk(value, reveal)
}

fn walk(value: &mut Value, reveal: bool) -> bool {
    match value {
        Value::Object(map) => walk_object(map, reveal),
        Value::Array(arr) => {
            let mut found = false;
            for v in arr {
                if walk(v, reveal) {
                    found = true;
                }
            }
            found
        }
        Value::String(s) if value_looks_credential(s) => {
            if !reveal {
                *s = REDACTED_PLACEHOLDER.into();
            }
            true
        }
        _ => false,
    }
}

fn walk_object(map: &mut Map<String, Value>, reveal: bool) -> bool {
    let mut found = false;

    // Pattern: `{ "key": "<NAME>", "value": "<VALUE>", ... }` — the
    // `EnvVar` shape used by env list / env get / env set responses.
    // We only redact the `value` field if `key` (which is the env-var
    // *name*, not a metadata field) names a secret. Without this guard,
    // we'd either over-redact every `value` field (breaking dry-run /
    // config payloads) or under-redact when the env-var name is the
    // only signal of secrecy.
    let env_var_name_is_secret = matches!(
        (map.get("key"), map.get("value")),
        (Some(Value::String(name)), Some(Value::String(_)))
            if env_var_key_is_secret(name) || lowercase_field_is_secret(name)
    );
    if env_var_name_is_secret {
        if let Some(slot) = map.get_mut("value") {
            if let Value::String(s) = slot {
                if !s.is_empty() {
                    found = true;
                    if !reveal {
                        *slot = Value::String(REDACTED_PLACEHOLDER.into());
                    }
                }
            }
        }
    }

    let keys: Vec<String> = map.keys().cloned().collect();
    for key in keys {
        let key_secret = lowercase_field_is_secret(&key) || env_var_key_is_secret(&key);
        let Some(child) = map.get_mut(&key) else {
            continue;
        };
        match child {
            Value::String(s) => {
                if key_secret {
                    if !s.is_empty() {
                        found = true;
                        if !reveal {
                            *child = Value::String(REDACTED_PLACEHOLDER.into());
                        }
                    }
                } else if value_looks_credential(s) {
                    found = true;
                    if !reveal {
                        *child = Value::String(REDACTED_PLACEHOLDER.into());
                    }
                }
            }
            Value::Object(inner) => {
                if walk_object(inner, reveal) {
                    found = true;
                }
            }
            Value::Array(arr) => {
                for v in arr.iter_mut() {
                    if walk(v, reveal) {
                        found = true;
                    }
                }
            }
            _ => {}
        }
    }

    found
}

fn lowercase_field_is_secret(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SECRET_FIELD_NAMES.iter().any(|n| *n == lower)
}

/// Whether an env-var name is secret-bearing, by the SAME rule the `--json`
/// redaction boundary uses (UPPER_SNAKE_CASE, a secret token, not on the
/// allowlist). Exposed so preflight's web-secret scan flags exactly what the
/// redactor would redact — no parallel heuristic that drifts (e.g. one that
/// flags the allowlisted `PUBLIC_KEY`).
pub fn env_var_key_is_secret(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    // Env var names are conventionally UPPER_SNAKE_CASE. Skip mixed-case
    // keys so we don't catch normal JSON field names like `apiKey`
    // (those are caught by `lowercase_field_is_secret` instead).
    let chars_ok = name
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    let has_letter = name.chars().any(|c| c.is_ascii_uppercase());
    if !chars_ok || !has_letter {
        return false;
    }
    if ENV_VAR_ALLOWLIST.contains(&name) {
        return false;
    }
    ENV_VAR_SECRET_TOKENS
        .iter()
        .any(|token| name.contains(token))
}

fn value_looks_credential(s: &str) -> bool {
    if s.len() < 8 {
        return false;
    }
    URI_CREDENTIAL_RE.is_match(s)
        || FLOO_API_KEY_RE.is_match(s)
        || AWS_ACCESS_KEY_RE.is_match(s)
        || BEARER_TOKEN_RE.is_match(s)
        || JWT_RE.is_match(s)
}

/// Classify whether an env var `{key, value}` pair holds a secret.
///
/// Uses the exact three-layer detection the JSON walker applies to a
/// `{key, value}` object: the key name (UPPER_SNAKE secret tokens, or a
/// lowercase secret field name) **or** the value's own credential shape.
/// The human-mode `floo env get` reveal gate calls this so it agrees with the
/// `--json` redactor on precisely which values are protected — the two output
/// modes must never disagree on whether a value is exposed (#1152).
///
/// An empty value is never a secret: the JSON walker skips empty strings
/// (`!s.is_empty()`), so the human gate must too, or `env get` on an
/// empty-valued secret-named key would refuse while `--json` passes it through.
pub fn is_secret(key: &str, value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    env_var_key_is_secret(key) || lowercase_field_is_secret(key) || value_looks_credential(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn redact(mut v: Value) -> (Value, bool) {
        let found = walk(&mut v, false);
        (v, found)
    }

    fn reveal(mut v: Value) -> (Value, bool) {
        let found = walk(&mut v, true);
        (v, found)
    }

    #[test]
    fn lowercase_field_password_redacted() {
        let (out, found) = redact(json!({"password": "hunter2"}));
        assert!(found);
        assert_eq!(out["password"], REDACTED_PLACEHOLDER);
    }

    #[test]
    fn lowercase_field_token_redacted() {
        let (out, found) = redact(json!({"token": "abcdef1234567890"}));
        assert!(found);
        assert_eq!(out["token"], REDACTED_PLACEHOLDER);
    }

    #[test]
    fn generated_password_redacted() {
        // Regression: `Deploy.generated_password` was leaking via
        // `floo deploys logs --json` and `floo redeploy --json`.
        let (out, found) = redact(json!({"deploy": {"generated_password": "AbCdEf12!"}}));
        assert!(found);
        assert_eq!(out["deploy"]["generated_password"], REDACTED_PLACEHOLDER);
    }

    #[test]
    fn env_var_pair_secret_name_redacts_value() {
        // Regression: `floo env get --json` returned `{key, value}`
        // with the plaintext value field.
        let (out, found) = redact(json!({"key": "DATABASE_URL", "value": "postgres://u:p@h/db"}));
        assert!(found);
        assert_eq!(out["value"], REDACTED_PLACEHOLDER);
        assert_eq!(out["key"], "DATABASE_URL");
    }

    #[test]
    fn env_var_pair_non_secret_name_keeps_value() {
        let (out, found) = redact(json!({"key": "PORT", "value": "3000"}));
        assert!(!found);
        assert_eq!(out["value"], "3000");
    }

    #[test]
    fn env_var_map_dev_session_redacts_secrets() {
        // Regression: `floo dev --json` emits
        //   { "services": { "web": { "DATABASE_URL": "postgres://..." } } }
        // with the inner map carrying the credentials.
        let payload = json!({
            "services": {
                "web": {
                    "DATABASE_URL": "postgresql://u:p@h/d",
                    "REDIS_URL": "redis://default:tok@h/0",
                    "SECRET_KEY_BASE": "abc123",
                    "PORT": "3000",
                }
            }
        });
        let (out, found) = redact(payload);
        assert!(found);
        let web = &out["services"]["web"];
        assert_eq!(web["DATABASE_URL"], REDACTED_PLACEHOLDER);
        assert_eq!(web["REDIS_URL"], REDACTED_PLACEHOLDER);
        assert_eq!(web["SECRET_KEY_BASE"], REDACTED_PLACEHOLDER);
        assert_eq!(web["PORT"], "3000");
    }

    #[test]
    fn nested_arrays_walk_through() {
        let (out, found) = redact(json!({"vars": [
            {"key": "API_KEY", "value": "k123"},
            {"key": "USER", "value": "alice"},
        ]}));
        assert!(found);
        assert_eq!(out["vars"][0]["value"], REDACTED_PLACEHOLDER);
        assert_eq!(out["vars"][1]["value"], "alice");
    }

    #[test]
    fn uri_credential_in_value_redacted_even_with_innocent_key() {
        // Last-resort net: an audit log line embeds a credential URI
        // under an innocuous key like `message`.
        let (out, found) = redact(json!({"message": "connecting to postgres://u:p@h/db now"}));
        assert!(found);
        assert_eq!(out["message"], REDACTED_PLACEHOLDER);
    }

    #[test]
    fn floo_api_key_in_value_redacted() {
        let (out, found) = redact(json!({"note": "use floo_aaaaaaaaaaaaaaaaaaaaaaaa for auth"}));
        assert!(found);
        assert_eq!(out["note"], REDACTED_PLACEHOLDER);
    }

    #[test]
    fn aws_access_key_in_value_redacted() {
        let (out, found) = redact(json!({"hint": "key AKIAIOSFODNN7EXAMPLE was rotated"}));
        assert!(found);
        assert_eq!(out["hint"], REDACTED_PLACEHOLDER);
    }

    #[test]
    fn jwt_in_value_redacted() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signaturehereXX";
        let (out, found) = redact(json!({"auth": jwt}));
        assert!(found);
        assert_eq!(out["auth"], REDACTED_PLACEHOLDER);
    }

    #[test]
    fn allowlist_keeps_port_visible() {
        let (out, found) = redact(json!({"PORT": "3000"}));
        assert!(!found);
        assert_eq!(out["PORT"], "3000");
    }

    #[test]
    fn allowlist_keeps_public_key_visible() {
        let (out, found) = redact(json!({"PUBLIC_KEY": "ssh-rsa AAAA..."}));
        assert!(!found);
        assert_eq!(out["PUBLIC_KEY"], "ssh-rsa AAAA...");
    }

    #[test]
    fn masked_value_field_is_left_alone() {
        // `EnvVar.masked_value` is already masked server-side to a fixed,
        // content-free marker (#1152); we must not redact it (it carries no
        // secret) nor mistake it for a value to protect.
        let (out, found) = redact(json!({"key": "DATABASE_URL", "masked_value": "********"}));
        assert!(!found);
        assert_eq!(out["masked_value"], "********");
    }

    #[test]
    fn is_secret_classifies_key_and_value() {
        // Secret by key name (UPPER_SNAKE token or lowercase field name).
        assert!(is_secret("DATABASE_URL", "anything"));
        assert!(is_secret("MY_API_KEY", "x"));
        assert!(is_secret("password", "x"));
        // Non-secret keys (incl. allowlisted look-alikes) with plain values.
        assert!(!is_secret("RAILS_ENV", "production"));
        assert!(!is_secret("PORT", "3000"));
        assert!(!is_secret("LOG_LEVEL", "debug"));
        // Non-secret key name but a credential-shaped VALUE — caught by the
        // value layer so human `env get` agrees with the JSON walker (#1152).
        assert!(is_secret("MIGRATION_NOTE", "postgres://u:p@h/db"));
        assert!(is_secret("NOTE", "floo_aaaaaaaaaaaaaaaaaaaaaaaa"));
        // Empty value is never a secret — mirrors the JSON walker's empty-string
        // skip, so human `env get` and `--json` agree on an empty-valued key.
        assert!(!is_secret("DATABASE_URL", ""));
        assert!(!is_secret("password", ""));
    }

    #[test]
    fn reveal_mode_keeps_values_but_still_signals() {
        let (out, found) = reveal(json!({"password": "hunter2"}));
        assert!(found, "detection still fires under reveal");
        assert_eq!(
            out["password"], "hunter2",
            "reveal keeps the original value"
        );
    }

    #[test]
    fn empty_strings_not_flagged() {
        let (_, found) = redact(json!({"password": ""}));
        assert!(!found, "empty strings carry no secret to redact");
    }

    #[test]
    fn short_strings_not_flagged_by_value_pattern() {
        // A `note` field with a short value like "ok" should not trip
        // any value-shape pattern.
        let (_, found) = redact(json!({"note": "ok"}));
        assert!(!found);
    }

    #[test]
    fn deeply_nested_secret_redacted() {
        let (out, found) = redact(json!({
            "data": {"deploy": {"audit": {"env": {"DATABASE_URL": "postgres://u:p@h/d"}}}}
        }));
        assert!(found);
        assert_eq!(
            out["data"]["deploy"]["audit"]["env"]["DATABASE_URL"],
            REDACTED_PLACEHOLDER
        );
    }
}

/// Snapshot regression tests for `--json` output across every CLI
/// command surface that has ever leaked or could leak secrets.
///
/// Strategy: build a JSON payload that matches each command's `--json`
/// envelope shape, including realistic credential-shaped values
/// (database URLs with embedded passwords, JWTs, floo API keys). Drive
/// the payload through the same redaction step `output::print_json`
/// runs, then assert that no plaintext credential survives unless
/// `--reveal-secrets` was set. Each `FORBIDDEN_SUBSTRINGS` entry is
/// embedded in at least one payload so a regression that skips a
/// branch in the redactor fails fast.
///
/// These live alongside the unit tests instead of in `tests/` because
/// `floo` is a binary-only crate (no library target), so integration
/// tests can't access `crate::redact` directly.
#[cfg(test)]
mod snapshots {
    use super::*;
    use serde_json::{json, Value};
    use std::sync::Mutex;

    /// Serializes tests that toggle the global `REVEAL_SECRETS` atomic
    /// against tests that assume the default-redact posture. Without
    /// this, parallel test execution lets a reveal-mode test set the
    /// atomic, a redact-mode test snapshot it, and assertions race.
    static REVEAL_LOCK: Mutex<()> = Mutex::new(());

    /// Fixed strings that must never survive redaction. Each
    /// command-shaped payload below embeds at least one.
    const FORBIDDEN_SUBSTRINGS: &[&str] = &[
        "hunter2",
        "supersecretvaluethatshouldnotleak",
        "postgres-backed-pass-bd7fb",
        "redis-token-9e13-rotate-me",
        "secret_key_base_xyzzy_9183",
        "floo_aaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "AKIAIOSFODNN7EXAMPLE",
    ];

    fn collect_leaks(value: &Value) -> Vec<String> {
        fn walk(v: &Value, hits: &mut Vec<String>) {
            match v {
                Value::String(s) => {
                    for forbidden in FORBIDDEN_SUBSTRINGS {
                        if s.contains(forbidden) {
                            hits.push(format!("{forbidden:?} in {s:?}"));
                        }
                    }
                }
                Value::Array(arr) => arr.iter().for_each(|v| walk(v, hits)),
                Value::Object(map) => map.values().for_each(|v| walk(v, hits)),
                _ => {}
            }
        }
        let mut hits = Vec::new();
        walk(value, &mut hits);
        hits
    }

    /// Mirror of `output::print_json`'s redaction step: redact then
    /// stamp the top-level `contains_secrets` marker.
    fn through_print_json(mut value: Value) -> Value {
        let contains_secrets = process_in_place(&mut value);
        if contains_secrets {
            if let Value::Object(map) = &mut value {
                map.entry(CONTAINS_SECRETS_KEY.to_string())
                    .or_insert(Value::Bool(true));
            }
        }
        value
    }

    /// Take the reveal-state lock and reset to the default-redact
    /// posture. Returned guard MUST be held for the duration of any
    /// test that depends on the global flag's state — drop it only
    /// after the last assertion.
    fn lock_default_redact() -> std::sync::MutexGuard<'static, ()> {
        let guard = REVEAL_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        set_reveal_secrets(false);
        guard
    }

    fn lock_reveal() -> std::sync::MutexGuard<'static, ()> {
        let guard = REVEAL_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        set_reveal_secrets(true);
        guard
    }

    /// `floo dev --json` start payload — the highest-impact leak in
    /// the bug report. Streams managed-service env vars per service.
    #[test]
    fn floo_dev_start_payload_redacted() {
        let _g = lock_default_redact();
        let payload = json!({
            "success": true,
            "data": {
                "session_id": "session-abc",
                "app": "floo-artifact",
                "postgres_authorized": true,
                "services": [
                    {
                        "name": "web",
                        "port": 3000,
                        "url": "http://localhost:3000",
                        "auth_proxied_url": null,
                        "env_vars": {
                            "DATABASE_URL": "postgres://app:postgres-backed-pass-bd7fb@127.0.0.1:5432/app",
                            "REDIS_URL": "redis://default:redis-token-9e13-rotate-me@127.0.0.1:6379/0",
                            "SECRET_KEY_BASE": "secret_key_base_xyzzy_9183",
                            "PORT": "3000",
                            "RAILS_ENV": "development",
                        }
                    }
                ]
            }
        });
        let out = through_print_json(payload);
        let leaks = collect_leaks(&out);
        assert!(leaks.is_empty(), "leaks survived redaction: {leaks:?}");
        assert_eq!(out["contains_secrets"], true);
        let env = &out["data"]["services"][0]["env_vars"];
        assert_eq!(env["PORT"], "3000", "non-secret env var must remain");
        assert_eq!(
            env["RAILS_ENV"], "development",
            "non-secret env var must remain"
        );
    }

    /// `floo dev --json` log streaming. App boot logs frequently echo
    /// their own DATABASE_URL — defense in depth via value-shape regex.
    #[test]
    fn floo_dev_log_event_redacts_uri_credentials() {
        let _g = lock_default_redact();
        let payload = json!({
            "event": "log",
            "service": "web",
            "stream": "stdout",
            "line": "Connected to postgres://u:postgres-backed-pass-bd7fb@127.0.0.1:5432/app"
        });
        let out = through_print_json(payload);
        assert!(
            collect_leaks(&out).is_empty(),
            "URI userinfo leaked through log event"
        );
        assert_eq!(out["contains_secrets"], true);
    }

    /// `floo env get KEY --json` — was returning `{key, value}`
    /// plaintext.
    #[test]
    fn floo_env_get_secret_key_redacted() {
        let _g = lock_default_redact();
        let payload = json!({
            "success": true,
            "data": {
                "key": "DATABASE_URL",
                "value": "postgres://u:postgres-backed-pass-bd7fb@h/db"
            }
        });
        let out = through_print_json(payload);
        assert!(collect_leaks(&out).is_empty());
        assert_eq!(out["contains_secrets"], true);
        assert_eq!(out["data"]["key"], "DATABASE_URL");
        assert_ne!(
            out["data"]["value"],
            "postgres://u:postgres-backed-pass-bd7fb@h/db"
        );
    }

    #[test]
    fn floo_env_get_non_secret_key_kept() {
        let _g = lock_default_redact();
        let payload = json!({
            "success": true,
            "data": {"key": "RAILS_ENV", "value": "production"}
        });
        let out = through_print_json(payload);
        assert_eq!(out["data"]["value"], "production");
        assert!(out.get("contains_secrets").is_none());
    }

    /// `floo env list --json` — the API returns `masked_value` already masked
    /// to the fixed `********` marker (#1152). The marker must pass through
    /// untouched (it carries no secret); nothing should be flagged here.
    #[test]
    fn floo_env_list_masked_values_kept() {
        let _g = lock_default_redact();
        let payload = json!({
            "success": true,
            "data": {
                "env_vars": [
                    {"key": "DATABASE_URL", "masked_value": "********"},
                    {"key": "API_TOKEN",    "masked_value": "********"},
                ]
            }
        });
        let out = through_print_json(payload);
        assert!(collect_leaks(&out).is_empty());
        let arr = out["data"]["env_vars"].as_array().unwrap();
        assert_eq!(arr[0]["masked_value"], "********");
        assert_eq!(arr[1]["masked_value"], "********");
    }

    /// `floo deploys watch --json` — historical leak of Cloud Run
    /// audit payloads carrying full env values (feedback 35437986).
    #[test]
    fn floo_deploys_watch_audit_payload_redacted() {
        let _g = lock_default_redact();
        let payload = json!({
            "event": "done",
            "status": "failed",
            "url": "",
            "deploy": {
                "id": "d2b3a9be-dd53-4c8b-a2de-46beb60335fd",
                "status": "failed",
                "audit": {
                    "container": {
                        "env": [
                            {"key": "DATABASE_URL", "value": "postgres://u:postgres-backed-pass-bd7fb@h/d"},
                            {"key": "SECRET_KEY_BASE", "value": "secret_key_base_xyzzy_9183"},
                            {"key": "PORT", "value": "8080"}
                        ]
                    }
                }
            }
        });
        let out = through_print_json(payload);
        let leaks = collect_leaks(&out);
        assert!(leaks.is_empty(), "audit payload leaked secrets: {leaks:?}");
        assert_eq!(out["contains_secrets"], true);
    }

    /// `floo deploys logs --json` — `Deploy.generated_password` was
    /// being serialized verbatim.
    #[test]
    fn floo_deploys_logs_generated_password_redacted() {
        let _g = lock_default_redact();
        let payload = json!({
            "success": true,
            "data": {
                "deploy_id": "abc",
                "status": "live",
                "build_logs": "Step 1: ...",
                "deploy": {
                    "id": "abc",
                    "status": "live",
                    "generated_password": "hunter2"
                }
            }
        });
        let out = through_print_json(payload);
        assert!(collect_leaks(&out).is_empty());
        assert_eq!(out["contains_secrets"], true);
    }

    /// `floo apps password --json` is explicitly designed to reveal a
    /// secret. Default redaction must hold.
    #[test]
    fn floo_apps_password_default_redacts() {
        let _g = lock_default_redact();
        let payload = json!({"success": true, "data": {"password": "hunter2"}});
        let out = through_print_json(payload);
        assert!(collect_leaks(&out).is_empty());
        assert_eq!(out["contains_secrets"], true);
    }

    /// `floo apps password --json --reveal-secrets` lets it through —
    /// but the marker still fires so harnesses can refuse the payload.
    #[test]
    fn floo_apps_password_reveal_emits_plaintext_with_marker() {
        let _g = lock_reveal();
        let payload = json!({"success": true, "data": {"password": "hunter2"}});
        let out = through_print_json(payload);
        // Reset before dropping the lock so a subsequent default-redact
        // test sees the canonical posture.
        set_reveal_secrets(false);
        assert_eq!(
            out["data"]["password"], "hunter2",
            "reveal must keep plaintext"
        );
        assert_eq!(
            out["contains_secrets"], true,
            "marker fires even under reveal so harnesses can refuse"
        );
    }

    /// `floo logs --json` — defense in depth even though the API
    /// already redacts upstream.
    #[test]
    fn floo_logs_runtime_message_redacted_on_uri_leak() {
        let _g = lock_default_redact();
        let payload = json!({
            "success": true,
            "data": {
                "logs": [
                    {
                        "timestamp": "2026-04-30T16:14:43Z",
                        "severity": "INFO",
                        "service_name": "web",
                        "message": "boot: connecting to postgres://u:postgres-backed-pass-bd7fb@h/db"
                    }
                ]
            }
        });
        let out = through_print_json(payload);
        let leaks = collect_leaks(&out);
        assert!(
            leaks.is_empty(),
            "runtime log credentials leaked: {leaks:?}"
        );
    }

    /// `floo auth token` — stored API key.
    #[test]
    fn floo_auth_token_api_key_redacted_by_default() {
        let _g = lock_default_redact();
        let payload = json!({
            "success": true,
            "data": {
                "api_key": "floo_aaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "email": "team@getfloo.com"
            }
        });
        let out = through_print_json(payload);
        let leaks = collect_leaks(&out);
        assert!(leaks.is_empty(), "API key leaked: {leaks:?}");
        assert_eq!(out["contains_secrets"], true);
        assert_eq!(out["data"]["email"], "team@getfloo.com", "non-secret kept");
    }

    /// `floo db query --json` — a query result row that happens to
    /// include a credential URI must not pass through.
    #[test]
    fn floo_db_query_result_row_credential_caught() {
        let _g = lock_default_redact();
        let payload = json!({
            "success": true,
            "data": {
                "rows": [
                    {"id": 1, "msg": "ok"},
                    {"id": 2, "msg": "old: postgres://u:postgres-backed-pass-bd7fb@h/db"},
                ],
                "row_count": 2
            }
        });
        let out = through_print_json(payload);
        assert!(collect_leaks(&out).is_empty(), "query result leaked URI");
    }

    /// AWS access key inside an arbitrary command response.
    #[test]
    fn aws_access_key_in_arbitrary_response_caught() {
        let _g = lock_default_redact();
        let payload = json!({
            "success": true,
            "data": {
                "warning": "stale credential AKIAIOSFODNN7EXAMPLE detected — please rotate"
            }
        });
        let out = through_print_json(payload);
        assert!(
            collect_leaks(&out).is_empty(),
            "AWS key leaked through warning"
        );
    }

    /// Sweep test: kitchen-sink payload combining every leak vector.
    /// If any future change skips a branch in the redactor, this
    /// fails before it reaches users.
    #[test]
    fn kitchen_sink_no_forbidden_substring_survives() {
        let _g = lock_default_redact();
        let payload = json!({
            "success": true,
            "data": {
                "session_id": "s",
                "services": [{
                    "name": "web",
                    "env_vars": {
                        "DATABASE_URL": "postgres://u:postgres-backed-pass-bd7fb@h/d",
                        "REDIS_URL": "redis://default:redis-token-9e13-rotate-me@h/0",
                        "SECRET_KEY_BASE": "secret_key_base_xyzzy_9183",
                        "PORT": "3000",
                    }
                }],
                "deploy": {
                    "id": "abc",
                    "generated_password": "hunter2",
                    "audit": {
                        "container": {
                            "env": [
                                {"key": "API_TOKEN", "value": "supersecretvaluethatshouldnotleak"}
                            ]
                        }
                    }
                },
                "auth": {"api_key": "floo_aaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
                "warning": "rotate AKIAIOSFODNN7EXAMPLE",
                "env_var": {"key": "DATABASE_URL", "value": "postgres://u:postgres-backed-pass-bd7fb@h/d"},
            }
        });
        let out = through_print_json(payload);
        let leaks = collect_leaks(&out);
        assert!(leaks.is_empty(), "kitchen-sink leaked: {leaks:?}");
        let serialized = serde_json::to_string(&out).unwrap();
        for forbidden in FORBIDDEN_SUBSTRINGS {
            assert!(
                !serialized.contains(forbidden),
                "serialized JSON still contains {forbidden:?}: {serialized}"
            );
        }
        assert_eq!(out["contains_secrets"], true);
    }
}
