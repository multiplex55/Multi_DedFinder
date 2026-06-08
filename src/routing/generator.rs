use std::collections::HashSet;

use chrono::Utc;

use crate::config::AppConfig;
use crate::graph::highsec_graph::HighsecGraph;
use crate::model::route::{GeneratedRoute, RouteLeg, RouteMode, RouteWaypoint};
use crate::model::score::ScoredSystem;

const COVERAGE_RADIUS: u32 = 1;
const LOOP_BIAS_RADIUS: u32 = 2;

#[derive(Clone, Copy, Debug)]
struct GeneratorTuning {
    travel_cost_weight: f32,
    backtrack_weight: f32,
    covered_system_weight: f32,
    density_weight: f32,
    dead_end_weight: f32,
    loop_bias_weight: f32,
}

impl GeneratorTuning {
    const fn for_mode(mode: RouteMode) -> Self {
        match mode {
            RouteMode::UltraQuiet => Self {
                travel_cost_weight: 0.018,
                backtrack_weight: 0.025,
                covered_system_weight: 0.015,
                density_weight: 0.015,
                dead_end_weight: 0.010,
                loop_bias_weight: 0.020,
            },
            RouteMode::DenseQuiet => Self {
                travel_cost_weight: 0.040,
                backtrack_weight: 0.060,
                covered_system_weight: 0.045,
                density_weight: 0.080,
                dead_end_weight: 0.100,
                loop_bias_weight: 0.035,
            },
            RouteMode::Sweep => Self {
                travel_cost_weight: 0.030,
                backtrack_weight: 0.180,
                covered_system_weight: 0.140,
                density_weight: 0.035,
                dead_end_weight: 0.180,
                loop_bias_weight: 0.050,
            },
        }
    }
}

#[derive(Clone, Debug)]
struct CandidateEvaluation<'a> {
    candidate: &'a ScoredSystem,
    path: Vec<i32>,
    next_value: f32,
}

struct EvaluationContext<'a> {
    graph: &'a HighsecGraph,
    current_system_id: i32,
    start_system_id: i32,
    used_transit_systems: &'a HashSet<i32>,
    covered_systems: &'a HashSet<i32>,
    tuning: GeneratorTuning,
    prefer_loop: bool,
    selected_count: usize,
    waypoint_count: usize,
}

