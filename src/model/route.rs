use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use clap::ValueEnum;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::model::score::ScoreBreakdown;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum RouteMode {
    UltraQuiet,
    DenseQuiet,
    Sweep,
}

impl RouteMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UltraQuiet => "ultra_quiet",
            Self::DenseQuiet => "dense_quiet",
            Self::Sweep => "sweep",
        }
    }
}

impl Default for RouteMode {
    fn default() -> Self {
        Self::DenseQuiet
    }
}

impl fmt::Display for RouteMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RouteMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ultra_quiet" | "ultra-quiet" => Ok(Self::UltraQuiet),
            "dense_quiet" | "dense-quiet" => Ok(Self::DenseQuiet),
            "sweep" => Ok(Self::Sweep),
            other => Err(format!("unsupported route mode '{other}'")),
        }
    }
}

impl Serialize for RouteMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RouteMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GeneratedRoute {
    pub start_system: String,
    pub start_system_id: i32,
    pub mode: RouteMode,
    pub highsec_only: bool,
    pub total_jumps: u32,
    pub average_score: f32,
    pub activity_timestamp: DateTime<Utc>,
    pub config_used: serde_json::Value,
    pub waypoints: Vec<RouteWaypoint>,
    pub legs: Vec<RouteLeg>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RouteWaypoint {
    pub order: usize,
    pub system_id: i32,
    pub system_name: String,
    pub security_status: f32,
    pub region_id: i32,
    pub constellation_id: i32,
    pub score: f32,
    pub jumps_last_hour: u32,
    pub npc_kills_last_hour: u32,
    pub ship_kills_last_hour: u32,
    pub pod_kills_last_hour: u32,
    pub distance_from_start: u32,
    pub score_breakdown: ScoreBreakdown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RouteLeg {
    pub from_system_id: i32,
    pub to_system_id: i32,
    pub jump_count: u32,
    pub path_system_ids: Vec<i32>,
    pub path_system_names: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Route {
    pub mode: RouteMode,
    pub system_names: Vec<String>,
    pub total_jumps: u32,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;

    use super::*;

    fn score_breakdown() -> ScoreBreakdown {
        ScoreBreakdown {
            activity: 0.2,
            distance: 0.3,
            security: 0.4,
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

    #[test]
    fn route_mode_serializes_and_deserializes_as_expected_strings() {
        let cases = [
            (RouteMode::UltraQuiet, "ultra_quiet"),
            (RouteMode::DenseQuiet, "dense_quiet"),
            (RouteMode::Sweep, "sweep"),
        ];

        for (mode, expected_json_string) in cases {
            let serialized = serde_json::to_string(&mode).expect("route mode should serialize");
            assert_eq!(serialized, format!("\"{expected_json_string}\""));

            let deserialized: RouteMode =
                serde_json::from_str(&serialized).expect("route mode should deserialize");
            assert_eq!(deserialized, mode);
        }
    }

    #[test]
    fn generated_route_json_contains_score_breakdown() {
        let route = GeneratedRoute {
            start_system: "Jita".to_string(),
            start_system_id: 30_000_142,
            mode: RouteMode::DenseQuiet,
            highsec_only: true,
            total_jumps: 2,
            average_score: 0.9,
            activity_timestamp: Utc.with_ymd_and_hms(2026, 6, 7, 12, 0, 0).unwrap(),
            config_used: json!({"route": {"waypoint_count": 1}}),
            waypoints: vec![waypoint()],
            legs: vec![RouteLeg {
                from_system_id: 30_000_142,
                to_system_id: 30_000_141,
                jump_count: 1,
                path_system_ids: vec![30_000_142, 30_000_141],
                path_system_names: vec!["Jita".to_string(), "Perimeter".to_string()],
            }],
        };

        let value = serde_json::to_value(route).expect("generated route should serialize");

        assert_eq!(value["mode"], json!("dense_quiet"));
        let breakdown = &value["waypoints"][0]["score_breakdown"];
        assert!(breakdown.is_object());
        assert!((breakdown["activity"].as_f64().unwrap() - 0.2).abs() < 0.000_001);
        assert!((breakdown["distance"].as_f64().unwrap() - 0.3).abs() < 0.000_001);
        assert!((breakdown["security"].as_f64().unwrap() - 0.4).abs() < 0.000_001);
        assert!((breakdown["total"].as_f64().unwrap() - 0.9).abs() < 0.000_001);
    }

    #[test]
    fn route_waypoint_includes_system_id_security_region_and_constellation() {
        let value = serde_json::to_value(waypoint()).expect("waypoint should serialize");

        assert_eq!(value["system_id"], json!(30_000_142));
        assert!((value["security_status"].as_f64().unwrap() - 0.946).abs() < 0.000_001);
        assert_eq!(value["region_id"], json!(10_000_002));
        assert_eq!(value["constellation_id"], json!(20_000_020));
    }
}
