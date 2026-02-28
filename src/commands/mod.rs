use std::process;

use crate::api_client::FlooClient;
use crate::config::{load_config, FlooConfig};
use crate::output;

pub mod analytics;
pub mod apps;
pub mod auth;
pub mod deploy;
pub mod domains;
pub mod env;
pub mod logs;
pub mod releases;
pub mod rollbacks;
pub mod services;
pub mod update;

pub(crate) fn init_client(config: Option<FlooConfig>) -> FlooClient {
    match FlooClient::new(config) {
        Ok(client) => client,
        Err(error) => {
            output::error(
                &error.message,
                &error.code,
                Some("Check your network/TLS setup and try again."),
            );
            process::exit(1);
        }
    }
}

pub(crate) fn require_auth() {
    let config = load_config();
    if config.api_key.is_none() {
        output::error(
            "Not logged in.",
            "NOT_AUTHENTICATED",
            Some("Run 'floo login' to authenticate."),
        );
        process::exit(1);
    }
}

pub(crate) fn expect_str_field<'a>(data: &'a serde_json::Value, field: &str) -> &'a str {
    data.get(field).and_then(|v| v.as_str()).unwrap_or_else(|| {
        output::error(
            &format!("Response missing '{field}' field."),
            "PARSE_ERROR",
            Some("This is a bug. Please report it."),
        );
        process::exit(1);
    })
}
