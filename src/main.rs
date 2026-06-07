#![allow(dead_code)]

use anyhow::{bail, Context};
use clap::Parser;
use eve_ded_route::cli::{Cli, Commands};
use eve_ded_route::config::AppConfig;
use eve_ded_route::data::cache::load_system_activity;
use eve_ded_route::data::sde::SdeData;
use eve_ded_route::esi;
use eve_ded_route::graph::highsec_graph::build_highsec_graph;
use eve_ded_route::output;
use eve_ded_route::routing::candidate_filter::filter_candidates;
use eve_ded_route::routing::generator::generate_route;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let config = AppConfig::from_optional_path(cli.config.as_deref())?;

    match cli.command {
        Commands::Generate(options) => {
            let config = config.with_cli_overrides(&options);
            let Some(sde_path) = &config.data.sde_path else {
                output::text::print_generation_summary(&config)
                    .context("failed to write generation summary")?;
                return Ok(());
            };

            let sde_data = SdeData::load_from_path(sde_path)
                .with_context(|| format!("failed to load SDE data from {}", sde_path.display()))?;
            tracing::info!(
                systems = sde_data.systems.len(),
                stargate_connections = sde_data.stargate_connections.len(),
                skipped_unknown_stargate_edges = sde_data.skipped_unknown_stargate_edges(),
                "loaded SDE data"
            );

            let start_name = config
                .start
                .system
                .as_deref()
                .context("generate requires a start system name (--start or [start].system)")?;
            let start_system_id = sde_data.system_id_by_name(start_name).with_context(|| {
                format!("start system {start_name:?} was not found in SDE data")
            })?;
            let graph = build_highsec_graph(
                sde_data.systems.values().cloned(),
                sde_data.stargate_connections.clone(),
                config.filter.min_security_status,
            );
            if !graph.contains_system(start_system_id) {
                bail!("start system {start_name:?} is not present in the high-sec routing graph");
            }

            let activity = load_system_activity(&config).await?;
            let candidates = filter_candidates(&graph, start_system_id, &activity, &config);
            let route = generate_route(&graph, start_system_id, &candidates, &config);

            if let Some(output_path) = &config.route.output_path {
                output::text::write_route(&route, output_path).with_context(|| {
                    format!("failed to write text route to {}", output_path.display())
                })?;
            } else {
                output::text::print_route(&route)
                    .context("failed to write text route to stdout")?;
            }

            if let Some(json_path) = &config.route.json_path {
                output::json::write_route(&route, json_path).with_context(|| {
                    format!("failed to write JSON route to {}", json_path.display())
                })?;
            }
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
