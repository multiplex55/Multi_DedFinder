use std::fmt;
use std::str::FromStr;

use clap::ValueEnum;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum RouteMode {
    UltraQuiet,
    DenseQuiet,
    Sweep,
}

impl RouteMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UltraQuiet => "ultra_quiet",
            Self::DenseQuiet => "dense_quiet",
            Self::Sweep => "sweep",
        }
    }
}

impl Default for RouteMode {
    fn default() -> Self {
        Self::DenseQuiet
    }
}

impl fmt::Display for RouteMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RouteMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ultra_quiet" | "ultra-quiet" => Ok(Self::UltraQuiet),
            "dense_quiet" | "dense-quiet" => Ok(Self::DenseQuiet),
            "sweep" => Ok(Self::Sweep),
            other => Err(format!("unsupported route mode '{other}'")),
        }
    }
}

impl Serialize for RouteMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RouteMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}
