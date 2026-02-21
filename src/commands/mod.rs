use std::process;

use crate::api_client::FlooClient;
use crate::config::FlooConfig;
use crate::output;

pub mod apps;
pub mod auth;
pub mod db;
pub mod deploy;
pub mod domains;
pub mod env;
pub mod logs;
pub mod releases;
pub mod rollbacks;
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
