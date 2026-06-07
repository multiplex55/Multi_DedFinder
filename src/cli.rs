use std::path::PathBuf;
use std::str::FromStr;

use clap::{ArgAction, Args, Parser, Subcommand};

use crate::model::route::RouteMode;

#[derive(Debug, Parser)]
#[command(name = "eve-ded-route")]
#[command(about = "Generate and optionally push EVE Online DED route waypoints.")]
#[command(
    long_about = "Generate high-sec DED route waypoints from public ESI activity data and static/local SDE data. This tool does not scan live anomalies, parse probe scanner results, automate the EVE UI, click in-client, or interact with the EVE client process."
)]
pub struct Cli {
    /// Path to a config.toml file.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Generate a route from local SDE and public ESI activity data; does not scan live anomalies.
    #[command(
        long_about = "Generate a route from public/static data: public ESI activity data and static/local SDE data. This tool does not scan live anomalies, parse probe scanner results, automate the EVE UI, click in-client, or interact with the EVE client process."
    )]
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

    /// Route generation strategy. Use --all-modes to generate UltraQuiet, DenseQuiet, and Sweep together.
    #[arg(long, value_parser = parse_route_mode)]
    pub mode: Option<RouteMode>,

    /// Generate UltraQuiet, DenseQuiet, and Sweep routes in one run.
    #[arg(long, action = ArgAction::SetTrue, conflicts_with = "mode")]
    pub all_modes: Option<bool>,

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

fn parse_route_mode(value: &str) -> Result<RouteMode, String> {
    RouteMode::from_str(value).map_err(|error| format!("invalid route mode: {error}"))
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