/// Generates a high-sec-only waypoint route from an already scored candidate pool.
///
/// The generator greedily evaluates each remaining candidate from the current route
/// position using:
///
/// `next_value = candidate_score - travel_cost_penalty - backtrack_penalty`
///
/// Mode-specific tuning controls how heavily travel distance, reused transit,
/// local coverage, density, dead ends, and loop friendliness affect that value.
/// Pathfinding is always performed against `HighsecGraph`, so low-sec systems or
/// systems absent from the high-sec graph can never appear in route legs.
#[must_use]
pub fn generate_route(
    graph: &HighsecGraph,
    start_system_id: i32,
    candidates: &[ScoredSystem],
    config: &AppConfig,
) -> GeneratedRoute {
    let mode = config.route.mode;
    let tuning = GeneratorTuning::for_mode(mode);
    let start_system = graph
        .systems
        .get(&start_system_id)
        .map(|system| system.name.clone())
        .unwrap_or_else(|| start_system_id.to_string());

    let mut route = GeneratedRoute {
        start_system,
        start_system_id,
        mode,
        highsec_only: true,
        total_jumps: 0,
        average_score: 0.0,
        activity_timestamp: Utc::now(),
        config_used: serde_json::to_value(config).unwrap_or(serde_json::Value::Null),
        waypoints: Vec::new(),
        legs: Vec::new(),
    };

    if config.route.waypoint_count == 0 || !graph.contains_system(start_system_id) {
        return route;
    }

    let mut current_system_id = start_system_id;
    let mut selected_waypoints = HashSet::new();
    let mut used_transit_systems = HashSet::from([start_system_id]);
    let mut covered_systems = HashSet::new();

    while route.waypoints.len() < config.route.waypoint_count {
        let Some(best) = candidates
            .iter()
            .filter(|candidate| !selected_waypoints.contains(&candidate.system_id))
            .filter(|candidate| graph.contains_system(candidate.system_id))
            .filter_map(|candidate| {
                let context = EvaluationContext {
                    graph,
                    current_system_id,
                    start_system_id,
                    used_transit_systems: &used_transit_systems,
                    covered_systems: &covered_systems,
                    tuning,
                    prefer_loop: config.route.prefer_loop,
                    selected_count: route.waypoints.len(),
                    waypoint_count: config.route.waypoint_count,
                };
                evaluate_candidate(candidate, &context)
            })
            .max_by(|left, right| {
                left.next_value
                    .total_cmp(&right.next_value)
                    .then_with(|| right.path.len().cmp(&left.path.len()))
                    .then_with(|| left.candidate.score.total_cmp(&right.candidate.score))
                    .then_with(|| right.candidate.system_id.cmp(&left.candidate.system_id))
            })
        else {
            break;
        };

        let waypoint = waypoint_from_candidate(
            best.candidate,
            route.waypoints.len() + 1,
            graph
                .jump_distance(start_system_id, best.candidate.system_id)
                .unwrap_or(best.candidate.distance_from_start),
        );
        let leg = route_leg(
            graph,
            current_system_id,
            best.candidate.system_id,
            best.path,
        );

        selected_waypoints.insert(best.candidate.system_id);
        for system_id in &leg.path_system_ids {
            used_transit_systems.insert(*system_id);
        }
        covered_systems
            .extend(graph.systems_within_jumps(best.candidate.system_id, COVERAGE_RADIUS));

        current_system_id = best.candidate.system_id;
        route.legs.push(leg);
        route.waypoints.push(waypoint);
    }

    if config.route.prefer_loop && current_system_id != start_system_id {
        if let Some(path) = graph.shortest_path_highsec_only(current_system_id, start_system_id) {
            route
                .legs
                .push(route_leg(graph, current_system_id, start_system_id, path));
        }
    }

    route.total_jumps = route.legs.iter().map(|leg| leg.jump_count).sum();
    route.average_score = if route.waypoints.is_empty() {
        0.0
    } else {
        route
            .waypoints
            .iter()
            .map(|waypoint| waypoint.score)
            .sum::<f32>()
            / route.waypoints.len() as f32
    };

    route
}

/// Generates one route for each supported route mode from the same candidate pool.
#[must_use]
pub fn generate_all_modes(
    graph: &HighsecGraph,
    start_system_id: i32,
    candidates: &[ScoredSystem],
    config: &AppConfig,
) -> Vec<GeneratedRoute> {
    [
        RouteMode::UltraQuiet,
        RouteMode::DenseQuiet,
        RouteMode::Sweep,
    ]
    .into_iter()
    .map(|mode| {
        let mut mode_config = config.clone();
        mode_config.route.mode = mode;
        generate_route(graph, start_system_id, candidates, &mode_config)
    })
    .collect()
}

fn evaluate_candidate<'a>(
    candidate: &'a ScoredSystem,
    context: &EvaluationContext<'_>,
) -> Option<CandidateEvaluation<'a>> {
    let path = context
        .graph
        .shortest_path_highsec_only(context.current_system_id, candidate.system_id)?;
    if !path
        .iter()
        .all(|system_id| context.graph.contains_system(*system_id))
    {
        return None;
    }

    let jumps = path.len().saturating_sub(1) as f32;
    let reused_systems = path
        .iter()
        .skip(1)
        .filter(|system_id| context.used_transit_systems.contains(system_id))
        .count() as f32;
    let travel_cost_penalty = jumps * context.tuning.travel_cost_weight;
    let backtrack_penalty = reused_systems * context.tuning.backtrack_weight;
    let covered_penalty = if context.covered_systems.contains(&candidate.system_id) {
        context.tuning.covered_system_weight
    } else {
        0.0
    };
    let density_bonus = context
        .graph
        .highsec_density(candidate.system_id, COVERAGE_RADIUS)
        * context.tuning.density_weight;
    let dead_end_penalty = if context.graph.neighbor_count(candidate.system_id) <= 1 {
        context.tuning.dead_end_weight
    } else {
        0.0
    };
    let loop_bonus = if context.prefer_loop
        && is_late_route_selection(context.selected_count, context.waypoint_count)
    {
        context
            .graph
            .jump_distance(candidate.system_id, context.start_system_id)
            .map(|distance| {
                let efficient_return_score =
                    1.0 / (1.0 + distance.saturating_sub(LOOP_BIAS_RADIUS) as f32);
                efficient_return_score * context.tuning.loop_bias_weight
            })
            .unwrap_or(0.0)
    } else {
        0.0
    };
    let next_value = candidate.score - travel_cost_penalty - backtrack_penalty - covered_penalty
        + density_bonus
        - dead_end_penalty
        + loop_bonus;

    Some(CandidateEvaluation {
        candidate,
        path,
        next_value,
    })
}

