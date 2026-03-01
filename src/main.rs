mod api_client;
mod api_types;
mod archive;
mod cli;
mod commands;
mod config;
mod constants;
mod detection;
mod errors;
mod names;
mod output;
mod project_config;
mod resolve;
mod updater;

fn main() {
    cli::run();
}
