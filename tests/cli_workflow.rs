use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_eve-ded-route")
}

fn temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "eve-ded-route-cli-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn write_highsec_sde(root: &Path) -> PathBuf {
    let sde = root.join("sde");
    fs::create_dir_all(&sde).unwrap();
    fs::write(
        sde.join("systems.csv"),
        "system_id,system_name,security_status,region_id,constellation_id\n100,Start,0.9,1,10\n101,One,0.8,1,10\n102,Two,0.7,1,10\n103,Three,0.6,1,10\n",
    )
    .unwrap();
    fs::write(
        sde.join("stargates.csv"),
        "from_system_id,to_system_id\n100,101\n101,102\n102,103\n",
    )
    .unwrap();
    sde
}

fn write_lowsec_start_sde(root: &Path) -> PathBuf {
    let sde = root.join("sde-lowsec");
    fs::create_dir_all(&sde).unwrap();
    fs::write(
        sde.join("systems.csv"),
        "system_id,system_name,security_status,region_id,constellation_id\n100,Low Start,0.4,1,10\n101,One,0.8,1,10\n",
    )
    .unwrap();
    fs::write(
        sde.join("stargates.csv"),
        "from_system_id,to_system_id\n100,101\n",
    )
    .unwrap();
    sde
}

fn write_activity_cache(root: &Path) -> PathBuf {
    let cache = root.join("activity.json");
    fs::write(
        &cache,
        r#"{
  "fetched_at": "2026-06-07T00:00:00Z",
  "expires_at": "2999-01-01T00:00:00Z",
  "activity_by_system_id": {
    "100": {"system_id": 100, "jumps_last_hour": 0, "npc_kills_last_hour": 0, "ship_kills_last_hour": 0, "pod_kills_last_hour": 0},
    "101": {"system_id": 101, "jumps_last_hour": 4, "npc_kills_last_hour": 10, "ship_kills_last_hour": 0, "pod_kills_last_hour": 0},
    "102": {"system_id": 102, "jumps_last_hour": 3, "npc_kills_last_hour": 8, "ship_kills_last_hour": 0, "pod_kills_last_hour": 0},
    "103": {"system_id": 103, "jumps_last_hour": 2, "npc_kills_last_hour": 6, "ship_kills_last_hour": 0, "pod_kills_last_hour": 0}
  }
}
"#,
    )
    .unwrap();
    cache
}

fn write_config(root: &Path, sde: &Path, cache: &Path, start: &str) -> PathBuf {
    let config = root.join("config.toml");
    fs::write(
        &config,
        format!(
            r#"
[start]
system = {start:?}

[data]
sde_path = {sde:?}

[route]
waypoint_count = 2
route_history_enabled = false
prefer_loop = false
trade_hub_radius = 0

[filter]
activity_behavior = "disabled"
trade_hub_behavior = "disabled"
trade_hubs = []
max_jumps_last_hour = 1000
max_npc_kills_last_hour = 1000
max_ship_kills_last_hour = 1000
max_pod_kills_last_hour = 1000

[avoid]
systems = []
regions = []
region_ids = []

[esi]
activity_cache_path = {cache:?}
activity_cache_minutes = 60
"#
        ),
    )
    .unwrap();
    config
}

fn write_region_sde(root: &Path, include_regions: bool) -> PathBuf {
    let sde = root.join(if include_regions {
        "sde-regions"
    } else {
        "sde-no-regions"
    });
    fs::create_dir_all(&sde).unwrap();
    fs::write(
        sde.join("systems.csv"),
        "system_id,system_name,security_status,region_id,constellation_id\n100,Start,0.9,10,10\n101,Blocked Transit,0.8,20,20\n102,Blocked Candidate,0.8,20,20\n103,Detour,0.8,30,30\n104,Destination,0.8,30,30\n",
    )
    .unwrap();
    fs::write(
        sde.join("stargates.csv"),
        "from_system_id,to_system_id\n100,101\n101,104\n101,102\n100,103\n103,104\n",
    )
    .unwrap();
    if include_regions {
        fs::write(
            sde.join("regions.csv"),
            "region_id,region_name\n20,Exordium\n30,Allowed\n",
        )
        .unwrap();
    }
    sde
}

