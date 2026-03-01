use std::process;
use std::thread;
use std::time::{Duration, Instant};

use colored::Colorize;

use crate::config::{clear_config, load_config, save_config};
use crate::output;

pub fn login(api_key: Option<&str>, force: bool) {
    // Path 1: --api-key flag — save directly and validate
    if let Some(key) = api_key {
        let mut config = load_config();
        config.api_key = Some(key.to_string());
        if let Err(e) = save_config(&config) {
            output::error(
                &format!("Failed to save credentials: {e}"),
                "CONFIG_ERROR",
                None,
            );
            process::exit(1);
        }

        let client = super::init_client(Some(config));
        match client.whoami() {
            Ok(result) => {
                let email = &result.email;
                // Save the email too
                let mut config = load_config();
                config.user_email = Some(email.to_string());
                let _ = save_config(&config);
                output::success(
                    &format!("Logged in as {email}"),
                    Some(serde_json::json!({"email": email})),
                );
            }
            Err(e) => {
                // Key is invalid — clear it
                clear_config();
                output::error(&e.message, &e.code, Some("The API key is invalid."));
                process::exit(1);
            }
        }
        return;
    }

    // Path 2: Pre-check existing key (unless --force)
    if !force {
        let config = load_config();
        if config.api_key.is_some() {
            let client = super::init_client(Some(config));
            match client.whoami() {
                Ok(result) => {
                    let email = &result.email;
                    output::success(
                        &format!("Already logged in as {email}"),
                        Some(serde_json::json!({"email": email, "already_authenticated": true})),
                    );
                    return;
                }
                Err(_) => {
                    // Key is invalid — proceed to device code flow
                }
            }
        }
    }

    // Path 3: Device code flow
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

    let user_code = &auth.user_code;
    let verification_uri_complete = &auth.verification_uri_complete;
    let device_code = &auth.device_code;
    let interval = auth.interval;
    let expires_in = auth.expires_in;

    // Step 2: Display code and open browser
    if !output::is_json_mode() {
        eprintln!();
        eprintln!("  Your one-time code is:  {}", user_code.bold());
        eprintln!();
        eprintln!("  Opening browser to: {verification_uri_complete}");
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
                let api_key = &result.api_key;
                let email = &result.email;
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
                if !output::is_json_mode() {
                    eprintln!();
                    eprintln!(
                        "  Tip: Run 'floo skills install --path <dir>' to set up agent integration."
                    );
                }
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
            Err(e) if e.code == "SIGNUP_DISABLED" => {
                spinner.finish();
                output::error(
                    &e.message,
                    "SIGNUP_DISABLED",
                    Some("Join the waitlist at https://getfloo.com to request access."),
                );
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

pub fn token() {
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
            if output::is_json_mode() {
                output::success(
                    "API key retrieved",
                    Some(serde_json::json!({"api_key": key})),
                );
            } else {
                // Print raw key to stdout for piping
                println!("{key}");
            }
        }
    }
}

pub fn register(email: &str) {
    let spinner = output::Spinner::new("Creating account...");
    let client = super::init_client(None);
    match client.register(email) {
        Ok(result) => {
            spinner.finish();
            let api_key = &result.api_key;
            let resp_email = &result.email;
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

pub fn update_profile(name: &str) {
    super::require_auth();
    let client = super::init_client(None);

    match client.update_profile(name) {
        Ok(result) => {
            let updated_name = &result.name;
            output::success(
                &format!("Profile updated. Name: {updated_name}"),
                Some(output::to_value(&result)),
            );
        }
        Err(e) => {
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

            // Fetch live profile data from the API
            let client = super::init_client(None);
            match client.whoami() {
                Ok(result) => {
                    let email = &result.email;
                    let name = result.name.as_deref();

                    let mut data = serde_json::json!({
                        "email": email,
                        "api_key": masked,
                    });
                    if let Some(n) = name {
                        data.as_object_mut()
                            .unwrap()
                            .insert("name".to_string(), serde_json::Value::String(n.to_string()));
                    }

                    let display = if let Some(n) = name {
                        format!("{n} ({email}, key: {masked})")
                    } else {
                        format!("{email} (key: {masked})")
                    };
                    output::success(&format!("Logged in as {display}"), Some(data));
                }
                Err(e) => {
                    output::error(
                        &e.message,
                        &e.code,
                        Some("Your API key may be invalid. Try 'floo auth login'."),
                    );
                    process::exit(1);
                }
            }
        }
    }
}
