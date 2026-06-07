mod common;

use std::collections::HashSet;

use eve_ded_route::model::route::RouteMode;
use eve_ded_route::routing::generator::generate_route;

use common::{
    all_route_system_ids, candidates_without_start, config, hard_activity_filter_config,
    load_fixture, reused_transit_hops, route_from_fixture, waypoint_ids,
};

#[test]
fn linear_highsec_chain_uses_sequential_highsec_path() {
    let fixture = load_fixture("linear_highsec_chain");
    let config = config(RouteMode::DenseQuiet, 1);
    let candidates = candidates_without_start(&fixture, 100, &config)
        .into_iter()
        .filter(|candidate| candidate.system_id == 104)
        .collect::<Vec<_>>();

    let route = generate_route(&fixture.graph, 100, &candidates, &config);

    assert_eq!(route.legs.len(), 1);
    assert_eq!(route.legs[0].path_system_ids, vec![100, 101, 102, 103, 104]);
}

#[test]
fn generated_route_contains_only_highsec_systems() {
    let fixture = load_fixture("lowsec_shortcut");
    let route = route_from_fixture("lowsec_shortcut", 200, &config(RouteMode::UltraQuiet, 2));

    assert!(!fixture.graph.contains_system(202));
    assert!(all_route_system_ids(&route)
        .into_iter()
        .all(|system_id| fixture.graph.contains_system(system_id)));
}

#[test]
fn route_path_between_waypoints_contains_only_highsec_systems() {
    let fixture = load_fixture("lowsec_shortcut");
    let config = config(RouteMode::DenseQuiet, 1);
    let candidates = candidates_without_start(&fixture, 200, &config)
        .into_iter()
        .filter(|candidate| candidate.system_id == 203)
        .collect::<Vec<_>>();

    let route = generate_route(&fixture.graph, 200, &candidates, &config);

    assert_eq!(route.legs.len(), 1);
    assert_eq!(route.legs[0].to_system_id, 203);
    assert_eq!(route.legs[0].path_system_ids, vec![200, 201, 204, 205, 203]);
    assert!(!route.legs[0].path_system_ids.contains(&202));
}

#[test]
fn high_traffic_systems_are_avoided_when_alternatives_exist() {
    let route = route_from_fixture(
        "dense_quiet_vs_busy_cluster",
        400,
        &hard_activity_filter_config(RouteMode::DenseQuiet, 4),
    );
    let waypoint_ids = waypoint_ids(&route);

    assert!(!waypoint_ids
        .iter()
        .any(|system_id| (405..=408).contains(system_id)));
    assert!(waypoint_ids
        .iter()
        .any(|system_id| (401..=404).contains(system_id)));
}

#[test]
fn dense_quiet_chooses_cluster_over_isolated_quiet_dead_end() {
    let route = route_from_fixture(
        "quiet_dead_end_cluster",
        300,
        &config(RouteMode::DenseQuiet, 1),
    );
    let chosen = waypoint_ids(&route);

    assert_eq!(chosen.len(), 1);
    assert_ne!(chosen[0], 301);
    assert!((302..=306).contains(&chosen[0]));
}

#[test]
fn ultra_quiet_tolerates_less_efficient_travel_for_quieter_systems() {
    let route = route_from_fixture("lowsec_shortcut", 200, &config(RouteMode::UltraQuiet, 1));

    assert_eq!(waypoint_ids(&route), vec![203]);
    assert_eq!(route.legs[0].path_system_ids, vec![200, 201, 204, 205, 203]);
}

#[test]
fn sweep_covers_more_systems_with_less_backtracking() {
    let dense_route = route_from_fixture(
        "quiet_dead_end_cluster",
        300,
        &config(RouteMode::DenseQuiet, 4),
    );
    let sweep_route =
        route_from_fixture("quiet_dead_end_cluster", 300, &config(RouteMode::Sweep, 4));

    let dense_covered = all_route_system_ids(&dense_route)
        .into_iter()
        .collect::<HashSet<_>>()
        .len();
    let sweep_covered = all_route_system_ids(&sweep_route)
        .into_iter()
        .collect::<HashSet<_>>()
        .len();

    assert!(sweep_covered >= dense_covered);
    assert!(reused_transit_hops(&sweep_route) <= reused_transit_hops(&dense_route));
}

#[test]
fn candidate_pool_too_small_returns_fewer_than_requested_waypoints_without_panic() {
    let fixture = load_fixture("linear_highsec_chain");
    let mut config = config(RouteMode::DenseQuiet, 10);
    config.route.max_distance = Some(2);
    let candidates = candidates_without_start(&fixture, 100, &config);

    let route = generate_route(&fixture.graph, 100, &candidates, &config);

    assert!(route.waypoints.len() < 10);
    assert_eq!(waypoint_ids(&route), vec![101, 102]);
}

#[test]
fn isolated_start_after_highsec_filtering_returns_empty_route() {
    let route = route_from_fixture("isolated_start", 500, &config(RouteMode::DenseQuiet, 3));

    assert!(route.waypoints.is_empty());
    assert!(route.legs.is_empty());
    assert_eq!(route.total_jumps, 0);
}
