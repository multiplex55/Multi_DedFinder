use std::collections::{HashMap, HashSet, VecDeque};

use crate::graph::highsec_graph::HighsecGraph;

pub fn reachable_systems_from(graph: &HighsecGraph, start_system_id: i32) -> HashSet<i32> {
    let mut reachable = HashSet::new();

    if !graph.contains_system(start_system_id) {
        return reachable;
    }

    let mut queue = VecDeque::from([start_system_id]);
    reachable.insert(start_system_id);

    while let Some(system_id) = queue.pop_front() {
        if let Some(neighbors) = graph.neighbors.get(&system_id) {
            for &neighbor_id in neighbors {
                if reachable.insert(neighbor_id) {
                    queue.push_back(neighbor_id);
                }
            }
        }
    }

    reachable
}

pub fn shortest_path_highsec_only(graph: &HighsecGraph, from: i32, to: i32) -> Option<Vec<i32>> {
    if !graph.contains_system(from) || !graph.contains_system(to) {
        return None;
    }

    if from == to {
        return Some(vec![from]);
    }

    let mut queue = VecDeque::from([from]);
    let mut previous: HashMap<i32, i32> = HashMap::new();
    let mut visited = HashSet::from([from]);

    while let Some(system_id) = queue.pop_front() {
        if let Some(neighbors) = graph.neighbors.get(&system_id) {
            for &neighbor_id in neighbors {
                if !visited.insert(neighbor_id) {
                    continue;
                }

                previous.insert(neighbor_id, system_id);

                if neighbor_id == to {
                    return Some(reconstruct_path(from, to, &previous));
                }

                queue.push_back(neighbor_id);
            }
        }
    }

    None
}

pub fn jump_distance(graph: &HighsecGraph, from: i32, to: i32) -> Option<u32> {
    shortest_path_highsec_only(graph, from, to).map(|path| path.len().saturating_sub(1) as u32)
}

pub fn systems_within_jumps(graph: &HighsecGraph, system_id: i32, radius: u32) -> HashSet<i32> {
    let mut within = HashSet::new();

    if !graph.contains_system(system_id) {
        return within;
    }

    let mut queue = VecDeque::from([(system_id, 0)]);
    within.insert(system_id);

    while let Some((current_id, distance)) = queue.pop_front() {
        if distance == radius {
            continue;
        }

        if let Some(neighbors) = graph.neighbors.get(&current_id) {
            for &neighbor_id in neighbors {
                if within.insert(neighbor_id) {
                    queue.push_back((neighbor_id, distance + 1));
                }
            }
        }
    }

    within
}

fn reconstruct_path(from: i32, to: i32, previous: &HashMap<i32, i32>) -> Vec<i32> {
    let mut path = vec![to];
    let mut current = to;

    while current != from {
        current = previous[&current];
        path.push(current);
    }

    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use crate::graph::highsec_graph::{build_highsec_graph, DEFAULT_HIGHSEC_SECURITY_CUTOFF};
    use crate::model::system::{SolarSystem, StargateConnection};

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
    fn unreachable_systems_are_excluded_from_reachability_set() {
        let graph = build_highsec_graph(
            vec![system(1, 0.9), system(2, 0.9), system(3, 0.9)],
            vec![gate(1, 2)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        let reachable = graph.reachable_systems_from(1);

        assert_eq!(reachable, [1, 2].into_iter().collect());
        assert!(!reachable.contains(&3));
    }

    #[test]
    fn shortest_path_avoids_tempting_low_sec_shortcut() {
        let graph = build_highsec_graph(
            vec![
                system(1, 0.9),
                system(2, 0.2),
                system(3, 0.9),
                system(4, 0.9),
                system(5, 0.9),
            ],
            vec![gate(1, 2), gate(2, 3), gate(1, 4), gate(4, 5), gate(5, 3)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        assert_eq!(
            graph.shortest_path_highsec_only(1, 3),
            Some(vec![1, 4, 5, 3])
        );
        assert_eq!(graph.jump_distance(1, 3), Some(3));
    }

    #[test]
    fn start_system_isolated_after_high_sec_filtering_is_detected() {
        let graph = build_highsec_graph(
            vec![system(1, 0.9), system(2, 0.2), system(3, 0.9)],
            vec![gate(1, 2), gate(2, 3)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        assert_eq!(graph.neighbor_count(1), 0);
        assert_eq!(graph.reachable_systems_from(1), [1].into_iter().collect());
        assert_eq!(graph.shortest_path_highsec_only(1, 3), None);
    }

    #[test]
    fn no_route_path_contains_system_ids_absent_from_high_sec_graph() {
        let graph = build_highsec_graph(
            vec![
                system(1, 0.9),
                system(2, 0.1),
                system(3, 0.9),
                system(4, 0.9),
            ],
            vec![gate(1, 2), gate(2, 3), gate(1, 4), gate(4, 3)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        let path = graph.shortest_path_highsec_only(1, 3).unwrap();

        assert_eq!(path, vec![1, 4, 3]);
        assert!(path
            .iter()
            .all(|system_id| graph.contains_system(*system_id)));
    }

    #[test]
    fn missing_or_filtered_start_system_has_no_reachability_or_path() {
        let graph = build_highsec_graph(
            vec![system(1, 0.9), system(2, 0.1)],
            vec![gate(1, 2)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        assert!(graph.reachable_systems_from(2).is_empty());
        assert!(graph.reachable_systems_from(99).is_empty());
        assert_eq!(graph.shortest_path_highsec_only(2, 1), None);
        assert_eq!(graph.shortest_path_highsec_only(99, 1), None);
    }

    #[test]
    fn cluster_helpers_count_neighbors_radius_and_density() {
        let graph = build_highsec_graph(
            vec![
                system(1, 0.9),
                system(2, 0.9),
                system(3, 0.9),
                system(4, 0.9),
            ],
            vec![gate(1, 2), gate(2, 3)],
            DEFAULT_HIGHSEC_SECURITY_CUTOFF,
        );

        assert_eq!(graph.neighbor_count(2), 2);
        assert_eq!(
            graph.systems_within_jumps(1, 1),
            [1, 2].into_iter().collect()
        );
        assert_eq!(
            graph.systems_within_jumps(1, 2),
            [1, 2, 3].into_iter().collect()
        );
        assert_eq!(graph.highsec_density(1, 2), 0.75);
    }
}
