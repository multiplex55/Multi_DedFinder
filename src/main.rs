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

use data::sde::SdeData;

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
            if let Some(sde_path) = &config.data.sde_path {
                let sde_data = SdeData::load_from_path(sde_path).with_context(|| {
                    format!("failed to load SDE data from {}", sde_path.display())
                })?;
                tracing::info!(
                    systems = sde_data.systems.len(),
                    stargate_connections = sde_data.stargate_connections.len(),
                    skipped_unknown_stargate_edges = sde_data.skipped_unknown_stargate_edges(),
                    "loaded SDE data"
                );
            }
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
