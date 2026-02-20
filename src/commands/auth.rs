use std::process;
use std::thread;
use std::time::{Duration, Instant};

use colored::Colorize;
use dialoguer::Password;

use crate::config::{clear_config, load_config, save_config};
use crate::output;

pub fn login() {
    let client = super::init_client(None);

    // Step 1: Initiate device code flow
    let spinner = output::Spinner::new("Requesting device code...");
    let auth = match client.device_authorize() {
        Ok(result) => {
            spinner.finish();
            result
        }
        Err(e) => {
            spinner.finish();
            output::error(&e.message, &e.code, Some("Check your network connection."));
            process::exit(1);
        }
    };

    let user_code = auth
        .get("user_code")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            output::error("Invalid response: missing user_code", "PARSE_ERROR", None);
            process::exit(1);
        });
    let verification_uri_complete = auth
        .get("verification_uri_complete")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            output::error(
                "Invalid response: missing verification_uri_complete",
                "PARSE_ERROR",
                None,
            );
            process::exit(1);
        });
    let device_code = auth
        .get("device_code")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            output::error("Invalid response: missing device_code", "PARSE_ERROR", None);
            process::exit(1);
        });
    let interval = auth
        .get("interval")
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| {
            output::error("Invalid response: missing interval", "PARSE_ERROR", None);
            process::exit(1);
        });
    let expires_in = auth
        .get("expires_in")
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| {
            output::error("Invalid response: missing expires_in", "PARSE_ERROR", None);
            process::exit(1);
        });

    // Step 2: Display code and open browser
    if !output::is_json_mode() {
        eprintln!();
        eprintln!("  Your one-time code is:  {}", user_code.bold());
        eprintln!();
        eprintln!("  Opening browser to: {}", verification_uri_complete);
        eprintln!("  If the browser didn't open, visit the URL above and enter the code.");
        eprintln!();
    }

    // Open browser (non-fatal if it fails)
    let _ = open::that(verification_uri_complete);

    // Step 3: Poll for completion
    let spinner = output::Spinner::new("Waiting for browser authentication...");
    let mut network_retries = 0u8;
    let deadline = Instant::now() + Duration::from_secs(expires_in);
    let mut poll_interval = interval;

    loop {
        thread::sleep(Duration::from_secs(poll_interval));

        if Instant::now() > deadline {
            spinner.finish();
            output::error(
                "Device code expired. Please try again.",
                "DEVICE_CODE_EXPIRED",
                Some("Run 'floo auth login' to start a new session."),
            );
            process::exit(1);
        }

        match client.device_token(device_code) {
            Ok(result) => {
                spinner.finish();
                let api_key = result
                    .get("api_key")
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| {
                        output::error(
                            "Server returned success but API key was missing.",
                            "PARSE_ERROR",
                            Some("This may be a server bug. Please try again."),
                        );
                        process::exit(1);
                    });
                let email = result
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| {
                        output::error(
                            "Server returned success but email was missing.",
                            "PARSE_ERROR",
                            Some("This may be a server bug. Please try again."),
                        );
                        process::exit(1);
                    });
                let mut config = load_config();
                config.api_key = Some(api_key.to_string());
                config.user_email = Some(email.to_string());
                if let Err(e) = save_config(&config) {
                    output::error(
                        &format!("Failed to save credentials: {e}"),
                        "CONFIG_ERROR",
                        None,
                    );
                    process::exit(1);
                }
                output::success(
                    &format!("Logged in as {email}"),
                    Some(serde_json::json!({"email": email})),
                );
                return;
            }
            Err(e) if e.status_code == 202 => {
                // Still pending — continue polling
                network_retries = 0;
                if e.code == "DEVICE_SLOW_DOWN" {
                    // RFC 8628: increase interval by 5 seconds on slow_down
                    poll_interval = interval + 5;
                }
                continue;
            }
            Err(e) if e.code == "DEVICE_CODE_EXPIRED" => {
                spinner.finish();
                output::error(
                    "Device code expired. Please try again.",
                    "DEVICE_CODE_EXPIRED",
                    Some("Run 'floo auth login' to start a new session."),
                );
                process::exit(1);
            }
            Err(e) if e.code == "DEVICE_AUTH_DENIED" => {
                spinner.finish();
                output::error("Authorization was denied.", "DEVICE_AUTH_DENIED", None);
                process::exit(1);
            }
            Err(e) if e.status_code == 0 => {
                // Network error — retry up to 3 times
                network_retries += 1;
                if network_retries >= 3 {
                    spinner.finish();
                    output::error(&e.message, &e.code, Some("Check your network connection."));
                    process::exit(1);
                }
                continue;
            }
            Err(e) => {
                spinner.finish();
                output::error(&e.message, &e.code, None);
                process::exit(1);
            }
        }
    }
}

pub fn register(email: &str) {
    let password = Password::new()
        .with_prompt("Password")
        .with_confirmation("Confirm password", "Passwords do not match.")
        .interact()
        .unwrap_or_else(|_| process::exit(1));

    let spinner = output::Spinner::new("Creating account...");
    let client = super::init_client(None);
    match client.register(email, &password) {
        Ok(result) => {
            spinner.finish();
            let api_key = result
                .get("api_key")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| {
                    output::error(
                        "Server returned success but API key was missing.",
                        "PARSE_ERROR",
                        Some("This may be a server bug. Please try again."),
                    );
                    process::exit(1);
                });
            let resp_email = result
                .get("email")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| {
                    output::error(
                        "Server returned success but email was missing.",
                        "PARSE_ERROR",
                        Some("This may be a server bug. Please try again."),
                    );
                    process::exit(1);
                });
            let mut config = load_config();
            config.api_key = Some(api_key.to_string());
            config.user_email = Some(resp_email.to_string());
            if let Err(e) = save_config(&config) {
                output::error(
                    &format!("Failed to save credentials: {e}"),
                    "CONFIG_ERROR",
                    None,
                );
                process::exit(1);
            }
            output::success(
                &format!("Account created! Logged in as {resp_email}"),
                Some(serde_json::json!({"email": resp_email})),
            );
        }
        Err(e) if e.code == "EMAIL_TAKEN" => {
            spinner.finish();
            output::error(
                "This email is already registered.",
                "EMAIL_TAKEN",
                Some("Use 'floo auth login' to sign in."),
            );
            process::exit(1);
        }
        Err(e) => {
            spinner.finish();
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    }
}

pub fn logout() {
    clear_config();
    output::success("Logged out.", None);
}

pub fn whoami() {
    let config = load_config();
    match &config.api_key {
        None => {
            output::error(
                "Not logged in.",
                "NOT_AUTHENTICATED",
                Some("Run 'floo auth login' to authenticate."),
            );
            process::exit(1);
        }
        Some(key) => {
            let masked = if key.len() > 13 {
                format!("{}...{}", &key[..9], &key[key.len() - 4..])
            } else {
                key.clone()
            };
            let data = serde_json::json!({
                "email": config.user_email,
                "api_key": masked,
            });
            output::success(
                &format!(
                    "Logged in as {} (key: {masked})",
                    config.user_email.as_deref().unwrap_or("unknown")
                ),
                Some(data),
            );
        }
    }
}
