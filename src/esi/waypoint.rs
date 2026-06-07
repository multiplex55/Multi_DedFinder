use anyhow::Result;

use crate::config::AppConfig;

pub async fn push_waypoints(config: &AppConfig) -> Result<()> {
    tracing::info!(
        count = config.route.waypoint_count,
        "waypoint push requested"
    );
    Ok(())
}
