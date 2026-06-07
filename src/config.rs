use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::cli::{CliOptions, OutputFormat};
use crate::model::route::RouteMode;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct AppConfig {
    pub start: StartConfig,
    pub data: DataConfig,
    pub route: RouteConfig,
    pub filter: FilterConfig,
    pub weights: WeightConfig,
    pub avoid: AvoidConfig,
    pub esi: EsiConfig,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct StartConfig {
    pub system: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct DataConfig {
    pub sde_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct RouteConfig {
    pub waypoint_count: usize,
    pub max_distance: Option<u32>,
    pub mode: RouteMode,
    pub output: ConfigOutputFormat,
    pub push_waypoints: bool,
    pub prefer_loop: bool,
    pub trade_hub_radius: u32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigOutputFormat {
    #[default]
    Text,
    Json,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct FilterConfig {
    pub highsec_only: bool,
    pub min_security_status: f32,
    pub max_distance_from_start: Option<u32>,
    pub max_jumps_last_hour: Option<u32>,
    pub max_npc_kills_last_hour: Option<u32>,
    pub max_ship_kills_last_hour: Option<u32>,
    pub max_pod_kills_last_hour: Option<u32>,
    pub activity_behavior: FilterBehavior,
    pub trade_hub_behavior: FilterBehavior,
    pub trade_hubs: Vec<String>,
    pub trade_hub_soft_penalty: f32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterBehavior {
    #[default]
    HardExclude,
    SoftPenalty,
    Disabled,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct WeightConfig {
    pub activity: f32,
    pub distance: f32,
    pub security: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct AvoidConfig {
    pub systems: Vec<String>,
    pub regions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct EsiConfig {
    pub client_id: Option<String>,
    pub callback_url: Option<String>,
    pub activity_cache_minutes: u64,
    pub activity_cache_path: Option<PathBuf>,
    pub allow_stale_activity_cache: bool,
}

impl AppConfig {
    pub fn from_optional_path(path: Option<&Path>) -> Result<Self> {
        match path {
            Some(path) => Self::from_path(path),
            None => Ok(Self::default()),
        }
    }

    pub fn from_path(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;
        Self::from_toml_str(&contents)
    }

    pub fn from_toml_str(contents: &str) -> Result<Self> {
        toml::from_str(contents).context("failed to parse config TOML")
    }

    pub fn with_cli_overrides(mut self, cli: &CliOptions) -> Self {
        if let Some(start) = &cli.start {
            self.start.system = Some(start.clone());
        }
        if let Some(sde_path) = &cli.sde_path {
            self.data.sde_path = Some(sde_path.clone());
        }
        if let Some(waypoints) = cli.waypoints {
            self.route.waypoint_count = waypoints;
        }
        if let Some(max_distance) = cli.max_distance {
            self.route.max_distance = Some(max_distance);
        }
        if let Some(highsec_only) = cli.highsec_only {
            self.filter.highsec_only = highsec_only;
        }
        if let Some(mode) = cli.mode {
            self.route.mode = mode;
        }
        if let Some(output) = cli.output {
            self.route.output = output.into();
        }
        if cli.json.unwrap_or(false) {
            self.route.output = ConfigOutputFormat::Json;
        }
        if let Some(push_waypoints) = cli.push_waypoints {
            self.route.push_waypoints = push_waypoints;
        }
        if let Some(prefer_loop) = cli.prefer_loop_override() {
            self.route.prefer_loop = prefer_loop;
        }
        self
    }
}

impl Default for RouteConfig {
    fn default() -> Self {
        Self {
            waypoint_count: 25,
            max_distance: None,
            mode: RouteMode::DenseQuiet,
            output: ConfigOutputFormat::Text,
            push_waypoints: false,
            prefer_loop: true,
            trade_hub_radius: 3,
        }
    }
}

impl From<OutputFormat> for ConfigOutputFormat {
    fn from(value: OutputFormat) -> Self {
        match value {
            OutputFormat::Text => Self::Text,
            OutputFormat::Json => Self::Json,
        }
    }
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            highsec_only: true,
            min_security_status: 0.45,
            max_distance_from_start: None,
            max_jumps_last_hour: Some(80),
            max_npc_kills_last_hour: Some(250),
            max_ship_kills_last_hour: Some(25),
            max_pod_kills_last_hour: Some(10),
            activity_behavior: FilterBehavior::HardExclude,
            trade_hub_behavior: FilterBehavior::HardExclude,
            trade_hubs: default_trade_hubs(),
            trade_hub_soft_penalty: 0.25,
        }
    }
}

impl Default for AvoidConfig {
    fn default() -> Self {
        Self {
            systems: default_avoid_systems(),
            regions: Vec::new(),
        }
    }
}

fn default_avoid_systems() -> Vec<String> {
    ["Jita", "Perimeter", "Uedama", "Sivala", "Ahbazon"]
        .into_iter()
        .map(String::from)
        .collect()
}

fn default_trade_hubs() -> Vec<String> {
    ["Jita", "Amarr", "Dodixie", "Rens", "Hek"]
        .into_iter()
        .map(String::from)
        .collect()
}

impl Default for WeightConfig {
    fn default() -> Self {
        Self {
            activity: 1.0,
            distance: 1.0,
            security: 1.0,
        }
    }
}

impl Default for EsiConfig {
    fn default() -> Self {
        Self {
            client_id: None,
            callback_url: None,
            activity_cache_minutes: 15,
            activity_cache_path: None,
            allow_stale_activity_cache: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values_match_task_brief() {
        let config = AppConfig::default();

        assert_eq!(config.filter.min_security_status, 0.45);
        assert_eq!(
            config.avoid.systems,
            vec!["Jita", "Perimeter", "Uedama", "Sivala", "Ahbazon"]
        );
        assert_eq!(
            config.filter.trade_hubs,
            vec!["Jita", "Amarr", "Dodixie", "Rens", "Hek"]
        );
        assert_eq!(config.route.waypoint_count, 25);
        assert_eq!(config.esi.activity_cache_minutes, 15);
        assert!(config.esi.activity_cache_path.is_none());
        assert!(!config.esi.allow_stale_activity_cache);
        assert_eq!(config.route.mode, RouteMode::DenseQuiet);
        assert_eq!(config.route.trade_hub_radius, 3);
    }

    #[test]
    fn parses_toml_config() {
        let config = AppConfig::from_toml_str(
            r#"
[start]
system = "Jita"

[data]
sde_path = "tests/fixtures/sde"

[route]
waypoint_count = 12
max_distance = 40
mode = "ultra_quiet"
output = "json"
push_waypoints = true
prefer_loop = false
trade_hub_radius = 5

[filter]
highsec_only = false
min_security_status = 0.6

[weights]
activity = 2.0
distance = 0.5
security = 3.0

[avoid]
systems = ["Uedama"]
regions = ["Pochven"]

[esi]
client_id = "client"
callback_url = "http://localhost/callback"
activity_cache_minutes = 30
activity_cache_path = "/tmp/eve-activity.json"
allow_stale_activity_cache = true
"#,
        )
        .expect("config should parse");

        assert_eq!(config.start.system.as_deref(), Some("Jita"));
        assert_eq!(
            config.data.sde_path.as_deref(),
            Some(Path::new("tests/fixtures/sde"))
        );
        assert_eq!(config.route.waypoint_count, 12);
        assert_eq!(config.route.max_distance, Some(40));
        assert_eq!(config.route.mode, RouteMode::UltraQuiet);
        assert_eq!(config.route.output, ConfigOutputFormat::Json);
        assert!(config.route.push_waypoints);
        assert!(!config.route.prefer_loop);
        assert_eq!(config.route.trade_hub_radius, 5);
        assert!(!config.filter.highsec_only);
        assert_eq!(config.filter.min_security_status, 0.6);
        assert_eq!(config.weights.activity, 2.0);
        assert_eq!(config.avoid.systems, vec!["Uedama"]);
        assert_eq!(config.esi.activity_cache_minutes, 30);
        assert_eq!(
            config.esi.activity_cache_path.as_deref(),
            Some(Path::new("/tmp/eve-activity.json"))
        );
        assert!(config.esi.allow_stale_activity_cache);
    }

    #[test]
    fn cli_values_override_config_file_values() {
        let config = AppConfig::from_toml_str(
            r#"
[start]
system = "Amarr"

[route]
waypoint_count = 10
max_distance = 20
mode = "sweep"
output = "text"
push_waypoints = false
prefer_loop = true

[filter]
highsec_only = false
"#,
        )
        .expect("config should parse");

        let cli = CliOptions {
            start: Some("Jita".to_string()),
            sde_path: Some(PathBuf::from("/tmp/sde")),
            waypoints: Some(30),
            max_distance: Some(50),
            highsec_only: Some(true),
            mode: Some(RouteMode::UltraQuiet),
            output: None,
            json: Some(true),
            push_waypoints: Some(true),
            prefer_loop: None,
            no_prefer_loop: Some(true),
        };

        let merged = config.with_cli_overrides(&cli);

        assert_eq!(merged.start.system.as_deref(), Some("Jita"));
        assert_eq!(merged.data.sde_path.as_deref(), Some(Path::new("/tmp/sde")));
        assert_eq!(merged.route.waypoint_count, 30);
        assert_eq!(merged.route.max_distance, Some(50));
        assert!(merged.filter.highsec_only);
        assert_eq!(merged.route.mode, RouteMode::UltraQuiet);
        assert_eq!(merged.route.output, ConfigOutputFormat::Json);
        assert!(merged.route.push_waypoints);
        assert!(!merged.route.prefer_loop);
    }

    #[test]
    fn parses_supported_route_mode_strings() {
        assert_eq!(
            "ultra_quiet".parse::<RouteMode>().unwrap(),
            RouteMode::UltraQuiet
        );
        assert_eq!(
            "dense_quiet".parse::<RouteMode>().unwrap(),
            RouteMode::DenseQuiet
        );
        assert_eq!("sweep".parse::<RouteMode>().unwrap(), RouteMode::Sweep);
    }
}
