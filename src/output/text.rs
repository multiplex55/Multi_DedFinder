use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use anyhow::{bail, Result};

use crate::config::AppConfig;
use crate::model::route::{GeneratedRoute, RouteWaypoint};
use crate::model::score::ScoreBreakdown;

const TITLE: &str = "Quiet High-Sec DED Route";
const FORBIDDEN_OUTPUT_CLAIMS: [&str; 3] =
    ["scan anomalies", "scan signatures", "scanned signatures"];

pub fn print_generation_summary(config: &AppConfig) -> Result<()> {
    writeln!(
        io::stdout(),
        "Generating {} {} waypoints from {}",
        config.route.waypoint_count,
        config.route.mode,
        config.start.system.as_deref().unwrap_or("configured start")
    )?;
    Ok(())
}

pub fn render_route(route: &GeneratedRoute) -> Result<String> {
    let mut output = String::new();

    writeln!(output, "{TITLE}")?;
    writeln!(output, "========================")?;
    writeln!(
        output,
        "Start system: {} ({})",
        route.start_system, route.start_system_id
    )?;
    writeln!(output, "Mode: {}", route.mode)?;
    writeln!(output, "High-sec only: {}", route.highsec_only)?;
    writeln!(output, "Waypoint count: {}", route.waypoints.len())?;
    writeln!(output, "Total route jumps: {}", route.total_jumps)?;
    writeln!(
        output,
        "Activity data timestamp: {}",
        route.activity_timestamp.to_rfc3339()
    )?;
    writeln!(
        output,
        "Config summary: {}",
        config_summary(&route.config_used)
    )?;

    if route.waypoints.is_empty() {
        writeln!(output)?;
        writeln!(output, "Waypoints: none")?;
    } else {
        writeln!(output)?;
        writeln!(output, "Waypoints")?;
        writeln!(output, "---------")?;
        for waypoint in &route.waypoints {
            render_waypoint(&mut output, waypoint)?;
        }
    }

    validate_route_output_claims(&output)?;
    Ok(output)
}

pub fn render_routes(routes: &[GeneratedRoute]) -> Result<String> {
    let mut output = String::new();
    writeln!(output, "{TITLE} - all modes")?;
    writeln!(output, "============================")?;
    writeln!(output, "Routes generated: {}", routes.len())?;
    writeln!(
        output,
        "Data sources: public ESI activity data and static/local SDE data only; no live anomaly detection, probe scanner parsing, UI automation, in-client clicking, or EVE client process interaction."
    )?;

    for route in routes {
        writeln!(output)?;
        writeln!(output, "{} summary", route.mode)?;
        writeln!(output, "{}", "-".repeat(route.mode.to_string().len() + 8))?;
        writeln!(
            output,
            "Start system: {} ({})",
            route.start_system, route.start_system_id
        )?;
        writeln!(output, "Waypoint count: {}", route.waypoints.len())?;
        writeln!(output, "Total route jumps: {}", route.total_jumps)?;
        writeln!(output, "Average score: {:.4}", route.average_score)?;
        let names = route
            .waypoints
            .iter()
            .map(|waypoint| waypoint.system_name.as_str())
            .collect::<Vec<_>>()
            .join(" -> ");
        writeln!(
            output,
            "Waypoints: {}",
            if names.is_empty() { "none" } else { &names }
        )?;
    }

    validate_route_output_claims(&output)?;
    Ok(output)
}

pub fn write_routes(routes: &[GeneratedRoute], path: impl AsRef<Path>) -> Result<()> {
    let rendered = render_routes(routes)?;
    fs::write(path, rendered)?;
    Ok(())
}

pub fn print_routes(routes: &[GeneratedRoute]) -> Result<()> {
    let rendered = render_routes(routes)?;
    print!("{rendered}");
    Ok(())
}

pub fn write_route(route: &GeneratedRoute, path: impl AsRef<Path>) -> Result<()> {
    let rendered = render_route(route)?;
    fs::write(path, rendered)?;
    Ok(())
}

pub fn print_route(route: &GeneratedRoute) -> Result<()> {
    let rendered = render_route(route)?;
    print!("{rendered}");
    Ok(())
}

pub fn validate_route_output_claims(rendered: &str) -> Result<()> {
    let lowered = rendered.to_lowercase();
    for forbidden_claim in FORBIDDEN_OUTPUT_CLAIMS {
        if lowered.contains(forbidden_claim) {
            bail!("route output must not claim to scan anomalies or signatures");
        }
    }
    Ok(())
}

