use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::route::{GeneratedRoute, RouteMode};

pub const DEFAULT_ROUTE_HISTORY_PATH: &str = "routes/history.json";
pub const ROUTE_HISTORY_VERSION: u8 = 1;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RouteHistory {
    pub version: u8,
    pub routes: Vec<RouteHistoryEntry>,
}

impl Default for RouteHistory {
    fn default() -> Self {
        Self {
            version: ROUTE_HISTORY_VERSION,
            routes: Vec::new(),
        }
    }
}

impl RouteHistory {
    #[must_use]
    pub fn systems_used_in_last_route(&self) -> HashSet<i32> {
        self.routes
            .last()
            .map(RouteHistoryEntry::used_system_ids)
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RouteHistoryEntry {
    pub generated_at: DateTime<Utc>,
    pub start_system_id: i32,
    pub mode: RouteMode,
    pub waypoint_system_ids: Vec<i32>,
    pub transit_path_system_ids: Vec<i32>,
}

impl RouteHistoryEntry {
    #[must_use]
    pub fn from_generated_route(route: &GeneratedRoute) -> Self {
        Self {
            generated_at: route.activity_timestamp,
            start_system_id: route.start_system_id,
            mode: route.mode,
            waypoint_system_ids: route
                .waypoints
                .iter()
                .map(|waypoint| waypoint.system_id)
                .collect(),
            transit_path_system_ids: route
                .legs
                .iter()
                .flat_map(|leg| leg.path_system_ids.iter().copied())
                .collect(),
        }
    }

    #[must_use]
    pub fn used_system_ids(&self) -> HashSet<i32> {
        let mut used = HashSet::from([self.start_system_id]);
        used.extend(self.waypoint_system_ids.iter().copied());
        used.extend(self.transit_path_system_ids.iter().copied());
        used
    }
}

pub fn load_route_history(path: &Path) -> Result<RouteHistory> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse route history {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(RouteHistory::default()),
        Err(error) => {
            Err(error).with_context(|| format!("failed to read route history {}", path.display()))
        }
    }
}

pub fn save_route_history(path: &Path, generated_route: &GeneratedRoute) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create route history directory {}",
                parent.display()
            )
        })?;
    }

    let history = RouteHistory {
        version: ROUTE_HISTORY_VERSION,
        routes: vec![RouteHistoryEntry::from_generated_route(generated_route)],
    };
    let contents =
        serde_json::to_string_pretty(&history).context("failed to serialize route history")?;
    fs::write(path, contents)
        .with_context(|| format!("failed to write route history {}", path.display()))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;

    use crate::model::route::{GeneratedRoute, RouteLeg, RouteWaypoint};
    use crate::model::score::ScoreBreakdown;

    use super::*;

    fn history_path(test_name: &str) -> std::path::PathBuf {
        let unique = format!(
            "eve-ded-route-history-{test_name}-{}-{}.json",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap()
        );
        std::env::temp_dir().join(unique)
    }

    fn score_breakdown() -> ScoreBreakdown {
        ScoreBreakdown {
            activity: 0.0,
            distance: 0.0,
            security: 0.0,
            jump_score: 0.0,
            npc_score: 0.0,
            danger_score: 0.0,
            cluster_density_score: 0.0,
            hub_distance_score: 0.0,
            dead_end_penalty: 0.0,
            reuse_penalty: 0.0,
            total: 0.0,
        }
    }

    fn waypoint(order: usize, system_id: i32) -> RouteWaypoint {
        RouteWaypoint {
            order,
            system_id,
            system_name: format!("System {system_id}"),
            security_status: 0.8,
            region_id: 1,
            constellation_id: 1,
            score: 0.8,
            jumps_last_hour: 0,
            npc_kills_last_hour: 0,
            ship_kills_last_hour: 0,
            pod_kills_last_hour: 0,
            distance_from_start: order as u32,
            score_breakdown: score_breakdown(),
        }
    }

    fn route() -> GeneratedRoute {
        GeneratedRoute {
            start_system: "System 1".to_string(),
            start_system_id: 1,
            mode: RouteMode::DenseQuiet,
            highsec_only: true,
            total_jumps: 3,
            average_score: 0.8,
            activity_timestamp: Utc.with_ymd_and_hms(2026, 6, 7, 12, 0, 0).unwrap(),
            config_used: json!({"route": {"waypoint_count": 2}}),
            waypoints: vec![waypoint(1, 3), waypoint(2, 4)],
            legs: vec![
                RouteLeg {
                    from_system_id: 1,
                    to_system_id: 3,
                    jump_count: 2,
                    path_system_ids: vec![1, 2, 3],
                    path_system_names: vec![
                        "System 1".into(),
                        "System 2".into(),
                        "System 3".into(),
                    ],
                },
                RouteLeg {
                    from_system_id: 3,
                    to_system_id: 4,
                    jump_count: 1,
                    path_system_ids: vec![3, 4],
                    path_system_names: vec!["System 3".into(), "System 4".into()],
                },
            ],
        }
    }

    #[test]
    fn absent_history_file_produces_empty_history() {
        let path = history_path("absent");

        let history = load_route_history(&path).expect("absent history should load as empty");

        assert!(history.routes.is_empty());
        assert!(history.systems_used_in_last_route().is_empty());
    }

    #[test]
    fn saving_then_loading_route_history_round_trips() {
        let path = history_path("roundtrip");
        let route = route();

        save_route_history(&path, &route).expect("history should save");
        let loaded = load_route_history(&path).expect("history should load");

        assert_eq!(loaded.version, ROUTE_HISTORY_VERSION);
        assert_eq!(
            loaded.routes,
            vec![RouteHistoryEntry::from_generated_route(&route)]
        );
        assert_eq!(
            loaded.systems_used_in_last_route(),
            HashSet::from([1, 2, 3, 4])
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn malformed_history_reports_clear_error() {
        let path = history_path("malformed");
        fs::write(&path, "not json").expect("malformed fixture should write");

        let error = load_route_history(&path).expect_err("malformed history should fail");

        assert!(format!("{error:#}").contains("failed to parse route history"));
        let _ = fs::remove_file(path);
    }
}
