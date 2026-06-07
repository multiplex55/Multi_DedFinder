#![allow(dead_code)]

use std::path::Path;

use anyhow::{bail, Context, Result};
use clap::Parser;
use eve_ded_route::cli::{Cli, CliOptions, Commands};
use eve_ded_route::config::AppConfig;
use eve_ded_route::data::cache::load_system_activity;
use eve_ded_route::data::route_history::{load_route_history, save_route_history};
use eve_ded_route::data::sde::SdeData;
use eve_ded_route::esi;
use eve_ded_route::esi::auth::Character;
use eve_ded_route::esi::waypoint::PushOptions;
use eve_ded_route::graph::highsec_graph::build_highsec_graph;
use eve_ded_route::model::route::{GeneratedRoute, RouteMode};
use eve_ded_route::output;
use eve_ded_route::routing::candidate_filter::filter_candidates_with_route_history;
use eve_ded_route::routing::generator::{generate_all_modes, generate_route};

const HIGH_SEC_START_MINIMUM: f32 = 0.45;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let config = AppConfig::from_optional_path(cli.config.as_deref())?;

    match cli.command {
        Commands::Generate(options) => run_generate(config, options).await?,
        Commands::Push(options) => run_push(config, options).await?,
    }

    Ok(())
}

async fn run_generate(config: AppConfig, options: CliOptions) -> Result<()> {
    let config = config.with_cli_overrides(&options);
    validate_push_configuration_if_requested(&config, &options)?;

    let sde_path = config.data.sde_path.as_deref().context(
        "SDE file missing: generate requires --sde-path or [data].sde_path pointing to prepared systems/stargates files",
    )?;
    let sde_data = SdeData::load_from_path(sde_path)
        .with_context(|| format!("SDE file missing or unreadable at {}", sde_path.display()))?;
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
    let start_system = sde_data.system_by_name(start_name).with_context(|| {
        format!("start system not found: {start_name:?} was not found in SDE data")
    })?;
    if start_system.security_status < HIGH_SEC_START_MINIMUM {
        bail!(
            "start system not high-sec: {start_name:?} has security status {:.3}, below required high-sec minimum {:.2}",
            start_system.security_status,
            HIGH_SEC_START_MINIMUM
        );
    }
    let start_system_id = start_system.id;

    let graph = build_highsec_graph(
        sde_data.systems.values().cloned(),
        sde_data.stargate_connections.clone(),
        config.filter.min_security_status,
    );
    if !graph.contains_system(start_system_id) {
        bail!(
            "start system not high-sec: {start_name:?} is not present in the high-sec routing graph with minimum security {:.2}",
            config.filter.min_security_status
        );
    }

    let activity = load_system_activity(&config)
        .await
        .context("activity fetch failure")?;
    let route_history_systems = load_history_systems(&config)?;
    let mut candidates = filter_candidates_with_route_history(
        &graph,
        start_system_id,
        &activity,
        &config,
        &route_history_systems,
    );
    candidates.retain(|candidate| candidate.system_id != start_system_id);
    if candidates.is_empty() {
        bail!("high-sec graph has no reachable candidates after applying configured filters");
    }

    let routes = if options.all_modes.unwrap_or(false) {
        generate_all_modes(&graph, start_system_id, &candidates, &config)
    } else {
        vec![generate_route(
            &graph,
            start_system_id,
            &candidates,
            &config,
        )]
    };

    if routes.iter().all(|route| route.waypoints.is_empty()) {
        bail!("high-sec graph has no reachable candidates usable for route generation");
    }

    write_text_output(&routes, &config)?;
    write_json_output(&routes, &config)?;

    if config.route.route_history_enabled {
        let history_path = config
            .route
            .route_history_path
            .as_deref()
            .context("route history is enabled but [route].route_history_path is not set")?;
        let route_for_history = routes
            .iter()
            .find(|route| route.mode == RouteMode::DenseQuiet)
            .unwrap_or(&routes[0]);
        save_route_history(history_path, route_for_history)?;
    }

    if config.route.push_waypoints {
        let route_to_push = routes
            .iter()
            .find(|route| route.mode == config.route.mode)
            .unwrap_or(&routes[0]);
        push_route_from_options(&config, &options, route_to_push)
            .await
            .context("failed to push generated waypoints")?;
    }

    Ok(())
}

