use petgraph::algo::astar;
use petgraph::graph::NodeIndex;

use crate::graph::highsec_graph::HighsecGraph;

pub fn shortest_path(
    graph: &HighsecGraph,
    start: NodeIndex,
    goal: NodeIndex,
) -> Option<(u32, Vec<NodeIndex>)> {
    astar(
        graph,
        start,
        |node| node == goal,
        |edge| *edge.weight(),
        |_| 0,
    )
}
