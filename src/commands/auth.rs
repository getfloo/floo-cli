use std::process;

use dialoguer::{Input, Password};

use crate::api_client::FlooClient;
use crate::config::{clear_config, load_config, save_config};
use crate::output;

pub fn login(email: Option<String>, password: Option<String>) {
    let email = email.unwrap_or_else(|| {
        Input::new()
            .with_prompt("Email")
            .interact_text()
            .unwrap_or_else(|_| process::exit(1))
    });

    let password = password.unwrap_or_else(|| {
        Password::new()
            .with_prompt("Password")
            .interact()
            .unwrap_or_else(|_| process::exit(1))
    });

    let spinner = output::Spinner::new("Logging in...");
    let client = FlooClient::new(None);
    match client.login(&email, &password) {
        Ok(result) => {
            spinner.finish();
            let mut config = load_config();
            config.api_key = result
                .get("api_key")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            config.user_email = result
                .get("email")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            if let Err(e) = save_config(&config) {
                output::error(
                    &format!("Failed to save credentials: {e}"),
                    "CONFIG_ERROR",
                    None,
                );
                process::exit(1);
            }
            let display_email = config.user_email.as_deref().unwrap_or(&email);
            output::success(
                &format!("Logged in as {display_email}"),
                Some(serde_json::json!({"email": display_email})),
            );
        }
        Err(e) => {
            spinner.finish();
            output::error(&e.message, &e.code, Some("Check your email and password."));
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
                Some("Run 'floo login' to authenticate."),
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
