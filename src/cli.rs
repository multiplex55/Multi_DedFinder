use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

use crate::model::route::RouteMode;

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

#[derive(Args, Clone, Debug, Default, PartialEq)]
pub struct CliOptions {
    /// Starting solar system name.
    #[arg(long)]
    pub start: Option<String>,

    /// Path to prepared local SDE-derived data files.
    #[arg(long)]
    pub sde_path: Option<PathBuf>,

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

    /// Write text output to this path. If omitted, text output is printed to stdout.
    #[arg(long, value_name = "PATH")]
    pub output: Option<PathBuf>,

    /// Write pretty-printed JSON output to this path.
    #[arg(long, value_name = "PATH")]
    pub json: Option<PathBuf>,

    /// Push generated waypoints after generation.
    #[arg(long, action = ArgAction::SetTrue)]
    pub push_waypoints: Option<bool>,

    /// Prefer routes that return to the start system.
    #[arg(long, action = ArgAction::SetTrue, conflicts_with = "no_prefer_loop")]
    pub prefer_loop: Option<bool>,

    /// Do not prefer routes that return to the start system.
    #[arg(long, action = ArgAction::SetTrue)]
    pub no_prefer_loop: Option<bool>,

    /// EVE character ID to authenticate and push waypoints for.
    #[arg(long)]
    pub character_id: Option<i64>,

    /// EVE character name to show in prompts and validation messages.
    #[arg(long)]
    pub character_name: Option<String>,

    /// Skip confirmation prompts.
    #[arg(long, action = ArgAction::SetTrue)]
    pub yes: Option<bool>,

    /// Print what would be pushed without calling ESI.
    #[arg(long, action = ArgAction::SetTrue)]
    pub dry_run: Option<bool>,
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
