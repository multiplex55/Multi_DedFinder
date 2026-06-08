use std::collections::HashSet;

use crate::config::{AppConfig, WeightConfig};
use crate::data::esi_activity::SystemActivity;
use crate::graph::highsec_graph::HighsecGraph;
use crate::model::route::RouteMode;
use crate::model::score::{RouteScore, ScoreBreakdown, ScoredSystem};
use crate::model::system::SolarSystem;
use crate::routing::route_modes::RouteModeScoring;

const JUMP_CAP: u32 = 80;
const NPC_KILL_CAP: u32 = 250;
const DANGER_CAP: u32 = 25;
const HUB_DISTANCE_CAP: u32 = 12;
const CLUSTER_RADIUS: u32 = 1;
const CLUSTER_DENSITY_CAP_PERCENT: u32 = 35;

const TRADE_HUB_SYSTEM_IDS: [i32; 5] = [30_000_142, 30_002_187, 30_002_653, 30_001_962, 30_002_010];

pub fn weighted_score(score: RouteScore, weights: &WeightConfig) -> f32 {
    (score.activity * weights.activity)
        + (score.distance * weights.distance)
        + (score.security * weights.security)
}

pub fn inverse_score(value: u32, cap: u32) -> f64 {
    if cap == 0 {
        return 0.0;
    }

    let clamped = value.min(cap) as f64;
    1.0 - (clamped / cap as f64)
}

pub fn score_system(
    system: &SolarSystem,
    activity: &SystemActivity,
    graph: &HighsecGraph,
    config: &AppConfig,
    mode: RouteMode,
    route_history: &HashSet<i32>,
) -> ScoredSystem {
    let tuning = RouteModeScoring::for_mode(mode);
    let jump_score = inverse_score(activity.jumps_last_hour, JUMP_CAP);
    let npc_score = inverse_score(activity.npc_kills_last_hour, NPC_KILL_CAP);
    let danger_score = danger_score(activity.ship_kills_last_hour, activity.pod_kills_last_hour);
    let cluster_density_score = cluster_density_score(system.id, graph);
    let hub_distance_score = hub_distance_score(system.id, graph, config.route.trade_hub_radius);
    let dead_end_penalty = dead_end_penalty(system.id, graph);
    let reuse_penalty = reuse_penalty(system.id, route_history);

    let positive_total = (jump_score * tuning.jump_weight)
        + (npc_score * tuning.npc_weight)
        + (danger_score * tuning.danger_weight)
        + (cluster_density_score * tuning.cluster_density_weight)
        + (hub_distance_score * tuning.hub_distance_weight);

    let normalized_positive = if tuning.positive_weight_total() == 0.0 {
        0.0
    } else {
        positive_total / tuning.positive_weight_total()
    };

    let penalty_total = (dead_end_penalty * tuning.dead_end_penalty_weight)
        + (reuse_penalty * tuning.reuse_penalty_weight);
    let penalty_weight_total = tuning.dead_end_penalty_weight + tuning.reuse_penalty_weight;
    let normalized_penalty = if penalty_weight_total == 0.0 {
        0.0
    } else {
        penalty_total / penalty_weight_total
    };

    let score = (normalized_positive - normalized_penalty).clamp(0.0, 1.0) as f32;
    let distance_from_start = nearest_hub_distance(system.id, graph).unwrap_or(HUB_DISTANCE_CAP);
    let score_breakdown = ScoreBreakdown {
        activity: ((jump_score + npc_score + danger_score) / 3.0) as f32,
        distance: hub_distance_score as f32,
        security: system.security_status.clamp(0.0, 1.0),
        jump_score: jump_score as f32,
        npc_score: npc_score as f32,
        danger_score: danger_score as f32,
        cluster_density_score: cluster_density_score as f32,
        hub_distance_score: hub_distance_score as f32,
        dead_end_penalty: dead_end_penalty as f32,
        reuse_penalty: reuse_penalty as f32,
        faction_space_bonus: 0.0,
        total: score,
    };

    ScoredSystem {
        system_id: system.id,
        system_name: system.name.clone(),
        security_status: system.security_status,
        region_id: system.region_id,
        constellation_id: system.constellation_id,
        score,
        jumps_last_hour: activity.jumps_last_hour,
        npc_kills_last_hour: activity.npc_kills_last_hour,
        ship_kills_last_hour: activity.ship_kills_last_hour,
        pod_kills_last_hour: activity.pod_kills_last_hour,
        distance_from_start,
        score_breakdown,
    }
}

fn danger_score(ship_kills_last_hour: u32, pod_kills_last_hour: u32) -> f64 {
    let danger = ship_kills_last_hour.saturating_add(pod_kills_last_hour.saturating_mul(2));
    inverse_score(danger, DANGER_CAP)
}

