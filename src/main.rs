#![allow(dead_code)]

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Parser;
use eve_ded_route::cli::{Cli, CliOptions, Commands};
use eve_ded_route::config::{AppConfig, StartSource};
use eve_ded_route::data::cache::load_system_activity;
use eve_ded_route::data::route_history::{load_route_history, save_route_history};
use eve_ded_route::data::sde::SdeData;
use eve_ded_route::esi;
use eve_ded_route::esi::auth::{Character, EsiAuthConfig};
use eve_ded_route::esi::location::LOCATION_SCOPE;
use eve_ded_route::esi::waypoint::PushOptions;
use eve_ded_route::graph::highsec_graph::build_highsec_graph;
use eve_ded_route::model::route::{GeneratedRoute, RouteMode};
use eve_ded_route::model::system::SolarSystem;
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
    validate_push_configuration_if_requested(&config)?;

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

    let start_system = resolve_start_system(&config, &options, &sde_data).await?;
    let start_name = start_system.name.clone();
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

async fn resolve_start_system<'a>(
    config: &AppConfig,
    options: &CliOptions,
    sde_data: &'a SdeData,
) -> Result<&'a SolarSystem> {
    resolve_start_system_with_location_fetcher(
        config,
        options,
        sde_data,
        |character_id, access_token| async move {
            esi::location::get_character_location(character_id, &access_token).await
        },
    )
    .await
}

async fn resolve_start_system_with_location_fetcher<'a, Fetch, Fut>(
    config: &AppConfig,
    options: &CliOptions,
    sde_data: &'a SdeData,
    fetch_location: Fetch,
) -> Result<&'a SolarSystem>
where
    Fetch: FnOnce(i64, String) -> Fut,
    Fut: std::future::Future<Output = Result<esi::location::CharacterLocation>>,
{
    if options.start.is_some() || config.start.source == StartSource::Config {
        return resolve_configured_start_system(config, sde_data);
    }

    match config.start.source {
        StartSource::Config => resolve_configured_start_system(config, sde_data),
        StartSource::CharacterLocation => {
            match resolve_character_location_start_system(config, sde_data, fetch_location).await {
                Ok(system) => Ok(system),
                Err(error) if config.start.fallback_to_config_system => {
                    tracing::warn!(error = %error, "falling back to configured start system after character-location start resolution failed");
                    resolve_configured_start_system(config, sde_data)
                        .context("character-location start resolution failed and configured start fallback also failed")
                }
                Err(error) => Err(error),
            }
        }
    }
}

async fn resolve_character_location_start_system<'a, Fetch, Fut>(
    config: &AppConfig,
    sde_data: &'a SdeData,
    fetch_location: Fetch,
) -> Result<&'a SolarSystem>
where
    Fetch: FnOnce(i64, String) -> Fut,
    Fut: std::future::Future<Output = Result<esi::location::CharacterLocation>>,
{
    let character_id = config.character.id.context(
        "character-location start requires a character ID; set --character-id or [character].id",
    )?;
    let client_id = config.esi.client_id.clone().context(
        "character-location start requires ESI config: set [esi].client_id before authentication",
    )?;
    let character = Character::new(character_id, config.character.name.clone());
    let required_scopes = if config.route.push_waypoints {
        vec![LOCATION_SCOPE, eve_ded_route::esi::auth::WAYPOINT_SCOPE]
    } else {
        vec![LOCATION_SCOPE]
    };
    let auth_config = EsiAuthConfig::new_with_required_scopes(
        client_id,
        config.esi.callback_url.clone(),
        config.esi.scopes.clone(),
        &required_scopes,
    )?;
    let token = esi::location::token_for_location(&auth_config, &character).await?;
    let location = fetch_location(character_id, token.access_token.clone())
        .await
        .context("failed to fetch authenticated character location from ESI")?;
    resolve_location_system(location.solar_system_id, sde_data)
}

