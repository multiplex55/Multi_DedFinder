use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use eve_ded_route::config::{AppConfig, FilterBehavior};
use eve_ded_route::data::esi_activity::SystemActivity;
use eve_ded_route::graph::highsec_graph::{build_highsec_graph, HighsecGraph};
use eve_ded_route::model::route::{GeneratedRoute, RouteMode};
use eve_ded_route::model::score::ScoredSystem;
use eve_ded_route::model::system::{SolarSystem, StargateConnection};
use eve_ded_route::routing::candidate_filter::filter_candidates;
use eve_ded_route::routing::generator::generate_route;
use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct FixtureCase {
    pub graph: HighsecGraph,
    pub activity: HashMap<i32, SystemActivity>,
}

#[derive(Debug, Deserialize)]
struct SystemRow {
    id: i32,
    name: String,
    security_status: f32,
    region_id: i32,
    constellation_id: i32,
}

impl From<SystemRow> for SolarSystem {
    fn from(row: SystemRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            security_status: row.security_status,
            region_id: row.region_id,
            constellation_id: row.constellation_id,
        }
    }
}

pub fn load_fixture(name: &str) -> FixtureCase {
    let root = fixture_dir(name);
    let systems = read_csv::<SystemRow>(&root.join("systems.csv"))
        .into_iter()
        .map(SolarSystem::from)
        .collect::<Vec<_>>();
    let stargates = read_csv::<StargateConnection>(&root.join("stargates.csv"));
    let activity = read_csv::<SystemActivity>(&root.join("activity.csv"))
        .into_iter()
        .map(|activity| (activity.system_id, activity))
        .collect::<HashMap<_, _>>();

    FixtureCase {
        graph: build_highsec_graph(systems, stargates, 0.45),
        activity,
    }
}

pub fn config(mode: RouteMode, waypoint_count: usize) -> AppConfig {
    let mut config = AppConfig::default();
    config.route.mode = mode;
    config.route.waypoint_count = waypoint_count;
    config.route.prefer_loop = false;
    config.route.trade_hub_radius = 0;
    config.filter.activity_behavior = FilterBehavior::SoftPenalty;
    config.filter.trade_hub_behavior = FilterBehavior::Disabled;
    config.filter.max_jumps_last_hour = None;
    config.filter.max_npc_kills_last_hour = None;
    config.filter.max_ship_kills_last_hour = None;
    config.filter.max_pod_kills_last_hour = None;
    config.filter.trade_hubs.clear();
    config.avoid.systems.clear();
    config
}

pub fn hard_activity_filter_config(mode: RouteMode, waypoint_count: usize) -> AppConfig {
    let mut config = config(mode, waypoint_count);
    config.filter.activity_behavior = FilterBehavior::HardExclude;
    config.filter.max_jumps_last_hour = Some(20);
    config.filter.max_npc_kills_last_hour = Some(100);
    config.filter.max_ship_kills_last_hour = Some(5);
    config.filter.max_pod_kills_last_hour = Some(2);
    config
}

pub fn route_from_fixture(name: &str, start_system_id: i32, config: &AppConfig) -> GeneratedRoute {
    let fixture = load_fixture(name);
    let candidates = candidates_without_start(&fixture, start_system_id, config);
    generate_route(&fixture.graph, start_system_id, &candidates, config)
}

pub fn candidates_without_start(
    fixture: &FixtureCase,
    start_system_id: i32,
    config: &AppConfig,
) -> Vec<ScoredSystem> {
    filter_candidates(&fixture.graph, start_system_id, &fixture.activity, config)
        .into_iter()
        .filter(|candidate| candidate.system_id != start_system_id)
        .collect()
}

pub fn waypoint_ids(route: &GeneratedRoute) -> Vec<i32> {
    route
        .waypoints
        .iter()
        .map(|waypoint| waypoint.system_id)
        .collect()
}

pub fn all_route_system_ids(route: &GeneratedRoute) -> Vec<i32> {
    route
        .legs
        .iter()
        .flat_map(|leg| leg.path_system_ids.iter().copied())
        .collect()
}

pub fn reused_transit_hops(route: &GeneratedRoute) -> usize {
    let mut seen = HashSet::new();
    route
        .legs
        .iter()
        .flat_map(|leg| leg.path_system_ids.iter().skip(1).copied())
        .filter(|system_id| !seen.insert(*system_id))
        .count()
}

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn read_csv<T>(path: &Path) -> Vec<T>
where
    T: for<'de> Deserialize<'de>,
{
    let mut reader = csv::Reader::from_path(path)
        .unwrap_or_else(|error| panic!("failed to open fixture {}: {error}", path.display()));
    reader
        .deserialize()
        .collect::<Result<Vec<T>, _>>()
        .unwrap_or_else(|error| panic!("failed to parse fixture {}: {error}", path.display()))
}
