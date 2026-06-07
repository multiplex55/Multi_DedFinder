use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

use crate::model::system::{SolarSystem, StargateConnection};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SdeDiagnostics {
    pub skipped_unknown_stargate_edges: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SdeData {
    pub systems: HashMap<i32, SolarSystem>,
    pub systems_by_name: HashMap<String, i32>,
    pub stargate_connections: Vec<StargateConnection>,
    pub diagnostics: SdeDiagnostics,
    systems_by_lowercase_name: HashMap<String, i32>,
}

#[derive(Debug, Deserialize)]
struct SystemRecord {
    system_id: i32,
    system_name: String,
    security_status: f32,
    region_id: i32,
    constellation_id: i32,
}

#[derive(Debug, Deserialize)]
struct StargateRecord {
    #[serde(alias = "source_system_id", alias = "from_system_id")]
    source_system_id: i32,
    #[serde(alias = "target_system_id", alias = "to_system_id")]
    target_system_id: i32,
}

impl SdeData {
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if path.is_file() {
            bail!(
                "SDE path {} must be a directory containing systems and stargates files",
                path.display()
            );
        }

        let systems_path = find_prepared_file(path, "systems")?;
        let stargates_path = find_prepared_file(path, "stargates")?;
        Self::load_from_files(systems_path, stargates_path)
    }

    pub fn load_from_files(
        systems_path: impl AsRef<Path>,
        stargates_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let system_records = read_records::<SystemRecord>(systems_path.as_ref())?;
        let stargate_records = read_records::<StargateRecord>(stargates_path.as_ref())?;
        Self::from_records(system_records, stargate_records)
    }

    fn from_records(
        system_records: Vec<SystemRecord>,
        stargate_records: Vec<StargateRecord>,
    ) -> Result<Self> {
        let mut systems = HashMap::new();
        let mut systems_by_name = HashMap::new();
        let mut systems_by_lowercase_name = HashMap::new();

        for record in system_records {
            let canonical_name = normalize_system_name(&record.system_name);
            if canonical_name.is_empty() {
                bail!("system {} has an empty system_name", record.system_id);
            }

            let system = SolarSystem {
                id: record.system_id,
                name: canonical_name.clone(),
                security_status: record.security_status,
                region_id: record.region_id,
                constellation_id: record.constellation_id,
            };

            if systems.insert(system.id, system).is_some() {
                bail!("duplicate system_id {} in SDE data", record.system_id);
            }

            if let Some(existing_id) =
                systems_by_name.insert(canonical_name.clone(), record.system_id)
            {
                bail!(
                    "duplicate system_name {:?} in SDE data for system IDs {} and {}",
                    canonical_name,
                    existing_id,
                    record.system_id
                );
            }

            let lowercase_name = canonical_name.to_lowercase();
            if let Some(existing_id) =
                systems_by_lowercase_name.insert(lowercase_name, record.system_id)
            {
                bail!(
                    "duplicate case-insensitive system_name {:?} in SDE data for system IDs {} and {}",
                    canonical_name,
                    existing_id,
                    record.system_id
                );
            }
        }

        let mut stargate_connections = Vec::new();
        let mut diagnostics = SdeDiagnostics::default();

        for record in stargate_records {
            if !systems.contains_key(&record.source_system_id)
                || !systems.contains_key(&record.target_system_id)
            {
                diagnostics.skipped_unknown_stargate_edges += 1;
                continue;
            }

            stargate_connections.push(StargateConnection {
                from_system_id: record.source_system_id,
                to_system_id: record.target_system_id,
            });
        }

        Ok(Self {
            systems,
            systems_by_name,
            stargate_connections,
            diagnostics,
            systems_by_lowercase_name,
        })
    }

    pub fn system_id_by_name(&self, name: &str) -> Option<i32> {
        let normalized_name = normalize_system_name(name);
        self.systems_by_name
            .get(&normalized_name)
            .copied()
            .or_else(|| {
                self.systems_by_lowercase_name
                    .get(&normalized_name.to_lowercase())
                    .copied()
            })
    }

    pub fn system_by_name(&self, name: &str) -> Option<&SolarSystem> {
        self.system_id_by_name(name)
            .and_then(|system_id| self.systems.get(&system_id))
    }

    pub fn skipped_unknown_stargate_edges(&self) -> usize {
        self.diagnostics.skipped_unknown_stargate_edges
    }
}