fn write_region_activity_cache(root: &Path) -> PathBuf {
    let cache = root.join("region-activity.json");
    fs::write(
        &cache,
        r#"{
  "fetched_at": "2026-06-07T00:00:00Z",
  "expires_at": "2999-01-01T00:00:00Z",
  "activity_by_system_id": {
    "100": {"system_id": 100, "jumps_last_hour": 0, "npc_kills_last_hour": 0, "ship_kills_last_hour": 0, "pod_kills_last_hour": 0},
    "101": {"system_id": 101, "jumps_last_hour": 0, "npc_kills_last_hour": 0, "ship_kills_last_hour": 0, "pod_kills_last_hour": 0},
    "102": {"system_id": 102, "jumps_last_hour": 0, "npc_kills_last_hour": 0, "ship_kills_last_hour": 0, "pod_kills_last_hour": 0},
    "103": {"system_id": 103, "jumps_last_hour": 999, "npc_kills_last_hour": 0, "ship_kills_last_hour": 0, "pod_kills_last_hour": 0},
    "104": {"system_id": 104, "jumps_last_hour": 0, "npc_kills_last_hour": 0, "ship_kills_last_hour": 0, "pod_kills_last_hour": 0}
  }
}
"#,
    )
    .unwrap();
    cache
}

fn write_region_config(
    root: &Path,
    sde: &Path,
    cache: &Path,
    start: &str,
    avoid_regions: &str,
    avoid_region_ids: &str,
) -> PathBuf {
    let config = root.join("region-config.toml");
    fs::write(
        &config,
        format!(
            r#"
[start]
system = {start:?}

[data]
sde_path = {sde:?}

[route]
waypoint_count = 1
route_history_enabled = false
prefer_loop = false
trade_hub_radius = 0

[filter]
activity_behavior = "hard_exclude"
trade_hub_behavior = "disabled"
trade_hubs = []
max_jumps_last_hour = 10
max_npc_kills_last_hour = 1000
max_ship_kills_last_hour = 1000
max_pod_kills_last_hour = 1000

[avoid]
systems = []
regions = {avoid_regions}
region_ids = {avoid_region_ids}

[esi]
activity_cache_path = {cache:?}
activity_cache_minutes = 60
"#
        ),
    )
    .unwrap();
    config
}

fn run(args: &[&str]) -> Output {
    Command::new(bin()).args(args).output().unwrap()
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn generate_help_includes_safety_wording() {
    let output = run(&["generate", "--help"]);
    assert!(output.status.success());
    let text = combined_output(&output);
    assert!(text.contains("public ESI activity data"));
    assert!(text.contains("static/local SDE data"));
    assert!(text.contains("does not scan live anomalies"));
    assert!(text.contains("parse probe scanner results"));
    assert!(text.contains("automate the EVE UI"));
    assert!(text.contains("click in-client"));
    assert!(text.contains("interact with the EVE client process"));
}

#[test]
fn start_system_not_found_returns_clear_error() {
    let root = temp_dir("missing-start");
    let sde = write_highsec_sde(&root);
    let cache = write_activity_cache(&root);
    let config = write_config(&root, &sde, &cache, "Missing");

    let output = run(&["--config", config.to_str().unwrap(), "generate"]);

    assert!(!output.status.success());
    assert!(combined_output(&output).contains("start system not found"));
}

#[test]
fn start_system_below_045_returns_clear_error() {
    let root = temp_dir("lowsec-start");
    let sde = write_lowsec_start_sde(&root);
    let cache = write_activity_cache(&root);
    let config = write_config(&root, &sde, &cache, "Low Start");

    let output = run(&["--config", config.to_str().unwrap(), "generate"]);

    assert!(!output.status.success());
    let text = combined_output(&output);
    assert!(text.contains("start system not high-sec"));
    assert!(text.contains("0.400"));
}

#[test]
fn all_three_modes_generation_returns_three_route_outputs() {
    let root = temp_dir("all-modes");
    let sde = write_highsec_sde(&root);
    let cache = write_activity_cache(&root);
    let config = write_config(&root, &sde, &cache, "Start");
    let json = root.join("routes.json");
    let text = root.join("routes.txt");

    let output = run(&[
        "--config",
        config.to_str().unwrap(),
        "generate",
        "--all-modes",
        "--json",
        json.to_str().unwrap(),
        "--output",
        text.to_str().unwrap(),
    ]);

    assert!(output.status.success(), "{}", combined_output(&output));
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(json).unwrap()).unwrap();
    assert_eq!(value["routes"].as_array().unwrap().len(), 3);
    let summary = fs::read_to_string(text).unwrap();
    assert!(summary.contains("ultra_quiet summary"));
    assert!(summary.contains("dense_quiet summary"));
    assert!(summary.contains("sweep summary"));
}

