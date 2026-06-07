use serde::{Deserialize, Serialize};

use crate::routing::route_modes::RouteMode;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Route {
    pub mode: RouteMode,
    pub system_names: Vec<String>,
    pub total_jumps: u32,
}
