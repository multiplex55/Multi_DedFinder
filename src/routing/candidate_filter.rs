use std::collections::{HashMap, HashSet};

use crate::config::{AppConfig, FactionExcludeBehavior, FactionSpaceBehavior, FilterBehavior};
use crate::data::esi_activity::SystemActivity;
use crate::graph::highsec_graph::HighsecGraph;
use crate::model::score::ScoredSystem;
use crate::routing::scorer::score_system;

const DEFAULT_TRADE_HUB_SOFT_PENALTY: f32 = 0.25;

/// Builds a scored candidate pool from systems that can be reached in the
/// high-sec graph from `start_system_id` and satisfy the configured filters.
///
/// The returned list is intentionally ordered by score only. Later route
/// generation stages should still account for travel shape and path quality
/// when selecting waypoints from this pool.
pub fn filter_candidates(
    graph: &HighsecGraph,
    start_system_id: i32,
    activity: &HashMap<i32, SystemActivity>,
    config: &AppConfig,
) -> Vec<ScoredSystem> {
    filter_candidates_with_route_history(graph, start_system_id, activity, config, &HashSet::new())
}

pub fn filter_candidates_with_route_history(
    graph: &HighsecGraph,
    start_system_id: i32,
    activity: &HashMap<i32, SystemActivity>,
    config: &AppConfig,
    route_history: &HashSet<i32>,
) -> Vec<ScoredSystem> {
    if !graph.contains_system(start_system_id) {
        return Vec::new();
    }

    let reachable = graph.reachable_systems_from(start_system_id);
    let avoid_system_ids = resolve_system_names(graph, &config.avoid.systems);
    let trade_hub_ids = resolve_system_names(graph, &config.filter.trade_hubs);
    let avoided_region_ids = config
        .avoid
        .region_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let preferred_faction_region_ids = config
        .faction_space
        .resolved_preferred_region_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let excluded_faction_region_ids = config
        .faction_space
        .resolved_excluded_candidate_only_region_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let max_distance = config
        .filter
        .max_distance_from_start
        .or(config.route.max_distance);

    let mut candidates: Vec<ScoredSystem> = graph
        .systems
        .values()
        .filter_map(|system| {
            if !reachable.contains(&system.id)
                || avoid_system_ids.contains(&system.id)
                || avoided_region_ids.contains(&system.region_id)
                || should_exclude_for_faction_include(
                    system.region_id,
                    config,
                    &preferred_faction_region_ids,
                )
                || should_exclude_for_faction_candidate_only(
                    system.region_id,
                    config,
                    &excluded_faction_region_ids,
                )
            {
                return None;
            }

            let distance_from_start = graph.jump_distance(start_system_id, system.id)?;
            if max_distance.is_some_and(|max_distance| distance_from_start > max_distance) {
                return None;
            }

            let system_activity = activity
                .get(&system.id)
                .cloned()
                .unwrap_or_else(|| empty_activity(system.id));

            if should_exclude_for_activity(
                &system_activity,
                config.filter.activity_behavior,
                config,
            ) {
                return None;
            }

            let hub_distance = nearest_distance_from_any(system.id, &trade_hub_ids, graph);
            if should_exclude_for_trade_hub(hub_distance, config) {
                return None;
            }

            let mut scored = score_system(
                system,
                &system_activity,
                graph,
                config,
                config.route.mode,
                route_history,
            );
            scored.distance_from_start = distance_from_start;

            if should_soft_penalize_trade_hub(hub_distance, config) {
                apply_trade_hub_soft_penalty(&mut scored, config.filter.trade_hub_soft_penalty);
            }

            if should_apply_faction_soft_bonus(
                system.region_id,
                config,
                &preferred_faction_region_ids,
            ) {
                apply_faction_space_soft_bonus(&mut scored, config.faction_space.soft_bonus);
            }

            Some(scored)
        })
        .collect();

    candidates.sort_by(|left, right| right.score.total_cmp(&left.score));
    candidates
}

fn should_exclude_for_faction_include(
    region_id: i32,
    config: &AppConfig,
    preferred_region_ids: &HashSet<i32>,
) -> bool {
    config.faction_space.behavior == FactionSpaceBehavior::HardInclude
        && !preferred_region_ids.contains(&region_id)
}

