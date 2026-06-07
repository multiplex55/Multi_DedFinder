use std::io::{self, Write};

use anyhow::Result;

use crate::config::AppConfig;

pub fn print_generation_summary(config: &AppConfig) -> Result<()> {
    writeln!(
        io::stdout(),
        "Generating {} {} waypoints from {}",
        config.route.waypoint_count,
        config.route.mode,
        config.start.system.as_deref().unwrap_or("configured start")
    )?;
    Ok(())
}