pub fn load_systems() -> Result<Vec<SolarSystem>> {
    Ok(Vec::new())
}

pub fn load_systems_from_path(path: impl AsRef<Path>) -> Result<Vec<SolarSystem>> {
    Ok(SdeData::load_from_path(path)?
        .systems
        .into_values()
        .collect())
}

fn find_prepared_file(directory: &Path, stem: &str) -> Result<PathBuf> {
    for extension in ["csv", "json"] {
        let candidate = directory.join(format!("{stem}.{extension}"));
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err(anyhow!(
        "could not find {stem}.csv or {stem}.json under {}",
        directory.display()
    ))
}

fn read_records<T>(path: &Path) -> Result<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
{
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("csv") => read_csv_records(path),
        Some("json") => read_json_records(path),
        extension => bail!(
            "unsupported SDE file extension {:?} for {} (expected csv or json)",
            extension,
            path.display()
        ),
    }
}

fn read_csv_records<T>(path: &Path) -> Result<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let mut reader = csv::Reader::from_path(path)
        .with_context(|| format!("failed to open CSV SDE file {}", path.display()))?;
    reader
        .deserialize()
        .collect::<std::result::Result<Vec<T>, csv::Error>>()
        .with_context(|| format!("failed to parse CSV SDE file {}", path.display()))
}

fn read_json_records<T>(path: &Path) -> Result<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let file = File::open(path)
        .with_context(|| format!("failed to open JSON SDE file {}", path.display()))?;
    serde_json::from_reader(file)
        .with_context(|| format!("failed to parse JSON SDE file {}", path.display()))
}

fn normalize_system_name(name: &str) -> String {
    name.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_path(file_name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/sde")
            .join(file_name)
    }

    fn load_small_fixture() -> SdeData {
        SdeData::load_from_files(
            fixture_path("systems_small.csv"),
            fixture_path("stargates_small.csv"),
        )
        .expect("small SDE fixture should load")
    }

    #[test]
    fn system_lookup_by_exact_name() {
        let data = load_small_fixture();

        assert_eq!(data.system_id_by_name("Jita"), Some(30000142));
    }

    #[test]
    fn system_lookup_by_case_insensitive_name() {
        let data = load_small_fixture();

        assert_eq!(data.system_id_by_name("  jItA  "), Some(30000142));
    }

    #[test]
    fn duplicate_system_id_detection() {
        let error = SdeData::from_records(
            vec![
                SystemRecord {
                    system_id: 1,
                    system_name: "Alpha".to_string(),
                    security_status: 0.9,
                    region_id: 10,
                    constellation_id: 100,
                },
                SystemRecord {
                    system_id: 1,
                    system_name: "Beta".to_string(),
                    security_status: 0.8,
                    region_id: 20,
                    constellation_id: 200,
                },
            ],
            Vec::new(),
        )
        .expect_err("duplicate system IDs should be rejected");

        assert!(error.to_string().contains("duplicate system_id 1"));
    }

    #[test]
    fn missing_stargate_endpoint_is_skipped_and_reported() {
        let data = SdeData::load_from_files(
            fixture_path("systems_with_missing_edge.csv"),
            fixture_path("stargates_with_missing_endpoint.csv"),
        )
        .expect("missing stargate endpoints should not fail loading");

        assert_eq!(data.stargate_connections.len(), 1);
        assert_eq!(data.skipped_unknown_stargate_edges(), 2);
        assert_eq!(
            data.stargate_connections[0],
            StargateConnection {
                from_system_id: 30000142,
                to_system_id: 30000144,
            }
        );
    }

    #[test]
    fn loaded_fields_preserve_security_region_and_constellation() {
        let data = load_small_fixture();
        let system = data.system_by_name("Perimeter").expect("system exists");

        assert_eq!(system.security_status, 1.0);
        assert_eq!(system.region_id, 10000002);
        assert_eq!(system.constellation_id, 20000020);
    }
}