async fn run_push(config: AppConfig, options: CliOptions) -> Result<()> {
    let config = config.with_cli_overrides(&options);
    let route_path = options
        .json
        .as_deref()
        .context("push requires --json PATH pointing to a generated route JSON file")?;
    let route = load_route_json(route_path)?;
    if route.waypoints.is_empty() {
        bail!("cannot push route because it has zero waypoints");
    }
    push_route_from_options(&config, &options, &route)
        .await
        .context("failed to push waypoints")?;
    Ok(())
}

fn load_history_systems(config: &AppConfig) -> Result<std::collections::HashSet<i32>> {
    if !config.route.route_history_enabled {
        return Ok(Default::default());
    }

    let history_path = config
        .route
        .route_history_path
        .as_deref()
        .context("route history is enabled but [route].route_history_path is not set")?;
    match load_route_history(history_path) {
        Ok(history) => Ok(history.systems_used_in_last_route()),
        Err(error)
            if config.route.ignore_malformed_route_history
                && error.downcast_ref::<serde_json::Error>().is_some() =>
        {
            tracing::warn!(
                history_path = %history_path.display(),
                error = %error,
                "ignoring malformed route history because config allows it"
            );
            Ok(Default::default())
        }
        Err(error) => Err(error),
    }
}

fn write_text_output(routes: &[GeneratedRoute], config: &AppConfig) -> Result<()> {
    if routes.len() == 1 {
        if let Some(output_path) = &config.route.output_path {
            output::text::write_route(&routes[0], output_path).with_context(|| {
                format!("failed to write text route to {}", output_path.display())
            })?;
        } else {
            output::text::print_route(&routes[0])
                .context("failed to write text route to stdout")?;
        }
    } else if let Some(output_path) = &config.route.output_path {
        output::text::write_routes(routes, output_path).with_context(|| {
            format!(
                "failed to write all-modes text route summary to {}",
                output_path.display()
            )
        })?;
    } else {
        output::text::print_routes(routes)
            .context("failed to write all-modes text routes to stdout")?;
    }
    Ok(())
}

fn write_json_output(routes: &[GeneratedRoute], config: &AppConfig) -> Result<()> {
    let Some(json_path) = &config.route.json_path else {
        return Ok(());
    };

    if routes.len() == 1 {
        output::json::write_route(&routes[0], json_path)
            .with_context(|| format!("failed to write JSON route to {}", json_path.display()))?;
    } else {
        output::json::write_routes(routes, json_path).with_context(|| {
            format!(
                "failed to write combined all-modes JSON routes to {}",
                json_path.display()
            )
        })?;
    }
    Ok(())
}

fn load_route_json(route_path: &Path) -> Result<GeneratedRoute> {
    let route_contents = std::fs::read_to_string(route_path)
        .with_context(|| format!("failed to read route JSON from {}", route_path.display()))?;
    serde_json::from_str(&route_contents)
        .with_context(|| format!("failed to parse route JSON from {}", route_path.display()))
}

async fn push_route_from_options(
    config: &AppConfig,
    options: &CliOptions,
    route: &GeneratedRoute,
) -> Result<()> {
    let character_id = options
        .character_id
        .context("--character-id is required when pushing waypoints")?;
    let character = Character::new(character_id, options.character_name.clone());
    let push_options = PushOptions {
        dry_run: options.dry_run.unwrap_or(false),
        yes: options.yes.unwrap_or(false),
    };
    esi::waypoint::push_waypoints_from_config(config, character, route, &push_options).await?;
    Ok(())
}

fn validate_push_configuration_if_requested(
    config: &AppConfig,
    options: &CliOptions,
) -> Result<()> {
    if !config.route.push_waypoints {
        return Ok(());
    }
    if options.character_id.is_none() {
        bail!("--push-waypoints requires --character-id for ESI authentication");
    }
    if config.esi.client_id.is_none() {
        bail!("--push-waypoints requires ESI config: set [esi].client_id before authentication");
    }
    Ok(())
}
