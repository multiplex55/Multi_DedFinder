use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SolarSystem {
    pub id: i32,
    pub name: String,
    pub security_status: f32,
    pub region_id: i32,
}

impl SolarSystem {
    pub fn is_highsec_at(&self, min_security_status: f32) -> bool {
        self.security_status >= min_security_status
    }
}
