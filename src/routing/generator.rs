use anyhow::Result;

use crate::config::AppConfig;
use crate::model::route::Route;

pub fn generate_route(config: &AppConfig) -> Result<Route> {
    Ok(Route {
        mode: config.route.mode,
        system_names: config.start.system.clone().into_iter().collect(),
        total_jumps: 0,
    })
}