fn is_late_route_selection(selected_count: usize, waypoint_count: usize) -> bool {
    selected_count + 1 >= waypoint_count.saturating_sub(1).max(1)
}

fn waypoint_from_candidate(
    candidate: &ScoredSystem,
    order: usize,
    distance_from_start: u32,
) -> RouteWaypoint {
    RouteWaypoint {
        order,
        system_id: candidate.system_id,
        system_name: candidate.system_name.clone(),
        security_status: candidate.security_status,
        region_id: candidate.region_id,
        constellation_id: candidate.constellation_id,
        score: candidate.score,
        jumps_last_hour: candidate.jumps_last_hour,
        npc_kills_last_hour: candidate.npc_kills_last_hour,
        ship_kills_last_hour: candidate.ship_kills_last_hour,
        pod_kills_last_hour: candidate.pod_kills_last_hour,
        distance_from_start,
        score_breakdown: candidate.score_breakdown,
    }
}

fn route_leg(
    graph: &HighsecGraph,
    from_system_id: i32,
    to_system_id: i32,
    path: Vec<i32>,
) -> RouteLeg {
    RouteLeg {
        from_system_id,
        to_system_id,
        jump_count: path.len().saturating_sub(1) as u32,
        path_system_names: path
            .iter()
            .map(|system_id| {
                graph
                    .systems
                    .get(system_id)
                    .map(|system| system.name.clone())
                    .unwrap_or_else(|| system_id.to_string())
            })
            .collect(),
        path_system_ids: path,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::config::AppConfig;
    use crate::graph::highsec_graph::{build_highsec_graph, DEFAULT_HIGHSEC_SECURITY_CUTOFF};
    use crate::model::route::RouteMode;
    use crate::model::score::{ScoreBreakdown, ScoredSystem};
    use crate::model::system::{SolarSystem, StargateConnection};

    use super::*;

    fn system(id: i32, security_status: f32) -> SolarSystem {
        SolarSystem {
            id,
            name: format!("System {id}"),
            security_status,
            region_id: 1,
            constellation_id: 1,
        }
    }

    fn gate(from_system_id: i32, to_system_id: i32) -> StargateConnection {
        StargateConnection {
            from_system_id,
            to_system_id,
        }
    }

    fn score_breakdown(
        score: f32,
        cluster_density_score: f32,
        dead_end_penalty: f32,
    ) -> ScoreBreakdown {
        ScoreBreakdown {
            activity: score,
            distance: 0.0,
            security: 0.7,
            jump_score: score,
            npc_score: score,
            danger_score: score,
            cluster_density_score,
            hub_distance_score: 0.0,
            dead_end_penalty,
            reuse_penalty: 0.0,
            faction_space_bonus: 0.0,
            total: score,
        }
    }

    fn candidate(system_id: i32, score: f32) -> ScoredSystem {
        ScoredSystem {
            system_id,
            system_name: format!("System {system_id}"),
            security_status: 0.7,
            region_id: 1,
            constellation_id: 1,
            score,
            jumps_last_hour: 0,
            npc_kills_last_hour: 0,
            ship_kills_last_hour: 0,
            pod_kills_last_hour: 0,
            distance_from_start: 0,
            score_breakdown: score_breakdown(score, 0.0, 0.0),
        }
    }

    fn config(mode: RouteMode, waypoint_count: usize) -> AppConfig {
        let mut config = AppConfig::default();
        config.route.mode = mode;
        config.route.waypoint_count = waypoint_count;
        config.route.prefer_loop = false;
        config
    }

    fn waypoint_ids(route: &GeneratedRoute) -> Vec<i32> {
        route
            .waypoints
            .iter()
            .map(|waypoint| waypoint.system_id)
            .collect()
    }

    #[test]
    fn route_generator_does_not_include_duplicate_waypoints() {
        let graph = build_highsec_graph(
            vec![system(1, 0.9), system(2, 0.9), system(3, 0.9)],
            vec![gate(1, 2), gate(2, 3)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );
        let candidates = vec![candidate(2, 0.9), candidate(2, 0.95), candidate(3, 0.8)];

        let route = generate_route(&graph, 1, &candidates, &config(RouteMode::DenseQuiet, 3));

        let ids = waypoint_ids(&route);
        let unique_ids: HashSet<_> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique_ids.len());
    }

    #[test]
    fn candidate_pool_exhaustion_returns_partial_route_cleanly() {
        let graph = build_highsec_graph(
            vec![system(1, 0.9), system(2, 0.9)],
            vec![gate(1, 2)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        let route = generate_route(
            &graph,
            1,
            &[candidate(2, 0.9)],
            &config(RouteMode::DenseQuiet, 4),
        );

        assert_eq!(waypoint_ids(&route), vec![2]);
        assert_eq!(route.legs.len(), 1);
    }

    #[test]
    fn route_generator_never_pathfinds_through_low_sec() {
        let graph = build_highsec_graph(
            vec![
                system(1, 0.9),
                system(2, 0.1),
                system(3, 0.9),
                system(4, 0.9),
                system(5, 0.9),
            ],
            vec![gate(1, 2), gate(2, 3), gate(1, 4), gate(4, 5), gate(5, 3)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        let route = generate_route(
            &graph,
            1,
            &[candidate(3, 0.9)],
            &config(RouteMode::DenseQuiet, 1),
        );

        assert_eq!(route.legs[0].path_system_ids, vec![1, 4, 5, 3]);
        assert!(route.legs[0]
            .path_system_ids
            .iter()
            .all(|system_id| graph.contains_system(*system_id)));
    }

    #[test]
    fn route_debug_path_ids_never_include_graph_excluded_avoided_region() {
        let blocked_region_id = 20;
        let graph = build_highsec_graph(
            vec![
                SolarSystem {
                    region_id: 10,
                    ..system(1, 0.9)
                },
                SolarSystem {
                    region_id: blocked_region_id,
                    ..system(2, 0.9)
                },
                SolarSystem {
                    region_id: 30,
                    ..system(3, 0.9)
                },
                SolarSystem {
                    region_id: 30,
                    ..system(4, 0.9)
                },
            ]
            .into_iter()
            .filter(|system| system.region_id != blocked_region_id),
            vec![gate(1, 2), gate(2, 4), gate(1, 3), gate(3, 4)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );
        let route = generate_route(
            &graph,
            1,
            &[candidate(4, 0.9)],
            &config(RouteMode::DenseQuiet, 1),
        );

        assert_eq!(route.legs[0].path_system_ids, vec![1, 3, 4]);
        assert!(!route.legs[0].path_system_ids.contains(&2));
        assert!(route.legs[0].path_system_ids.iter().all(|system_id| {
            graph
                .systems
                .get(system_id)
                .is_some_and(|system| system.region_id != blocked_region_id)
        }));
    }

    #[test]
    fn total_jumps_equals_sum_of_route_leg_distances() {
        let graph = build_highsec_graph(
            vec![
                system(1, 0.9),
                system(2, 0.9),
                system(3, 0.9),
                system(4, 0.9),
            ],
            vec![gate(1, 2), gate(2, 3), gate(3, 4)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );
        let route = generate_route(
            &graph,
            1,
            &[candidate(2, 0.9), candidate(4, 0.8)],
            &config(RouteMode::UltraQuiet, 2),
        );

        assert_eq!(
            route.total_jumps,
            route.legs.iter().map(|leg| leg.jump_count).sum::<u32>()
        );
    }

    #[test]
    fn route_starts_from_configured_start_system() {
        let graph = build_highsec_graph(
            vec![system(1, 0.9), system(2, 0.9), system(3, 0.9)],
            vec![gate(1, 2), gate(2, 3)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        let route = generate_route(
            &graph,
            2,
            &[candidate(3, 0.9)],
            &config(RouteMode::DenseQuiet, 1),
        );

        assert_eq!(route.start_system_id, 2);
        assert_eq!(route.start_system, "System 2");
        assert_eq!(route.legs[0].from_system_id, 2);
    }

    #[test]
    fn unreachable_candidates_are_ignored() {
        let graph = build_highsec_graph(
            vec![system(1, 0.9), system(2, 0.9), system(3, 0.9)],
            vec![gate(1, 2)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        let route = generate_route(
            &graph,
            1,
            &[candidate(3, 1.0), candidate(2, 0.7)],
            &config(RouteMode::DenseQuiet, 2),
        );

        assert_eq!(waypoint_ids(&route), vec![2]);
    }

    #[test]
    fn dense_quiet_chooses_dense_cluster_over_isolated_quiet_dead_end() {
        let graph = build_highsec_graph(
            vec![
                system(1, 0.9),
                system(2, 0.9),
                system(3, 0.9),
                system(4, 0.9),
                system(5, 0.9),
                system(6, 0.9),
                system(7, 0.9),
            ],
            vec![
                gate(1, 2),
                gate(2, 3),
                gate(3, 4),
                gate(2, 4),
                gate(1, 5),
                gate(5, 6),
                gate(6, 7),
            ],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        let route = generate_route(
            &graph,
            1,
            &[candidate(7, 0.86), candidate(3, 0.82)],
            &config(RouteMode::DenseQuiet, 1),
        );

        assert_eq!(waypoint_ids(&route), vec![3]);
    }

    #[test]
    fn ultra_quiet_tolerates_less_efficient_travel_for_quieter_systems() {
        let graph = build_highsec_graph(
            vec![
                system(1, 0.9),
                system(2, 0.9),
                system(3, 0.9),
                system(4, 0.9),
                system(5, 0.9),
            ],
            vec![gate(1, 2), gate(1, 3), gate(3, 4), gate(4, 5)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        let route = generate_route(
            &graph,
            1,
            &[candidate(2, 0.78), candidate(5, 0.84)],
            &config(RouteMode::UltraQuiet, 1),
        );

        assert_eq!(waypoint_ids(&route), vec![5]);
    }

    #[test]
    fn sweep_covers_more_systems_with_less_backtracking() {
        let graph = build_highsec_graph(
            vec![
                system(1, 0.9),
                system(2, 0.9),
                system(3, 0.9),
                system(4, 0.9),
                system(5, 0.9),
                system(6, 0.9),
                system(7, 0.9),
            ],
            vec![
                gate(1, 2),
                gate(2, 3),
                gate(3, 4),
                gate(4, 5),
                gate(3, 6),
                gate(6, 7),
            ],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        let route = generate_route(
            &graph,
            1,
            &[candidate(4, 0.90), candidate(5, 0.89), candidate(7, 0.88)],
            &config(RouteMode::Sweep, 2),
        );

        assert_eq!(waypoint_ids(&route), vec![4, 5]);
        assert_eq!(route.legs[1].path_system_ids, vec![4, 5]);
    }

    #[test]
    fn generate_all_modes_returns_ultra_quiet_dense_quiet_and_sweep() {
        let graph = build_highsec_graph(
            vec![system(1, 0.9), system(2, 0.9)],
            vec![gate(1, 2)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        let routes = generate_all_modes(
            &graph,
            1,
            &[candidate(2, 0.9)],
            &config(RouteMode::DenseQuiet, 1),
        );

        assert_eq!(
            routes.iter().map(|route| route.mode).collect::<Vec<_>>(),
            vec![
                RouteMode::UltraQuiet,
                RouteMode::DenseQuiet,
                RouteMode::Sweep
            ]
        );
    }
}
