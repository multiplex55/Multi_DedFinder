use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SolarSystem {
    pub id: i32,
    pub name: String,
    pub security_status: f32,
    pub region_id: i32,
    pub constellation_id: i32,
}

impl SolarSystem {
    pub fn is_highsec_at(&self, min_security_status: f32) -> bool {
        self.security_status >= min_security_status
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StargateConnection {
    pub from_system_id: i32,
    pub to_system_id: i32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SystemActivity {
    pub system_id: i32,
    pub jumps_last_hour: u32,
    pub npc_kills_last_hour: u32,
    pub ship_kills_last_hour: u32,
    pub pod_kills_last_hour: u32,
    pub activity_timestamp: DateTime<Utc>,
}
