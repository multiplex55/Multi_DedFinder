use petgraph::graph::UnGraph;

use crate::model::system::SolarSystem;

pub type HighsecGraph = UnGraph<SolarSystem, u32>;

pub fn empty_highsec_graph() -> HighsecGraph {
    HighsecGraph::default()
}
