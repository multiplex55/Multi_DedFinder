use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct ScoreBreakdown {
    pub activity: f32,
    pub distance: f32,
    pub security: f32,
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
