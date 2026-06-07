pub use crate::model::route::RouteMode;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteModeScoring {
    pub jump_weight: f64,
    pub npc_weight: f64,
    pub danger_weight: f64,
    pub cluster_density_weight: f64,
    pub hub_distance_weight: f64,
    pub dead_end_penalty_weight: f64,
    pub reuse_penalty_weight: f64,
    pub score_threshold: f64,
}

impl RouteModeScoring {
    pub const fn for_mode(mode: RouteMode) -> Self {
        match mode {
            RouteMode::UltraQuiet => Self {
                jump_weight: 2.4,
                npc_weight: 2.2,
                danger_weight: 1.6,
                cluster_density_weight: 0.8,
                hub_distance_weight: 0.7,
                dead_end_penalty_weight: 0.1,
                reuse_penalty_weight: 0.9,
                score_threshold: 0.62,
            },
            RouteMode::DenseQuiet => Self {
                jump_weight: 1.4,
                npc_weight: 1.4,
                danger_weight: 1.4,
                cluster_density_weight: 2.0,
                hub_distance_weight: 1.1,
                dead_end_penalty_weight: 1.6,
                reuse_penalty_weight: 1.0,
                score_threshold: 0.55,
            },
            RouteMode::Sweep => Self {
                jump_weight: 2.0,
                npc_weight: 0.9,
                danger_weight: 1.1,
                cluster_density_weight: 0.8,
                hub_distance_weight: 2.1,
                dead_end_penalty_weight: 2.2,
                reuse_penalty_weight: 1.8,
                score_threshold: 0.40,
            },
        }
    }

    pub fn positive_weight_total(self) -> f64 {
        self.jump_weight
            + self.npc_weight
            + self.danger_weight
            + self.cluster_density_weight
            + self.hub_distance_weight
    }
}
