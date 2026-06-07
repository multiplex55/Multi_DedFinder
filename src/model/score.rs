use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct RouteScore {
    pub total: f32,
    pub activity: f32,
    pub distance: f32,
    pub security: f32,
}
