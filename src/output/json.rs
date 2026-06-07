use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::model::route::GeneratedRoute;
use crate::output::text::validate_route_output_claims;

pub fn to_pretty_json<T: Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string_pretty(value)?)
}

pub fn render_route(route: &GeneratedRoute) -> Result<String> {
    let rendered = to_pretty_json(route)?;
    validate_route_output_claims(&rendered)?;
    Ok(rendered)
}

pub fn write_route(route: &GeneratedRoute, path: impl AsRef<Path>) -> Result<()> {
    fs::write(path, render_route(route)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::{json, Value};

    use super::*;
    use crate::model::route::{GeneratedRoute, RouteLeg, RouteMode, RouteWaypoint};
    use crate::model::score::ScoreBreakdown;

    fn score_breakdown() -> ScoreBreakdown {
        ScoreBreakdown {
            activity: 0.2,
            distance: 0.3,
            security: 0.9,
            jump_score: 0.4,
            npc_score: 0.5,
            danger_score: 0.6,
            cluster_density_score: 0.7,
            hub_distance_score: 0.8,
            dead_end_penalty: 0.0,
            reuse_penalty: 0.1,
            total: 0.75,
        }
    }

    fn route() -> GeneratedRoute {
        GeneratedRoute {
            start_system: "Start".to_string(),
            start_system_id: 30_000_001,
            mode: RouteMode::DenseQuiet,
            highsec_only: true,
            total_jumps: 2,
            average_score: 0.75,
            activity_timestamp: Utc.with_ymd_and_hms(2026, 6, 7, 12, 0, 0).unwrap(),
            config_used: json!({"route": {"waypoint_count": 1}}),
            waypoints: vec![RouteWaypoint {
                order: 1,
                system_id: 30_000_002,
                system_name: "Waypoint".to_string(),
                security_status: 0.7,
                region_id: 10_000_001,
                constellation_id: 20_000_001,
                score: 0.75,
                jumps_last_hour: 3,
                npc_kills_last_hour: 4,
                ship_kills_last_hour: 1,
                pod_kills_last_hour: 0,
                distance_from_start: 2,
                score_breakdown: score_breakdown(),
            }],
            legs: vec![RouteLeg {
                from_system_id: 30_000_001,
                to_system_id: 30_000_002,
                jump_count: 2,
                path_system_ids: vec![30_000_001, 30_000_003, 30_000_002],
                path_system_names: vec![
                    "Start".to_string(),
                    "Middle".to_string(),
                    "Waypoint".to_string(),
                ],
            }],
        }
    }

    #[test]
    fn json_output_includes_score_breakdown() {
        let value: Value = serde_json::from_str(&render_route(&route()).unwrap()).unwrap();

        assert!(value["waypoints"][0]["score_breakdown"].is_object());
        assert_eq!(
            value["waypoints"][0]["score_breakdown"]["total"],
            json!(0.75)
        );
    }

    #[test]
    fn json_output_includes_config_used() {
        let value: Value = serde_json::from_str(&render_route(&route()).unwrap()).unwrap();

        assert_eq!(value["config_used"]["route"]["waypoint_count"], json!(1));
    }

    #[test]
    fn json_output_includes_activity_timestamp() {
        let value: Value = serde_json::from_str(&render_route(&route()).unwrap()).unwrap();

        assert_eq!(value["activity_timestamp"], json!("2026-06-07T12:00:00Z"));
    }

    #[test]
    fn json_output_includes_route_leg_debug_paths() {
        let value: Value = serde_json::from_str(&render_route(&route()).unwrap()).unwrap();

        assert_eq!(
            value["legs"][0]["path_system_ids"],
            json!([30_000_001, 30_000_003, 30_000_002])
        );
        assert_eq!(
            value["legs"][0]["path_system_names"],
            json!(["Start", "Middle", "Waypoint"])
        );
    }

    #[test]
    fn zero_waypoint_partial_route_serializes_cleanly() {
        let mut route = route();
        route.total_jumps = 0;
        route.average_score = 0.0;
        route.waypoints.clear();
        route.legs.clear();

        let rendered = render_route(&route).expect("zero-waypoint route should render");
        let value: Value = serde_json::from_str(&rendered).unwrap();

        assert_eq!(value["waypoints"], json!([]));
        assert_eq!(value["legs"], json!([]));
        assert!(rendered.contains('\n'));
    }
}