#[test]
fn push_validation_requires_cli_or_config_character_id() {
    let root = temp_dir("push-missing-character-id");
    let sde = write_highsec_sde(&root);
    let cache = write_activity_cache(&root);
    let config = write_config(&root, &sde, &cache, "Start");

    let output = run(&[
        "--config",
        config.to_str().unwrap(),
        "generate",
        "--push-waypoints",
    ]);

    assert!(!output.status.success());
    let text = combined_output(&output);
    assert!(text.contains("pushing waypoints requires a character ID"));
    assert!(text.contains("--character-id"));
    assert!(text.contains("[character].id"));
}

#[test]
fn push_waypoints_requires_esi_config_auth() {
    let root = temp_dir("push-requires-esi");
    let config = root.join("config.toml");
    fs::write(&config, "[route]\npush_waypoints = false\n").unwrap();

    let output = run(&[
        "--config",
        config.to_str().unwrap(),
        "generate",
        "--push-waypoints",
        "--character-id",
        "42",
    ]);

    assert!(!output.status.success());
    assert!(combined_output(&output).contains("requires ESI config"));
}

#[test]
fn route_generation_does_not_require_in_game_route_optimization() {
    let root = temp_dir("no-client-route-optimization");
    let sde = write_highsec_sde(&root);
    let cache = write_activity_cache(&root);
    let config = write_config(&root, &sde, &cache, "Start");
    let json = root.join("route.json");

    let output = run(&[
        "--config",
        config.to_str().unwrap(),
        "generate",
        "--json",
        json.to_str().unwrap(),
    ]);

    assert!(output.status.success(), "{}", combined_output(&output));
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(json).unwrap()).unwrap();
    assert_eq!(value["start_system"], "Start");
    assert!(value["waypoints"].as_array().unwrap().len() > 0);
}

#[test]
fn avoid_region_name_works_when_regions_csv_exists_and_paths_do_not_cross_it() {
    let root = temp_dir("avoid-region-name");
    let sde = write_region_sde(&root, true);
    let cache = write_region_activity_cache(&root);
    let config = write_region_config(&root, &sde, &cache, "Start", r#"["Exordium"]"#, "[]");
    let json = root.join("route.json");

    let output = run(&[
        "--config",
        config.to_str().unwrap(),
        "generate",
        "--json",
        json.to_str().unwrap(),
    ]);

    assert!(output.status.success(), "{}", combined_output(&output));
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(json).unwrap()).unwrap();
    let route = &value;
    let waypoint_ids: Vec<i64> = route["waypoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|waypoint| waypoint["system_id"].as_i64().unwrap())
        .collect();
    let path_ids: Vec<i64> = route["legs"][0]["path_system_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|system_id| system_id.as_i64().unwrap())
        .collect();

    assert_eq!(waypoint_ids, vec![104]);
    assert_eq!(path_ids, vec![100, 103, 104]);
    assert!(!path_ids.contains(&101));
    assert!(!waypoint_ids.contains(&102));
}

#[test]
fn avoid_region_name_fails_clearly_when_regions_file_is_absent() {
    let root = temp_dir("avoid-region-name-missing-data");
    let sde = write_region_sde(&root, false);
    let cache = write_region_activity_cache(&root);
    let config = write_region_config(&root, &sde, &cache, "Start", r#"["Exordium"]"#, "[]");

    let output = run(&["--config", config.to_str().unwrap(), "generate"]);

    assert!(!output.status.success());
    let text = combined_output(&output);
    assert!(text.contains("[avoid].regions"));
    assert!(text.contains("regions.csv or regions.json"));
}

#[test]
fn unknown_avoid_region_name_fails_clearly() {
    let root = temp_dir("avoid-region-name-unknown");
    let sde = write_region_sde(&root, true);
    let cache = write_region_activity_cache(&root);
    let config = write_region_config(&root, &sde, &cache, "Start", r#"["Unknown"]"#, "[]");

    let output = run(&["--config", config.to_str().unwrap(), "generate"]);

    assert!(!output.status.success());
    let text = combined_output(&output);
    assert!(text.contains("unknown avoided region name"));
    assert!(text.contains("Unknown"));
}

#[test]
fn start_system_in_avoided_region_fails_clearly() {
    let root = temp_dir("start-in-avoided-region");
    let sde = write_region_sde(&root, true);
    let cache = write_region_activity_cache(&root);
    let config = write_region_config(&root, &sde, &cache, "Blocked Transit", "[]", "[20]");

    let output = run(&["--config", config.to_str().unwrap(), "generate"]);

    assert!(!output.status.success());
    let text = combined_output(&output);
    assert!(text.contains("start system"));
    assert!(text.contains("avoided region ID 20"));
}
