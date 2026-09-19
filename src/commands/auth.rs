use std::process;
use std::thread;
use std::time::{Duration, Instant};

use colored::Colorize;

use crate::api_client::FlooClient;
use crate::api_types::WhoamiResponse;
use crate::config::{clear_config, load_config, save_config};
use crate::confirm::{confirm_tier2, ConfirmOutcome};
use crate::errors::{ErrorCode, FlooApiError};
use crate::output;

const TERMS_URL: &str = "https://getfloo.com/legal/terms";
const PRIVACY_URL: &str = "https://getfloo.com/legal/privacy";
const ACCEPT_TERMS_HELP: &str = "Run 'floo auth accept-terms' or, after the human agrees to both policies, 'floo auth accept-terms --yes'.";

fn terms_data(profile: &WhoamiResponse) -> serde_json::Value {
    serde_json::json!({
        "current_terms_version": profile.current_terms_version,
        "terms_accepted_version": profile.terms_accepted_version,
        "acceptance_required": profile.needs_terms_acceptance(),
        "terms_url": TERMS_URL,
        "privacy_url": PRIVACY_URL,
    })
}

/// Whether agreement was explicitly supplied or must be requested interactively.
pub enum TermsConsent {
    Prompt,
    Agreed,
}

fn request_terms_acceptance(
    client: &FlooClient,
    profile: WhoamiResponse,
    consent: TermsConsent,
) -> Result<WhoamiResponse, FlooApiError> {
    output::dim_line(&format!("Terms of Service: {TERMS_URL}"));
    output::dim_line(&format!("Privacy Policy: {PRIVACY_URL}"));
    output::dim_line(&format!(
        "Current terms version: {}",
        profile.current_terms_version
    ));
    if !profile.needs_terms_acceptance() {
        return Ok(profile);
    }
    match confirm_tier2(
        "Do you agree to the floo",
        "Terms of Service and Privacy Policy",
        matches!(consent, TermsConsent::Agreed),
    ) {
        ConfirmOutcome::Proceed => client.accept_terms(&profile.current_terms_version),
        ConfirmOutcome::Aborted | ConfirmOutcome::Refused { .. } => Err(FlooApiError::new(
            0,
            "CONFIRMATION_REQUIRED",
            "The account cannot be used until you accept the floo Terms of Service and Privacy Policy. Non-interactive acceptance requires --yes.",
        )),
    }
}

/// Record agreement only after --yes or interactive confirmation; never cache it.
pub fn accept_terms(consent: TermsConsent) {
    super::require_auth();
    let client = super::init_client(None);
    let profile = match client.whoami() {
        Ok(profile) => profile,
        Err(error) => {
            output::error(&error.message, &ErrorCode::from_api(&error.code), None);
            process::exit(1);
        }
    };
    let data = terms_data(&profile);
    let message = if profile.current_terms_version.is_empty() {
        "No terms acceptance is required by this API."
    } else if !profile.needs_terms_acceptance() {
        "Already accepted the current floo Terms of Service and Privacy Policy."
    } else {
        "Accepted the floo Terms of Service and Privacy Policy."
    };
    match request_terms_acceptance(&client, profile, consent) {
        Ok(profile) => output::success(message, Some(terms_data(&profile))),
        Err(error) => {
            output::error_with_data(
                &error.message,
                &ErrorCode::from_api(&error.code),
                Some(ACCEPT_TERMS_HELP),
                Some(data),
            );
            process::exit(1);
        }
    }
}

// Terms failures cannot undo a successful login or emit a second JSON envelope.
fn complete_login(
    client: &FlooClient,
    profile: Result<WhoamiResponse, FlooApiError>,
    message: &str,
    mut data: serde_json::Value,
) {
    let acceptance = profile.and_then(|profile| {
        if !profile.needs_terms_acceptance() {
            return Ok(profile);
        }
        data["terms"] = terms_data(&profile);
        request_terms_acceptance(client, profile, TermsConsent::Prompt)
    });
    match acceptance {
        Ok(profile) => data["terms"] = terms_data(&profile),
        Err(error) => {
            output::warn(&format!("{} {ACCEPT_TERMS_HELP}", error.message));
            data["terms_acceptance_error"] = serde_json::json!({
                "code": error.code, "message": error.message, "suggestion": ACCEPT_TERMS_HELP,
            });
        }
    }
    output::success(message, Some(data));
}

