use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct ScoreBreakdown {
    #[serde(default)]
    pub activity: f32,
    #[serde(default)]
    pub distance: f32,
    #[serde(default)]
    pub security: f32,
    #[serde(default)]
    pub jump_score: f32,
    #[serde(default)]
    pub npc_score: f32,
    #[serde(default)]
    pub danger_score: f32,
    #[serde(default)]
    pub cluster_density_score: f32,
    #[serde(default)]
    pub hub_distance_score: f32,
    #[serde(default)]
    pub dead_end_penalty: f32,
    #[serde(default)]
    pub reuse_penalty: f32,
    pub total: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ScoredSystem {
    pub system_id: i32,
    pub system_name: String,
    pub security_status: f32,
    pub region_id: i32,
    pub constellation_id: i32,
    pub score: f32,
    pub jumps_last_hour: u32,
    pub npc_kills_last_hour: u32,
    pub ship_kills_last_hour: u32,
    pub pod_kills_last_hour: u32,
    pub distance_from_start: u32,
    pub score_breakdown: ScoreBreakdown,
}

pub type RouteScore = ScoreBreakdown;
