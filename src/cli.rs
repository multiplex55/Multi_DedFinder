use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};

use crate::routing::route_modes::RouteMode;

#[derive(Debug, Parser)]
#[command(name = "eve-ded-route")]
#[command(about = "Generate and optionally push EVE Online DED route waypoints.")]
pub struct Cli {
    /// Path to a config.toml file.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Generate a route from local SDE and ESI activity data.
    Generate(CliOptions),
    /// Push generated waypoints to ESI.
    Push(CliOptions),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Args, Clone, Debug, Default, PartialEq)]
pub struct CliOptions {
    /// Starting solar system name.
    #[arg(long)]
    pub start: Option<String>,

    /// Number of waypoints to generate.
    #[arg(long)]
    pub waypoints: Option<usize>,

    /// Maximum route distance in jumps.
    #[arg(long)]
    pub max_distance: Option<u32>,

    /// Restrict candidates and paths to high-security systems.
    #[arg(long, action = ArgAction::SetTrue)]
    pub highsec_only: Option<bool>,

    /// Route generation strategy.
    #[arg(long, value_enum)]
    pub mode: Option<RouteMode>,

    /// Output format.
    #[arg(long, value_enum)]
    pub output: Option<OutputFormat>,

    /// Emit JSON output. Equivalent to --output json.
    #[arg(long, action = ArgAction::SetTrue)]
    pub json: Option<bool>,

    /// Push generated waypoints after generation.
    #[arg(long, action = ArgAction::SetTrue)]
    pub push_waypoints: Option<bool>,

    /// Prefer routes that return to the start system.
    #[arg(long, action = ArgAction::SetTrue, conflicts_with = "no_prefer_loop")]
    pub prefer_loop: Option<bool>,

    /// Do not prefer routes that return to the start system.
    #[arg(long, action = ArgAction::SetTrue)]
    pub no_prefer_loop: Option<bool>,
}

impl CliOptions {
    pub fn prefer_loop_override(&self) -> Option<bool> {
        match (self.prefer_loop, self.no_prefer_loop) {
            (Some(true), _) => Some(true),
            (_, Some(true)) => Some(false),
            _ => None,
        }
    }
}
