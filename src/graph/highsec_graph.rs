use std::collections::{HashMap, HashSet};

use crate::graph::pathfinding;
use crate::model::system::{SolarSystem, StargateConnection};

/// EVE systems with security status below this value are never included in the
/// high-sec routing graph, even when callers request a lower cutoff.
pub const DEFAULT_HIGHSEC_SECURITY_CUTOFF: f32 = 0.45;

/// A filtered, high-sec-only view of known solar systems and their stargate
/// adjacency list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HighsecGraph {
    pub systems: HashMap<i32, SolarSystem>,
    pub neighbors: HashMap<i32, Vec<i32>>,
}

impl HighsecGraph {
    pub fn contains_system(&self, system_id: i32) -> bool {
        self.systems.contains_key(&system_id)
    }

    pub fn reachable_systems_from(&self, start_system_id: i32) -> HashSet<i32> {
        pathfinding::reachable_systems_from(self, start_system_id)
    }

    pub fn shortest_path_highsec_only(&self, from: i32, to: i32) -> Option<Vec<i32>> {
        pathfinding::shortest_path_highsec_only(self, from, to)
    }

    pub fn jump_distance(&self, from: i32, to: i32) -> Option<u32> {
        pathfinding::jump_distance(self, from, to)
    }

    pub fn neighbor_count(&self, system_id: i32) -> usize {
        self.neighbors
            .get(&system_id)
            .map_or(0, |neighbors| neighbors.len())
    }

    pub fn systems_within_jumps(&self, system_id: i32, radius: u32) -> HashSet<i32> {
        pathfinding::systems_within_jumps(self, system_id, radius)
    }

    /// Returns the share of all high-sec graph systems that are within `radius`
    /// jumps of `system_id`.
    pub fn highsec_density(&self, system_id: i32, radius: u32) -> f32 {
        if self.systems.is_empty() || !self.contains_system(system_id) {
            return 0.0;
        }

        self.systems_within_jumps(system_id, radius).len() as f32 / self.systems.len() as f32
    }
}

pub fn empty_highsec_graph() -> HighsecGraph {
    HighsecGraph::default()
}

pub fn build_highsec_graph(
    systems: impl IntoIterator<Item = SolarSystem>,
    stargates: impl IntoIterator<Item = StargateConnection>,
    min_security: f32,
) -> HighsecGraph {
    let security_cutoff = min_security.max(DEFAULT_HIGHSEC_SECURITY_CUTOFF);

    let systems: HashMap<i32, SolarSystem> = systems
        .into_iter()
        .filter(|system| system.security_status >= security_cutoff)
        .map(|system| (system.id, system))
        .collect();

    let mut edge_set = HashSet::new();

    for stargate in stargates {
        let from = stargate.from_system_id;
        let to = stargate.to_system_id;

        if from == to || !systems.contains_key(&from) || !systems.contains_key(&to) {
            continue;
        }

        let edge = if from < to { (from, to) } else { (to, from) };
        edge_set.insert(edge);
    }

    let mut neighbors: HashMap<i32, Vec<i32>> = systems
        .keys()
        .map(|system_id| (*system_id, Vec::new()))
        .collect();

    for (from, to) in edge_set {
        neighbors.entry(from).or_default().push(to);
        neighbors.entry(to).or_default().push(from);
    }

    for system_neighbors in neighbors.values_mut() {
        system_neighbors.sort_unstable();
    }

    HighsecGraph { systems, neighbors }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn security_filter_excludes_systems_below_default_cutoff() {
        let graph = build_highsec_graph(
            vec![system(1, 0.9), system(2, 0.44), system(3, 0.45)],
            vec![gate(1, 2), gate(1, 3)],
            0.0,
        );

        assert!(graph.contains_system(1));
        assert!(graph.contains_system(3));
        assert!(!graph.contains_system(2));
        assert_eq!(graph.neighbors.get(&1), Some(&vec![3]));
    }

    #[test]
    fn low_sec_edges_are_removed() {
        let graph = build_highsec_graph(
            vec![system(1, 0.9), system(2, 0.3), system(3, 0.9)],
            vec![gate(1, 2), gate(2, 3)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        assert_eq!(graph.neighbor_count(1), 0);
        assert_eq!(graph.neighbor_count(3), 0);
    }

    #[test]
    fn graph_treats_edges_as_bidirectional_and_deduplicates_them() {
        let graph = build_highsec_graph(
            vec![system(1, 0.9), system(2, 0.9)],
            vec![gate(1, 2), gate(2, 1), gate(1, 2)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        assert_eq!(graph.neighbors.get(&1), Some(&vec![2]));
        assert_eq!(graph.neighbors.get(&2), Some(&vec![1]));
    }

    #[test]
    fn hard_excluded_faction_regions_prevent_route_legs_from_crossing_them() {
        let mut systems = vec![system(1, 0.9), system(2, 0.9), system(3, 0.9)];
        systems[1].region_id = 20;
        let excluded_region_ids = HashSet::from([20]);

        let graph = build_highsec_graph(
            systems
                .into_iter()
                .filter(|system| !excluded_region_ids.contains(&system.region_id)),
            vec![gate(1, 2), gate(2, 3)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        assert!(!graph.contains_system(2));
        assert_eq!(graph.shortest_path_highsec_only(1, 3), None);
    }

    #[test]
    fn missing_stargate_endpoint_is_ignored() {
        let graph = build_highsec_graph(
            vec![system(1, 0.9), system(2, 0.9)],
            vec![gate(1, 99), gate(1, 2)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        assert_eq!(graph.neighbors.get(&1), Some(&vec![2]));
        assert_eq!(graph.neighbors.get(&2), Some(&vec![1]));
        assert!(!graph.neighbors.contains_key(&99));
    }
}