fn cluster_density_score(system_id: i32, graph: &HighsecGraph) -> f64 {
    let density_percent = (graph.highsec_density(system_id, CLUSTER_RADIUS) * 100.0).round() as u32;
    (density_percent.min(CLUSTER_DENSITY_CAP_PERCENT) as f64 / CLUSTER_DENSITY_CAP_PERCENT as f64)
        .clamp(0.0, 1.0)
}

fn hub_distance_score(system_id: i32, graph: &HighsecGraph, trade_hub_radius: u32) -> f64 {
    let Some(distance) = nearest_hub_distance(system_id, graph) else {
        return 1.0;
    };

    let safe_distance = distance.saturating_sub(trade_hub_radius);
    1.0 - inverse_score(safe_distance, HUB_DISTANCE_CAP)
}

fn dead_end_penalty(system_id: i32, graph: &HighsecGraph) -> f64 {
    if graph.contains_system(system_id) && graph.neighbor_count(system_id) <= 1 {
        1.0
    } else {
        0.0
    }
}

fn reuse_penalty(system_id: i32, route_history: &HashSet<i32>) -> f64 {
    if route_history.contains(&system_id) {
        1.0
    } else {
        0.0
    }
}

fn nearest_hub_distance(system_id: i32, graph: &HighsecGraph) -> Option<u32> {
    TRADE_HUB_SYSTEM_IDS
        .iter()
        .filter_map(|hub_id| graph.jump_distance(system_id, *hub_id))
        .min()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::config::AppConfig;
    use crate::data::esi_activity::SystemActivity;
    use crate::graph::highsec_graph::build_highsec_graph;
    use crate::model::route::RouteMode;
    use crate::model::system::{SolarSystem, StargateConnection};

    use super::*;

    fn system(id: i32) -> SolarSystem {
        SolarSystem {
            id,
            name: format!("System {id}"),
            security_status: 0.7,
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

    fn activity(system_id: i32) -> SystemActivity {
        SystemActivity {
            system_id,
            jumps_last_hour: 10,
            npc_kills_last_hour: 10,
            ship_kills_last_hour: 0,
            pod_kills_last_hour: 0,
        }
    }

    fn test_graph() -> HighsecGraph {
        let ids = [30_000_142, 2, 3, 4, 5, 6, 7, 8];
        build_highsec_graph(
            ids.into_iter().map(system),
            vec![
                gate(30_000_142, 2),
                gate(2, 3),
                gate(3, 4),
                gate(4, 5),
                gate(5, 6),
                gate(6, 7),
                gate(4, 8),
                gate(5, 8),
            ],
            0.45,
        )
    }

    fn score_for(
        system_id: i32,
        activity: SystemActivity,
        mode: RouteMode,
        history: HashSet<i32>,
    ) -> ScoredSystem {
        let graph = test_graph();
        score_system(
            graph.systems.get(&system_id).unwrap(),
            &activity,
            &graph,
            &AppConfig::default(),
            mode,
            &history,
        )
    }

    #[test]
    fn inverse_scoring_favors_lower_jumps() {
        assert!(inverse_score(3, JUMP_CAP) > inverse_score(40, JUMP_CAP));
    }

    #[test]
    fn inverse_scoring_favors_lower_npc_kills() {
        assert!(inverse_score(5, NPC_KILL_CAP) > inverse_score(200, NPC_KILL_CAP));
    }

    #[test]
    fn inverse_scoring_handles_zero_cap_safely() {
        assert_eq!(inverse_score(10, 0), 0.0);
    }

    #[test]
    fn high_npc_kills_never_improve_score() {
        let low = score_for(
            5,
            SystemActivity {
                npc_kills_last_hour: 5,
                ..activity(5)
            },
            RouteMode::DenseQuiet,
            HashSet::new(),
        );
        let high = score_for(
            5,
            SystemActivity {
                npc_kills_last_hour: 240,
                ..activity(5)
            },
            RouteMode::DenseQuiet,
            HashSet::new(),
        );
        assert!(high.score <= low.score);
        assert!(high.score_breakdown.npc_score < low.score_breakdown.npc_score);
    }

    #[test]
    fn ship_kills_reduce_score() {
        let quiet = score_for(5, activity(5), RouteMode::DenseQuiet, HashSet::new());
        let dangerous = score_for(
            5,
            SystemActivity {
                ship_kills_last_hour: 10,
                ..activity(5)
            },
            RouteMode::DenseQuiet,
            HashSet::new(),
        );
        assert!(dangerous.score < quiet.score);
    }

    #[test]
    fn pod_kills_reduce_score() {
        let quiet = score_for(5, activity(5), RouteMode::DenseQuiet, HashSet::new());
        let dangerous = score_for(
            5,
            SystemActivity {
                pod_kills_last_hour: 6,
                ..activity(5)
            },
            RouteMode::DenseQuiet,
            HashSet::new(),
        );
        assert!(dangerous.score < quiet.score);
    }

    #[test]
    fn dead_end_penalty_applies_to_degree_one_systems() {
        let scored = score_for(7, activity(7), RouteMode::DenseQuiet, HashSet::new());
        assert_eq!(scored.score_breakdown.dead_end_penalty, 1.0);
    }

    #[test]
    fn dense_cluster_scores_higher_than_isolated_system_when_activity_is_similar() {
        let dense = score_for(5, activity(5), RouteMode::DenseQuiet, HashSet::new());
        let isolated = score_for(7, activity(7), RouteMode::DenseQuiet, HashSet::new());
        assert!(
            dense.score_breakdown.cluster_density_score
                > isolated.score_breakdown.cluster_density_score
        );
        assert!(dense.score > isolated.score);
    }

    #[test]
    fn hub_distance_score_improves_with_distance() {
        let near = score_for(2, activity(2), RouteMode::DenseQuiet, HashSet::new());
        let far = score_for(7, activity(7), RouteMode::DenseQuiet, HashSet::new());
        assert!(far.score_breakdown.hub_distance_score > near.score_breakdown.hub_distance_score);
    }

    #[test]
    fn route_reuse_penalty_applies_to_systems_in_history() {
        let unused = score_for(5, activity(5), RouteMode::DenseQuiet, HashSet::new());
        let reused = score_for(5, activity(5), RouteMode::DenseQuiet, HashSet::from([5]));
        assert_eq!(reused.score_breakdown.reuse_penalty, 1.0);
        assert!(reused.score < unused.score);
    }

    #[test]
    fn mode_specific_scoring_changes_ordering_for_ultra_quiet_dense_quiet_and_sweep() {
        let graph = test_graph();
        let quiet_remote = graph.systems.get(&7).unwrap();
        let dense_candidate = graph.systems.get(&4).unwrap();
        let sweep_efficient = graph.systems.get(&6).unwrap();
        let sweep_candidate = graph.systems.get(&7).unwrap();
        let quiet_remote_activity = SystemActivity {
            system_id: 7,
            jumps_last_hour: 2,
            npc_kills_last_hour: 2,
            ship_kills_last_hour: 0,
            pod_kills_last_hour: 0,
        };
        let ultra_noisy_candidate_activity = SystemActivity {
            system_id: 4,
            jumps_last_hour: 80,
            npc_kills_last_hour: 200,
            ship_kills_last_hour: 0,
            pod_kills_last_hour: 0,
        };
        let dense_candidate_activity = SystemActivity {
            system_id: 4,
            jumps_last_hour: 12,
            npc_kills_last_hour: 30,
            ship_kills_last_hour: 0,
            pod_kills_last_hour: 0,
        };
        let sweep_efficient_activity = SystemActivity {
            system_id: 6,
            jumps_last_hour: 2,
            npc_kills_last_hour: 2,
            ship_kills_last_hour: 0,
            pod_kills_last_hour: 0,
        };
        let sweep_candidate_activity = SystemActivity {
            system_id: 7,
            jumps_last_hour: 2,
            npc_kills_last_hour: 2,
            ship_kills_last_hour: 0,
            pod_kills_last_hour: 0,
        };
        let config = AppConfig::default();
        let history = HashSet::new();

        let ultra_quiet_remote = score_system(
            quiet_remote,
            &quiet_remote_activity,
            &graph,
            &config,
            RouteMode::UltraQuiet,
            &history,
        );
        let ultra_dense_candidate = score_system(
            dense_candidate,
            &ultra_noisy_candidate_activity,
            &graph,
            &config,
            RouteMode::UltraQuiet,
            &history,
        );
        let dense_quiet_remote = score_system(
            quiet_remote,
            &quiet_remote_activity,
            &graph,
            &config,
            RouteMode::DenseQuiet,
            &history,
        );
        let dense_cluster_candidate = score_system(
            dense_candidate,
            &dense_candidate_activity,
            &graph,
            &config,
            RouteMode::DenseQuiet,
            &history,
        );
        let sweep_remote = score_system(
            sweep_efficient,
            &sweep_efficient_activity,
            &graph,
            &config,
            RouteMode::Sweep,
            &history,
        );
        let sweep_dead_end = score_system(
            sweep_candidate,
            &sweep_candidate_activity,
            &graph,
            &config,
            RouteMode::Sweep,
            &history,
        );

        assert!(ultra_quiet_remote.score > ultra_dense_candidate.score);
        assert!(dense_cluster_candidate.score > dense_quiet_remote.score);
        assert!(sweep_remote.score > sweep_dead_end.score);
        assert_ne!(
            RouteModeScoring::for_mode(RouteMode::UltraQuiet),
            RouteModeScoring::for_mode(RouteMode::DenseQuiet)
        );
        assert_ne!(
            RouteModeScoring::for_mode(RouteMode::DenseQuiet),
            RouteModeScoring::for_mode(RouteMode::Sweep)
        );
    }
}
