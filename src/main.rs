mod api_client;
mod api_types;
mod cli;
mod commands;
mod config;
mod confirm;
mod constants;
mod deploy_status;
mod detection;
mod dev_proxy;
mod dockerfile;
mod errors;
mod names;
mod output;
mod postgres_ready;
mod project_config;
mod redact;
mod resolve;
mod services_lock;
mod updater;
mod version_check;

fn main() {
    cli::run();
}