pub fn login(api_key: Option<&str>, force: bool) {
    // Path 1: --api-key flag — save directly and validate
    if let Some(key) = api_key {
        let mut config = load_config();
        config.api_key = Some(key.to_string());
        if let Err(e) = save_config(&config) {
            output::error(
                &format!("Failed to save credentials: {e}"),
                &ErrorCode::ConfigError,
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
                complete_login(
                    &client,
                    Ok(result.clone()),
                    &format!("Logged in as {email}"),
                    serde_json::json!({"email": email}),
                );
            }
            Err(e) => {
                // Key is invalid — clear it
                clear_config();
                output::error(
                    &e.message,
                    &ErrorCode::from_api(&e.code),
                    Some("The API key is invalid."),
                );
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
                    complete_login(
                        &client,
                        Ok(result.clone()),
                        &format!("Already logged in as {email}"),
                        serde_json::json!({"email": email, "already_authenticated": true}),
                    );
                    return;
                }
                Err(_) => {
                    // Key is invalid — proceed to device code flow
                }
            }
        }
    }

    // Path 3 is structurally impossible on the dev stack.
    // api.dev.getfloo.com has no WorkOS configured, so
    // POST /v1/auth/device returns WORKOS_NOT_CONFIGURED. Dev auth is
    // API-key-only by design — the dev-stack seed injects a dev admin
    // key, surfaced as the FLOO_SMOKE_KEY_DEV GitHub Actions secret.
    // Without this guard `floo-dev auth login` runs the doomed device
    // flow and dies mid-spinner with a misleading "check your network"
    // suggestion (the symptom the operator reported as "nothing
    // happening"). Fail fast with the one command that actually works.
    if crate::config::is_dev_binary() {
        output::error(
            "The dev stack is API-key-only — api.dev.getfloo.com has no browser/WorkOS login.",
            &ErrorCode::NotAuthenticated,
            Some(
                "Run: floo-dev auth login --api-key <FLOO_SMOKE_KEY_DEV> \
                 (the dev admin key in the floo repo's GitHub Actions secrets).",
            ),
        );
        process::exit(1);
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
            output::error(
                &e.message,
                &ErrorCode::from_api(&e.code),
                Some("Check your network connection."),
            );
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
                &ErrorCode::DeviceCodeExpired,
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
                        &ErrorCode::ConfigError,
                        None,
                    );
                    process::exit(1);
                }
                let authenticated_client = super::init_client(Some(config));
                complete_login(
                    &authenticated_client,
                    authenticated_client.whoami(),
                    &format!("Logged in as {email}"),
                    serde_json::json!({"email": email}),
                );
                if !output::is_json_mode() {
                    eprintln!();
                    eprintln!("  Tip: Run 'floo docs' to see how floo works, or 'floo --help' to explore commands.");
                    eprintln!("  Tip: Run 'floo skills install --path <dir>' to set up agent integration.");
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
                    &ErrorCode::DeviceCodeExpired,
                    Some("Run 'floo auth login' to start a new session."),
                );
                process::exit(1);
            }
            Err(e) if e.code == "DEVICE_AUTH_DENIED" => {
                spinner.finish();
                output::error(
                    "Authorization was denied.",
                    &ErrorCode::DeviceAuthDenied,
                    None,
                );
                process::exit(1);
            }
            Err(e) if e.code == "SIGNUP_DISABLED" => {
                spinner.finish();
                output::error(
                    &e.message,
                    &ErrorCode::SignupDisabled,
                    Some("Join the waitlist at https://getfloo.com to request access."),
                );
                process::exit(1);
            }
            Err(e) if e.code == "WAITLISTED" => {
                spinner.finish();
                output::error(
                    "You're on the waitlist! We'll email you when your account is ready.",
                    &ErrorCode::Waitlisted,
                    None,
                );
                process::exit(1);
            }
            Err(e) if e.status_code == 0 => {
                // Network error — retry up to 3 times
                network_retries += 1;
                if network_retries >= 3 {
                    spinner.finish();
                    output::error(
                        &e.message,
                        &ErrorCode::from_api(&e.code),
                        Some("Check your network connection."),
                    );
                    process::exit(1);
                }
                continue;
            }
            Err(e) => {
                spinner.finish();
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
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
                &ErrorCode::NotAuthenticated,
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
                    &ErrorCode::ConfigError,
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
                &ErrorCode::EmailTaken,
                Some("Use 'floo auth login' to sign in."),
            );
            process::exit(1);
        }
        Err(e) => {
            spinner.finish();
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
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
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
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
                &ErrorCode::NotAuthenticated,
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
                        &ErrorCode::from_api(&e.code),
                        Some("Your API key may be invalid. Try 'floo auth login'."),
                    );
                    process::exit(1);
                }
            }
        }
    }
}