fn resolve_configured_start_system<'a>(
    config: &AppConfig,
    sde_data: &'a SdeData,
) -> Result<&'a SolarSystem> {
    let start_name = config
        .start
        .system
        .as_deref()
        .context("generate requires a start system name (--start or [start].system)")?;
    let start_system = sde_data.system_by_name(start_name).with_context(|| {
        format!("start system not found: {start_name:?} was not found in SDE data")
    })?;
    validate_highsec_start(start_system, start_name)?;
    Ok(start_system)
}

fn resolve_location_system(system_id: i32, sde_data: &SdeData) -> Result<&SolarSystem> {
    let system = sde_data.systems.get(&system_id).with_context(|| {
        format!("current-location start system ID {system_id} was not found in SDE data")
    })?;
    validate_highsec_start(system, &system.name)?;
    Ok(system)
}

fn validate_highsec_start(start_system: &SolarSystem, start_name: &str) -> Result<()> {
    if start_system.security_status < HIGH_SEC_START_MINIMUM {
        bail!(
            "start system not high-sec: {start_name:?} has security status {:.3}, below required high-sec minimum {:.2}",
            start_system.security_status,
            HIGH_SEC_START_MINIMUM
        );
    }
    Ok(())
}

async fn run_push(config: AppConfig, options: CliOptions) -> Result<()> {
    let config = config.with_cli_overrides(&options);
    let route_path = route_json_path_for_push(&config, &options)?;
    let route = load_route_json(&route_path, &config)?;
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

fn route_json_path_for_push(config: &AppConfig, options: &CliOptions) -> Result<PathBuf> {
    options
        .json
        .clone()
        .or_else(|| config.route.json_path.clone())
        .context(
            "push requires a route JSON file: set --json PATH or [route].json_path, then run generate first if the file does not exist",
        )
}

fn load_route_json(route_path: &Path, config: &AppConfig) -> Result<GeneratedRoute> {
    let route_contents = match std::fs::read_to_string(route_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut message = format!(
                "route JSON file not found at attempted path {}. Run `eve-ded-route --config config.toml generate` first to create it",
                route_path.display()
            );
            if config.route.push_waypoints {
                message.push_str(
                    ", or run `eve-ded-route --config config.toml generate --push-waypoints` to generate and push in one command",
                );
            }
            bail!(message);
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!("failed to read route JSON from {}", route_path.display())
            });
        }
    };
    serde_json::from_str(&route_contents)
        .with_context(|| format!("failed to parse route JSON from {}", route_path.display()))
}

async fn push_route_from_options(
    config: &AppConfig,
    options: &CliOptions,
    route: &GeneratedRoute,
) -> Result<()> {
    let character = character_for_push(config)?;
    let push_options = PushOptions {
        dry_run: options.dry_run.unwrap_or(false),
        yes: options.yes.unwrap_or(false),
    };
    esi::waypoint::push_waypoints_from_config(config, character, route, &push_options).await?;
    Ok(())
}

fn character_id_for_push(config: &AppConfig) -> Result<i64> {
    config
        .character
        .id
        .context("pushing waypoints requires a character ID; set --character-id or [character].id")
}

fn character_for_push(config: &AppConfig) -> Result<Character> {
    Ok(Character::new(
        character_id_for_push(config)?,
        config.character.name.clone(),
    ))
}