fn should_exclude_for_faction_candidate_only(
    region_id: i32,
    config: &AppConfig,
    excluded_region_ids: &HashSet<i32>,
) -> bool {
    config.faction_space.exclude_behavior == FactionExcludeBehavior::CandidateOnly
        && excluded_region_ids.contains(&region_id)
}

fn should_apply_faction_soft_bonus(
    region_id: i32,
    config: &AppConfig,
    preferred_region_ids: &HashSet<i32>,
) -> bool {
    config.faction_space.behavior == FactionSpaceBehavior::SoftBonus
        && preferred_region_ids.contains(&region_id)
}

fn apply_faction_space_soft_bonus(scored: &mut ScoredSystem, configured_bonus: f32) {
    let bonus = configured_bonus.max(0.0);
    scored.score = (scored.score + bonus).clamp(0.0, 1.0);
    scored.score_breakdown.faction_space_bonus = bonus;
    scored.score_breakdown.total = scored.score;
}

fn empty_activity(system_id: i32) -> SystemActivity {
    SystemActivity {
        system_id,
        jumps_last_hour: 0,
        npc_kills_last_hour: 0,
        ship_kills_last_hour: 0,
        pod_kills_last_hour: 0,
    }
}

fn resolve_system_names(graph: &HighsecGraph, names: &[String]) -> HashSet<i32> {
    names
        .iter()
        .filter_map(|name| resolve_system_name(graph, name))
        .collect()
}

fn resolve_system_name(graph: &HighsecGraph, name: &str) -> Option<i32> {
    graph
        .systems
        .values()
        .find(|system| system.name.eq_ignore_ascii_case(name))
        .map(|system| system.id)
}

fn should_exclude_for_activity(
    activity: &SystemActivity,
    behavior: FilterBehavior,
    config: &AppConfig,
) -> bool {
    behavior == FilterBehavior::HardExclude
        && (exceeds(activity.jumps_last_hour, config.filter.max_jumps_last_hour)
            || exceeds(
                activity.npc_kills_last_hour,
                config.filter.max_npc_kills_last_hour,
            )
            || exceeds(
                activity.ship_kills_last_hour,
                config.filter.max_ship_kills_last_hour,
            )
            || exceeds(
                activity.pod_kills_last_hour,
                config.filter.max_pod_kills_last_hour,
            ))
}

fn exceeds(value: u32, threshold: Option<u32>) -> bool {
    threshold.is_some_and(|threshold| value > threshold)
}

fn nearest_distance_from_any(
    system_id: i32,
    other_system_ids: &HashSet<i32>,
    graph: &HighsecGraph,
) -> Option<u32> {
    other_system_ids
        .iter()
        .filter_map(|other_system_id| graph.jump_distance(system_id, *other_system_id))
        .min()
}

fn should_exclude_for_trade_hub(hub_distance: Option<u32>, config: &AppConfig) -> bool {
    config.filter.trade_hub_behavior == FilterBehavior::HardExclude
        && within_configured_trade_hub_radius(hub_distance, config)
}

fn should_soft_penalize_trade_hub(hub_distance: Option<u32>, config: &AppConfig) -> bool {
    config.filter.trade_hub_behavior == FilterBehavior::SoftPenalty
        && within_configured_trade_hub_radius(hub_distance, config)
}

fn within_configured_trade_hub_radius(hub_distance: Option<u32>, config: &AppConfig) -> bool {
    hub_distance.is_some_and(|distance| distance <= config.route.trade_hub_radius)
}

