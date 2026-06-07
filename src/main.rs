#![allow(dead_code)]

mod cli;
mod config;
mod data;
mod error;
mod esi;
mod graph;
mod model;
mod output;
mod routing;

use anyhow::Context;
use clap::Parser;
use cli::{Cli, Commands};
use config::AppConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let config = AppConfig::from_optional_path(cli.config.as_deref())?;

    match cli.command {
        Commands::Generate(options) => {
            let config = config.with_cli_overrides(&options);
            output::text::print_generation_summary(&config)
                .context("failed to write generation summary")?;
        }
        Commands::Push(options) => {
            let config = config.with_cli_overrides(&options);
            esi::waypoint::push_waypoints(&config)
                .await
                .context("failed to push waypoints")?;
        }
    }

    Ok(())
}