fn validate_push_configuration_if_requested(config: &AppConfig) -> Result<()> {
    if !config.route.push_waypoints {
        return Ok(());
    }
    character_id_for_push(config)?;
    if config.esi.client_id.is_none() {
        bail!("--push-waypoints requires ESI config: set [esi].client_id before authentication");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::Utc;
    use eve_ded_route::model::route::{RouteLeg, RouteWaypoint};
    use eve_ded_route::model::score::ScoreBreakdown;
    use serde_json::json;

    use super::*;

    fn temp_route_path(file_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "eve-ded-route-main-tests-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("test temp directory should be created");
        dir.join(file_name)
    }

    fn score_breakdown() -> ScoreBreakdown {
        ScoreBreakdown {
            activity: 0.2,
            distance: 0.3,
            security: 0.4,
            jump_score: 0.2,
            npc_score: 0.3,
            danger_score: 0.4,
            cluster_density_score: 0.5,
            hub_distance_score: 0.6,
            dead_end_penalty: 0.0,
            reuse_penalty: 0.0,
            total: 0.9,
        }
    }

    fn waypoint() -> RouteWaypoint {
        RouteWaypoint {
            order: 1,
            system_id: 30_000_142,
            system_name: "Jita".to_string(),
            security_status: 0.946,
            region_id: 10_000_002,
            constellation_id: 20_000_020,
            score: 0.9,
            jumps_last_hour: 12,
            npc_kills_last_hour: 3,
            ship_kills_last_hour: 2,
            pod_kills_last_hour: 1,
            distance_from_start: 0,
            score_breakdown: score_breakdown(),
        }
    }

    fn route_with_waypoints(waypoints: Vec<RouteWaypoint>) -> GeneratedRoute {
        GeneratedRoute {
            start_system: "Jita".to_string(),
            start_system_id: 30_000_142,
            mode: RouteMode::DenseQuiet,
            highsec_only: true,
            total_jumps: 1,
            average_score: 0.9,
            activity_timestamp: Utc::now(),
            config_used: json!({"route": {"waypoint_count": waypoints.len()}}),
            waypoints,
            legs: vec![RouteLeg {
                from_system_id: 30_000_142,
                to_system_id: 30_000_141,
                jump_count: 1,
                path_system_ids: vec![30_000_142, 30_000_141],
                path_system_names: vec!["Jita".to_string(), "Perimeter".to_string()],
            }],
        }
    }

    fn write_route(path: &Path, route: &GeneratedRoute) {
        let contents = serde_json::to_string(route).expect("route should serialize");
        fs::write(path, contents).expect("route JSON should be written");
    }

    fn write_test_sde(systems: &str) -> SdeData {
        let systems_path = temp_route_path("systems.csv");
        let stargates_path = systems_path.with_file_name("stargates.csv");
        fs::write(&systems_path, systems).expect("systems CSV should be written");
        fs::write(
            &stargates_path,
            "from_system_id,to_system_id\n100,101\n101,102\n",
        )
        .expect("stargates CSV should be written");
        SdeData::load_from_files(&systems_path, &stargates_path).expect("test SDE should load")
    }

    fn highsec_test_sde() -> SdeData {
        write_test_sde(
            "system_id,system_name,security_status,region_id,constellation_id\n100,Start,0.9,1,10\n101,Current,0.8,1,10\n102,Other,0.7,1,10\n",
        )
    }

    #[test]
    fn current_location_solar_system_id_maps_to_sde_system() {
        let sde_data = highsec_test_sde();

        let system = resolve_location_system(101, &sde_data)
            .expect("current location should map to SDE system");

        assert_eq!(system.name, "Current");
        assert_eq!(system.id, 101);
    }

    #[test]
    fn lowsec_current_location_returns_highsec_validation_error() {
        let sde_data = write_test_sde(
            "system_id,system_name,security_status,region_id,constellation_id\n100,Start,0.9,1,10\n101,Danger,0.4,1,10\n102,Other,0.7,1,10\n",
        );

        let error = resolve_location_system(101, &sde_data)
            .expect_err("low-sec current location should fail high-sec validation");
        let message = error.to_string();

        assert!(message.contains("start system not high-sec"));
        assert!(message.contains("0.400"));
    }

    #[test]
    fn unknown_current_location_system_id_returns_sde_lookup_error() {
        let sde_data = highsec_test_sde();

        let error = resolve_location_system(999, &sde_data)
            .expect_err("unknown current location should fail SDE lookup");

        assert!(error
            .to_string()
            .contains("current-location start system ID 999 was not found in SDE data"));
    }

    #[tokio::test]
    async fn cli_start_overrides_current_location_source() {
        let sde_data = highsec_test_sde();
        let mut config = AppConfig::default();
        config.start.system = Some("Start".to_string());
        config.start.source = StartSource::CharacterLocation;
        let options = CliOptions {
            start: Some("Start".to_string()),
            ..Default::default()
        };

        let system = resolve_start_system_with_location_fetcher(
            &config,
            &options,
            &sde_data,
            |_character_id, _access_token| async {
                anyhow::bail!("location fetch should not be used for CLI --start")
            },
        )
        .await
        .expect("CLI --start should use configured start resolution");

        assert_eq!(system.name, "Start");
    }

    #[tokio::test]
    async fn fallback_is_not_used_unless_enabled() {
        let sde_data = highsec_test_sde();
        let mut config = AppConfig::default();
        config.start.system = Some("Start".to_string());
        config.start.source = StartSource::CharacterLocation;
        config.character.id = Some(42);
        let options = CliOptions::default();

        let error = resolve_start_system_with_location_fetcher(
            &config,
            &options,
            &sde_data,
            |_character_id, _access_token| async {
                anyhow::bail!("location fetch should not be reached without ESI config")
            },
        )
        .await
        .expect_err("fallback should be disabled by default");
        assert!(error
            .to_string()
            .contains("character-location start requires ESI config"));

        config.start.fallback_to_config_system = true;
        let system = resolve_start_system_with_location_fetcher(
            &config,
            &options,
            &sde_data,
            |_character_id, _access_token| async {
                anyhow::bail!("location fetch should not be reached without ESI config")
            },
        )
        .await
        .expect("enabled fallback should use [start].system");

        assert_eq!(system.name, "Start");
    }

    #[test]
    fn push_uses_config_route_json_path_when_cli_json_is_omitted() {
        let config_path = PathBuf::from("config-route.json");
        let mut config = AppConfig::default();
        config.route.json_path = Some(config_path.clone());
        let options = CliOptions::default();

        let path = route_json_path_for_push(&config, &options).expect("path should resolve");

        assert_eq!(path, config_path);
    }

    #[test]
    fn push_cli_json_overrides_config_route_json_path() {
        let cli_path = PathBuf::from("cli-route.json");
        let mut config = AppConfig::default();
        config.route.json_path = Some(PathBuf::from("config-route.json"));
        let options = CliOptions {
            json: Some(cli_path.clone()),
            ..Default::default()
        };

        let path = route_json_path_for_push(&config, &options).expect("path should resolve");

        assert_eq!(path, cli_path);
    }

    #[test]
    fn missing_route_json_returns_clear_error() {
        let missing_path = temp_route_path("missing-route.json");
        let mut config = AppConfig::default();
        config.route.push_waypoints = true;

        let error = load_route_json(&missing_path, &config).expect_err("missing file should fail");
        let message = error.to_string();

        assert!(
            message.contains(&missing_path.display().to_string()),
            "error should include attempted path: {message}"
        );
        assert!(
            message.contains("eve-ded-route --config config.toml generate"),
            "error should suggest generate command: {message}"
        );
        assert!(
            message.contains("generate --push-waypoints"),
            "error should mention generate --push-waypoints when configured: {message}"
        );
    }

    #[test]
    fn existing_route_json_loads_successfully() {
        let route_path = temp_route_path("route.json");
        let route = route_with_waypoints(vec![waypoint()]);
        write_route(&route_path, &route);

        let loaded = load_route_json(&route_path, &AppConfig::default())
            .expect("existing route JSON should load");

        assert_eq!(loaded, route);
    }

    #[tokio::test]
    async fn zero_waypoint_route_still_fails_existing_safety_check() {
        let route_path = temp_route_path("empty-route.json");
        write_route(&route_path, &route_with_waypoints(Vec::new()));
        let mut config = AppConfig::default();
        config.route.json_path = Some(route_path);

        let error = run_push(config, CliOptions::default())
            .await
            .expect_err("zero-waypoint route should not be pushed");

        assert_eq!(
            error.to_string(),
            "cannot push route because it has zero waypoints"
        );
    }
}
