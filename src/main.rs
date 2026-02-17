mod api_client;
mod archive;
mod cli;
mod commands;
mod config;
mod constants;
mod detection;
mod errors;
mod names;
mod output;
mod resolve;
mod updater;

fn main() {
    cli::run();
}