fn render_waypoint(output: &mut String, waypoint: &RouteWaypoint) -> Result<()> {
    writeln!(
        output,
        "{}. {} ({})",
        waypoint.order, waypoint.system_name, waypoint.system_id
    )?;
    writeln!(
        output,
        "   Security status: {:.3}",
        waypoint.security_status
    )?;
    writeln!(output, "   Region ID: {}", waypoint.region_id)?;
    writeln!(output, "   Constellation ID: {}", waypoint.constellation_id)?;
    writeln!(output, "   Score: {:.4}", waypoint.score)?;
    writeln!(output, "   Jumps last hour: {}", waypoint.jumps_last_hour)?;
    writeln!(
        output,
        "   NPC kills last hour: {}",
        waypoint.npc_kills_last_hour
    )?;
    writeln!(
        output,
        "   Ship kills last hour: {}",
        waypoint.ship_kills_last_hour
    )?;
    writeln!(
        output,
        "   Pod kills last hour: {}",
        waypoint.pod_kills_last_hour
    )?;
    writeln!(
        output,
        "   Distance from start: {} jumps",
        waypoint.distance_from_start
    )?;
    writeln!(
        output,
        "   Score breakdown: {}",
        format_score_breakdown(&waypoint.score_breakdown)
    )?;
    Ok(())
}

fn format_score_breakdown(score: &ScoreBreakdown) -> String {
    format!(
        "activity={:.4}, distance={:.4}, security={:.4}, jump_score={:.4}, npc_score={:.4}, danger_score={:.4}, cluster_density_score={:.4}, hub_distance_score={:.4}, dead_end_penalty={:.4}, reuse_penalty={:.4}, total={:.4}",
        score.activity,
        score.distance,
        score.security,
        score.jump_score,
        score.npc_score,
        score.danger_score,
        score.cluster_density_score,
        score.hub_distance_score,
        score.dead_end_penalty,
        score.reuse_penalty,
        score.total
    )
}

fn config_summary(config_used: &serde_json::Value) -> String {
    let route = config_used.get("route");
    let filter = config_used.get("filter");
    let weights = config_used.get("weights");

    let configured_waypoints = route
        .and_then(|route| route.get("waypoint_count"))
        .and_then(serde_json::Value::as_u64)
        .map_or_else(|| "unknown".to_string(), |value| value.to_string());
    let max_distance = route
        .and_then(|route| route.get("max_distance"))
        .map_or_else(|| "unknown".to_string(), value_to_summary);
    let prefer_loop = route
        .and_then(|route| route.get("prefer_loop"))
        .and_then(serde_json::Value::as_bool)
        .map_or_else(|| "unknown".to_string(), |value| value.to_string());
    let min_security = filter
        .and_then(|filter| filter.get("min_security_status"))
        .map_or_else(|| "unknown".to_string(), value_to_summary);
    let weight_summary = weights.map_or_else(|| "unknown".to_string(), value_to_summary);

    format!(
        "configured_waypoints={configured_waypoints}, max_distance={max_distance}, prefer_loop={prefer_loop}, min_security_status={min_security}, weights={weight_summary}"
    )
}

fn value_to_summary(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "none".to_string(),
        serde_json::Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    use super::*;
    use crate::model::route::{GeneratedRoute, RouteLeg, RouteMode};

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
            config_used: json!({
                "route": {"waypoint_count": 1, "max_distance": 10, "prefer_loop": true},
                "filter": {"min_security_status": 0.45},
                "weights": {"activity": 1.0, "distance": 1.0, "security": 1.0}
            }),
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
    fn text_output_contains_route_header() {
        let rendered = render_route(&route()).expect("route should render");

        assert!(rendered.contains("Quiet High-Sec DED Route"));
        assert!(rendered.contains("Start system: Start (30000001)"));
        assert!(rendered.contains("Mode: dense_quiet"));
        assert!(rendered.contains("Waypoint count: 1"));
    }

    #[test]
    fn text_output_includes_system_ids() {
        let rendered = render_route(&route()).expect("route should render");

        assert!(rendered.contains("30000001"));
        assert!(rendered.contains("Waypoint (30000002)"));
    }

    #[test]
    fn text_output_includes_score_breakdown() {
        let rendered = render_route(&route()).expect("route should render");

        assert!(rendered.contains("Score breakdown:"));
        assert!(rendered.contains("activity=0.2000"));
        assert!(rendered.contains("total=0.7500"));
    }

    #[test]
    fn text_output_does_not_claim_to_scan_anomalies_or_signatures() {
        let rendered = render_route(&route()).expect("route should render");

        assert!(!rendered.to_lowercase().contains("scan anomalies"));
        assert!(!rendered.to_lowercase().contains("scan signatures"));
        assert!(validate_route_output_claims("this would scan anomalies").is_err());
    }
}
