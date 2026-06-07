use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SystemActivity {
    pub system_id: i32,
    pub npc_kills: u32,
    pub ship_kills: u32,
    pub pod_kills: u32,
    pub observed_at: DateTime<Utc>,
}
