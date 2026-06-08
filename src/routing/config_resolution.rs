use std::collections::HashSet;

use anyhow::{bail, Result};

use crate::config::AppConfig;
use crate::data::sde::SdeData;
use crate::model::system::SolarSystem;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedAvoidance {
    pub system_ids: HashSet<i32>,
    pub region_ids: HashSet<i32>,
}

pub fn resolve_avoidance(config: &AppConfig, sde_data: &SdeData) -> Result<ResolvedAvoidance> {
    let system_ids = config
        .avoid
        .systems
        .iter()
        .filter_map(|name| sde_data.system_id_by_name(name))
        .collect();
    let region_ids = resolve_avoided_region_ids(config, sde_data)?;

    Ok(ResolvedAvoidance {
        system_ids,
        region_ids,
    })
}

pub fn resolve_avoided_region_ids(config: &AppConfig, sde_data: &SdeData) -> Result<HashSet<i32>> {
    let mut region_ids = config
        .avoid
        .region_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>();

    if config.avoid.regions.is_empty() {
        return Ok(region_ids);
    }

    if !sde_data.has_region_name_data() {
        bail!(
            "[avoid].regions contains region names, but no regions.csv or regions.json was found in the SDE data directory"
        );
    }

    for region_name in &config.avoid.regions {
        let Some(region_id) = sde_data.region_id_by_name(region_name) else {
            bail!("unknown avoided region name configured in [avoid].regions: {region_name:?}");
        };
        region_ids.insert(region_id);
    }

    Ok(region_ids)
}

pub fn apply_resolved_avoidance_to_config(config: &mut AppConfig, avoidance: &ResolvedAvoidance) {
    config.avoid.region_ids = avoidance.region_ids.iter().copied().collect();
    config.avoid.region_ids.sort_unstable();
    config.avoid.region_ids.dedup();
}

pub fn validate_start_not_avoided(
    config: &AppConfig,
    start_system: &SolarSystem,
    avoidance: &ResolvedAvoidance,
) -> Result<()> {
    if config
        .avoid
        .systems
        .iter()
        .any(|name| start_system.name.eq_ignore_ascii_case(name.trim()))
    {
        bail!(
            "start system {:?} is directly configured in [avoid].systems",
            start_system.name
        );
    }

    if avoidance.system_ids.contains(&start_system.id) {
        bail!(
            "start system {:?} (ID {}) is directly configured in [avoid].systems",
            start_system.name,
            start_system.id
        );
    }

    if avoidance.region_ids.contains(&start_system.region_id) {
        bail!(
            "start system {:?} (ID {}) is in avoided region ID {}",
            start_system.name,
            start_system.id,
            start_system.region_id
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::model::system::SolarSystem;

    fn fixture_path(file_name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/sde")
            .join(file_name)
    }

    fn sde_with_regions() -> SdeData {
        SdeData::load_from_files_with_optional_regions(
            fixture_path("systems_small.csv"),
            fixture_path("stargates_small.csv"),
            Some(fixture_path("regions_small.csv")),
        )
        .expect("SDE fixture should load")
    }

    fn sde_without_regions() -> SdeData {
        SdeData::load_from_files(
            fixture_path("systems_small.csv"),
            fixture_path("stargates_small.csv"),
        )
        .expect("SDE fixture should load")
    }

    #[test]
    fn region_names_resolve_when_region_data_exists() {
        let mut config = AppConfig::default();
        config.avoid.regions = vec!["Exordium".to_string()];

        let region_ids = resolve_avoided_region_ids(&config, &sde_with_regions()).unwrap();

        assert!(region_ids.contains(&10000027));
    }

    #[test]
    fn region_names_fail_clearly_without_region_data() {
        let mut config = AppConfig::default();
        config.avoid.regions = vec!["Exordium".to_string()];

        let error = resolve_avoided_region_ids(&config, &sde_without_regions()).unwrap_err();

        assert!(error.to_string().contains("regions.csv or regions.json"));
    }

    #[test]
    fn unknown_region_name_fails_clearly() {
        let mut config = AppConfig::default();
        config.avoid.regions = vec!["Missing Region".to_string()];

        let error = resolve_avoided_region_ids(&config, &sde_with_regions()).unwrap_err();

        assert!(error.to_string().contains("Missing Region"));
        assert!(error.to_string().contains("unknown avoided region name"));
    }

    #[test]
    fn start_system_in_avoided_region_fails_clearly() {
        let mut config = AppConfig::default();
        config.avoid.region_ids = vec![10000027];
        let avoidance = ResolvedAvoidance {
            system_ids: HashSet::new(),
            region_ids: HashSet::from([10000027]),
        };
        let start_system = SolarSystem {
            id: 42,
            name: "Start".to_string(),
            security_status: 0.9,
            region_id: 10000027,
            constellation_id: 1,
        };

        let error = validate_start_not_avoided(&config, &start_system, &avoidance).unwrap_err();

        assert!(error.to_string().contains("Start"));
        assert!(error.to_string().contains("avoided region ID 10000027"));
    }
}