fn apply_trade_hub_soft_penalty(scored: &mut ScoredSystem, configured_penalty: f32) {
    let penalty = if configured_penalty > 0.0 {
        configured_penalty
    } else {
        DEFAULT_TRADE_HUB_SOFT_PENALTY
    }
    .clamp(0.0, 1.0);
    let multiplier = 1.0 - penalty;

    scored.score = (scored.score * multiplier).clamp(0.0, 1.0);
    scored.score_breakdown.hub_distance_score =
        (scored.score_breakdown.hub_distance_score * multiplier).clamp(0.0, 1.0);
    scored.score_breakdown.distance =
        (scored.score_breakdown.distance * multiplier).clamp(0.0, 1.0);
    scored.score_breakdown.total = scored.score;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FactionExcludeBehavior, FactionSpaceBehavior, FilterConfig, RouteConfig};
    use crate::graph::highsec_graph::build_highsec_graph;
    use crate::model::system::{SolarSystem, StargateConnection};

    fn system(id: i32, name: &str) -> SolarSystem {
        SolarSystem {
            id,
            name: name.to_string(),
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

    fn graph() -> HighsecGraph {
        build_highsec_graph(
            vec![
                system(1, "Start"),
                system(2, "Near"),
                system(3, "Far"),
                system(4, "Jita"),
                system(5, "Perimeter"),
                system(6, "Unreachable"),
                system(7, "SoftHub"),
                system(8, "SoftNeighbor"),
                system(9, "Other"),
            ],
            vec![
                gate(1, 2),
                gate(2, 3),
                gate(3, 4),
                gate(4, 5),
                gate(1, 7),
                gate(7, 8),
                gate(8, 9),
            ],
            0.45,
        )
    }

    fn config() -> AppConfig {
        AppConfig {
            route: RouteConfig {
                trade_hub_radius: 1,
                ..RouteConfig::default()
            },
            filter: FilterConfig {
                trade_hubs: vec!["Jita".to_string()],
                trade_hub_behavior: FilterBehavior::Disabled,
                ..FilterConfig::default()
            },
            ..AppConfig::default()
        }
    }

    fn faction_region_graph() -> HighsecGraph {
        build_highsec_graph(
            vec![
                SolarSystem {
                    region_id: 10,
                    ..system(1, "Start")
                },
                SolarSystem {
                    region_id: 10,
                    ..system(2, "Preferred")
                },
                SolarSystem {
                    region_id: 20,
                    ..system(3, "Other")
                },
                SolarSystem {
                    region_id: 30,
                    ..system(4, "Excluded")
                },
            ],
            vec![gate(1, 2), gate(2, 3), gate(3, 4)],
            0.45,
        )
    }

    fn with_resolved_preferred_regions(mut config: AppConfig, region_ids: Vec<i32>) -> AppConfig {
        config.faction_space.resolved_preferred_region_ids = region_ids;
        config
    }

    fn with_resolved_candidate_only_excluded_regions(
        mut config: AppConfig,
        region_ids: Vec<i32>,
    ) -> AppConfig {
        config
            .faction_space
            .resolved_excluded_candidate_only_region_ids = region_ids;
        config
    }

    fn activity(system_id: i32) -> SystemActivity {
        SystemActivity {
            system_id,
            jumps_last_hour: 1,
            npc_kills_last_hour: 1,
            ship_kills_last_hour: 0,
            pod_kills_last_hour: 0,
        }
    }

    fn activity_map(
        entries: impl IntoIterator<Item = SystemActivity>,
    ) -> HashMap<i32, SystemActivity> {
        entries
            .into_iter()
            .map(|activity| (activity.system_id, activity))
            .collect()
    }

    fn ids(candidates: &[ScoredSystem]) -> HashSet<i32> {
        candidates
            .iter()
            .map(|candidate| candidate.system_id)
            .collect()
    }

    #[test]
    fn hard_include_returns_only_preferred_faction_region_candidates() {
        let graph = faction_region_graph();
        let mut config = with_resolved_preferred_regions(config(), vec![10]);
        config.faction_space.behavior = FactionSpaceBehavior::HardInclude;

        let candidates = filter_candidates(&graph, 1, &HashMap::new(), &config);
        let candidate_ids = ids(&candidates);

        assert!(candidate_ids.contains(&2));
        assert!(!candidate_ids.contains(&3));
        assert!(!candidate_ids.contains(&4));
    }

    #[test]
    fn soft_bonus_increases_preferred_region_score_without_excluding_other_regions() {
        let graph = faction_region_graph();
        let baseline = config();
        let mut soft = with_resolved_preferred_regions(config(), vec![10]);
        soft.faction_space.behavior = FactionSpaceBehavior::SoftBonus;
        soft.faction_space.soft_bonus = 0.2;

        let activity = activity_map([SystemActivity {
            jumps_last_hour: 50,
            npc_kills_last_hour: 50,
            ..activity(2)
        }]);
        let baseline_candidates = filter_candidates(&graph, 1, &activity, &baseline);
        let soft_candidates = filter_candidates(&graph, 1, &activity, &soft);

        let baseline_preferred_score = baseline_candidates
            .iter()
            .find(|candidate| candidate.system_id == 2)
            .unwrap()
            .score;
        let soft_preferred = soft_candidates
            .iter()
            .find(|candidate| candidate.system_id == 2)
            .unwrap();

        assert!(soft_preferred.score > baseline_preferred_score);
        assert_eq!(soft_preferred.score_breakdown.faction_space_bonus, 0.2);
        assert!(soft_candidates
            .iter()
            .any(|candidate| candidate.system_id == 3));
        assert!(soft_candidates
            .iter()
            .any(|candidate| candidate.system_id == 4));
    }

    #[test]
    fn candidate_only_excluded_faction_prevents_waypoints_but_allows_transit() {
        let graph = faction_region_graph();
        let mut config = with_resolved_candidate_only_excluded_regions(config(), vec![30]);
        config.faction_space.exclude_behavior = FactionExcludeBehavior::CandidateOnly;

        let candidates = filter_candidates(&graph, 1, &HashMap::new(), &config);
        let candidate_ids = ids(&candidates);

        assert!(!candidate_ids.contains(&4));
        assert_eq!(
            graph.shortest_path_highsec_only(1, 4),
            Some(vec![1, 2, 3, 4])
        );
    }

    #[test]
    fn unreachable_systems_are_excluded() {
        let graph = graph();
        let candidates = filter_candidates(&graph, 1, &HashMap::new(), &config());

        assert!(!ids(&candidates).contains(&6));
    }

    #[test]
    fn systems_beyond_max_distance_are_excluded() {
        let graph = graph();
        let mut config = config();
        config.filter.max_distance_from_start = Some(1);

        let candidates = filter_candidates(&graph, 1, &HashMap::new(), &config);

        assert!(ids(&candidates).contains(&2));
        assert!(!ids(&candidates).contains(&3));
    }

    #[test]
    fn avoid_list_systems_are_excluded() {
        let graph = graph();
        let mut config = config();
        config.avoid.systems = vec!["Near".to_string()];

        let candidates = filter_candidates(&graph, 1, &HashMap::new(), &config);

        assert!(!ids(&candidates).contains(&2));
    }

    #[test]
    fn avoid_region_ids_exclude_all_systems_in_region_from_candidates() {
        let graph = build_highsec_graph(
            vec![
                SolarSystem {
                    region_id: 10,
                    ..system(1, "Start")
                },
                SolarSystem {
                    region_id: 20,
                    ..system(2, "Blocked One")
                },
                SolarSystem {
                    region_id: 20,
                    ..system(3, "Blocked Two")
                },
                SolarSystem {
                    region_id: 30,
                    ..system(4, "Allowed")
                },
            ],
            vec![gate(1, 2), gate(2, 3), gate(1, 4)],
            0.45,
        );
        let mut config = config();
        config.avoid.region_ids = vec![20];

        let candidates = filter_candidates(&graph, 1, &HashMap::new(), &config);
        let candidate_ids = ids(&candidates);

        assert!(!candidate_ids.contains(&2));
        assert!(!candidate_ids.contains(&3));
        assert!(candidate_ids.contains(&4));
    }

    #[test]
    fn trade_hub_radius_exclusion_works() {
        let graph = graph();
        let mut config = config();
        config.filter.trade_hub_behavior = FilterBehavior::HardExclude;
        config.route.trade_hub_radius = 1;

        let candidates = filter_candidates(&graph, 1, &HashMap::new(), &config);
        let candidate_ids = ids(&candidates);

        assert!(!candidate_ids.contains(&4));
        assert!(!candidate_ids.contains(&5));
        assert!(!candidate_ids.contains(&3));
        assert!(candidate_ids.contains(&2));
    }

    #[test]
    fn trade_hub_soft_penalty_keeps_candidate_but_lowers_score() {
        let graph = graph();
        let mut baseline = config();
        baseline.filter.trade_hubs = vec!["SoftHub".to_string()];
        baseline.filter.trade_hub_behavior = FilterBehavior::Disabled;
        baseline.route.trade_hub_radius = 1;

        let mut soft = baseline.clone();
        soft.filter.trade_hub_behavior = FilterBehavior::SoftPenalty;
        soft.filter.trade_hub_soft_penalty = 0.5;

        let baseline_candidates = filter_candidates(&graph, 1, &HashMap::new(), &baseline);
        let soft_candidates = filter_candidates(&graph, 1, &HashMap::new(), &soft);

        let baseline_score = baseline_candidates
            .iter()
            .find(|candidate| candidate.system_id == 8)
            .unwrap()
            .score;
        let soft_score = soft_candidates
            .iter()
            .find(|candidate| candidate.system_id == 8)
            .unwrap()
            .score;

        assert!(soft_candidates
            .iter()
            .any(|candidate| candidate.system_id == 8));
        assert!(soft_score < baseline_score);
    }

    #[test]
    fn high_jumps_above_threshold_are_excluded_in_strict_mode() {
        let graph = graph();
        let mut config = config();
        config.filter.activity_behavior = FilterBehavior::HardExclude;
        config.filter.max_jumps_last_hour = Some(10);
        let activity = activity_map([SystemActivity {
            jumps_last_hour: 11,
            ..activity(2)
        }]);

        let candidates = filter_candidates(&graph, 1, &activity, &config);

        assert!(!ids(&candidates).contains(&2));
    }

    #[test]
    fn high_npc_kills_above_threshold_are_excluded_in_strict_mode() {
        let graph = graph();
        let mut config = config();
        config.filter.activity_behavior = FilterBehavior::HardExclude;
        config.filter.max_npc_kills_last_hour = Some(10);
        let activity = activity_map([SystemActivity {
            npc_kills_last_hour: 11,
            ..activity(2)
        }]);

        let candidates = filter_candidates(&graph, 1, &activity, &config);

        assert!(!ids(&candidates).contains(&2));
    }

    #[test]
    fn high_ship_kills_above_threshold_are_excluded() {
        let graph = graph();
        let mut config = config();
        config.filter.activity_behavior = FilterBehavior::HardExclude;
        config.filter.max_ship_kills_last_hour = Some(1);
        let activity = activity_map([SystemActivity {
            ship_kills_last_hour: 2,
            ..activity(2)
        }]);

        let candidates = filter_candidates(&graph, 1, &activity, &config);

        assert!(!ids(&candidates).contains(&2));
    }

    #[test]
    fn high_pod_kills_above_threshold_are_excluded() {
        let graph = graph();
        let mut config = config();
        config.filter.activity_behavior = FilterBehavior::HardExclude;
        config.filter.max_pod_kills_last_hour = Some(1);
        let activity = activity_map([SystemActivity {
            pod_kills_last_hour: 2,
            ..activity(2)
        }]);

        let candidates = filter_candidates(&graph, 1, &activity, &config);

        assert!(!ids(&candidates).contains(&2));
    }

    #[test]
    fn missing_activity_defaults_do_not_panic() {
        let graph = graph();

        let candidates = filter_candidates(&graph, 1, &HashMap::new(), &config());

        assert!(!candidates.is_empty());
        assert!(candidates
            .iter()
            .all(|candidate| candidate.jumps_last_hour == 0));
    }

    #[test]
    fn systems_from_last_route_receive_reuse_penalty() {
        let graph = graph();
        let candidates = filter_candidates_with_route_history(
            &graph,
            1,
            &activity_map([activity(2), activity(3)]),
            &config(),
            &HashSet::from([2]),
        );

        let reused = candidates
            .iter()
            .find(|candidate| candidate.system_id == 2)
            .expect("reused system should be a candidate");
        assert_eq!(reused.score_breakdown.reuse_penalty, 1.0);
    }

    #[test]
    fn unrelated_systems_do_not_receive_reuse_penalty() {
        let graph = graph();
        let candidates = filter_candidates_with_route_history(
            &graph,
            1,
            &activity_map([activity(2), activity(3)]),
            &config(),
            &HashSet::from([2]),
        );

        let unrelated = candidates
            .iter()
            .find(|candidate| candidate.system_id == 3)
            .expect("unrelated system should be a candidate");
        assert_eq!(unrelated.score_breakdown.reuse_penalty, 0.0);
    }
}
