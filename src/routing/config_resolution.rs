use std::collections::HashSet;

use anyhow::{bail, Result};

use crate::config::{AppConfig, FactionExcludeBehavior, FactionSpaceBehavior};
use crate::data::sde::SdeData;
use crate::model::system::SolarSystem;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedFactionSpace {
    pub preferred_region_ids: HashSet<i32>,
    pub excluded_candidate_only_region_ids: HashSet<i32>,
    pub excluded_hard_exclude_region_ids: HashSet<i32>,
}

impl ResolvedFactionSpace {
    pub fn excluded_graph_region_ids(&self) -> &HashSet<i32> {
        &self.excluded_hard_exclude_region_ids
    }
}

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

pub fn resolve_faction_space(
    config: &AppConfig,
    sde_data: &SdeData,
) -> Result<ResolvedFactionSpace> {
    validate_configured_faction_names(config)?;

    let preferred_region_ids = resolve_named_faction_regions(
        &config.faction_space.preferred_factions,
        config,
        sde_data,
        "preferred_factions",
    )?;
    let excluded_region_ids = resolve_named_faction_regions(
        &config.faction_space.excluded_factions,
        config,
        sde_data,
        "excluded_factions",
    )?;

    if matches!(
        config.faction_space.behavior,
        FactionSpaceBehavior::HardInclude | FactionSpaceBehavior::SoftBonus
    ) && preferred_region_ids.is_empty()
    {
        bail!(
            "[faction_space] behavior {:?} is enabled but no resolved faction regions were found from preferred_factions",
            config.faction_space.behavior
        );
    }

    if matches!(
        config.faction_space.exclude_behavior,
        FactionExcludeBehavior::CandidateOnly | FactionExcludeBehavior::HardExclude
    ) && excluded_region_ids.is_empty()
    {
        bail!(
            "[faction_space] exclude_behavior {:?} is enabled but no resolved faction regions were found from excluded_factions",
            config.faction_space.exclude_behavior
        );
    }

    let (excluded_candidate_only_region_ids, excluded_hard_exclude_region_ids) =
        match config.faction_space.exclude_behavior {
            FactionExcludeBehavior::Disabled => (HashSet::new(), HashSet::new()),
            FactionExcludeBehavior::CandidateOnly => (excluded_region_ids, HashSet::new()),
            FactionExcludeBehavior::HardExclude => (HashSet::new(), excluded_region_ids),
        };

    Ok(ResolvedFactionSpace {
        preferred_region_ids,
        excluded_candidate_only_region_ids,
        excluded_hard_exclude_region_ids,
    })
}

fn validate_configured_faction_names(config: &AppConfig) -> Result<()> {
    for faction_name in &config.faction_space.preferred_factions {
        if !config.faction_space.factions.contains_key(faction_name) {
            bail!(
                "unknown faction name in [faction_space].preferred_factions: {faction_name:?}; define it under [faction_space.factions]"
            );
        }
    }

    for faction_name in &config.faction_space.excluded_factions {
        if !config.faction_space.factions.contains_key(faction_name) {
            bail!(
                "unknown faction name in [faction_space].excluded_factions: {faction_name:?}; define it under [faction_space.factions]"
            );
        }
    }

    Ok(())
}

fn resolve_named_faction_regions(
    faction_names: &[String],
    config: &AppConfig,
    sde_data: &SdeData,
    list_name: &str,
) -> Result<HashSet<i32>> {
    let mut region_ids = HashSet::new();

    for faction_name in faction_names {
        let faction = config
            .faction_space
            .factions
            .get(faction_name)
            .expect("faction names should be validated before resolving regions");

        region_ids.extend(faction.region_ids.iter().copied());

        if faction.regions.is_empty() {
            continue;
        }

        if !sde_data.has_region_name_data() {
            bail!(
                "[faction_space.factions.{faction_name}].regions contains region names from {list_name}, but no regions.csv or regions.json was found in the SDE data directory"
            );
        }

        for region_name in &faction.regions {
            let Some(region_id) = sde_data.region_id_by_name(region_name) else {
                bail!(
                    "unknown region name configured for faction {faction_name:?} in [faction_space.factions]: {region_name:?}"
                );
            };
            region_ids.insert(region_id);
        }
    }

    Ok(region_ids)
}

pub fn apply_resolved_faction_space_to_config(
    config: &mut AppConfig,
    faction_space: &ResolvedFactionSpace,
) {
    config.faction_space.resolved_preferred_region_ids =
        sorted_ids(&faction_space.preferred_region_ids);
    config
        .faction_space
        .resolved_excluded_candidate_only_region_ids =
        sorted_ids(&faction_space.excluded_candidate_only_region_ids);
    config
        .faction_space
        .resolved_excluded_hard_exclude_region_ids =
        sorted_ids(&faction_space.excluded_hard_exclude_region_ids);
}

fn sorted_ids(ids: &HashSet<i32>) -> Vec<i32> {
    let mut ids = ids.iter().copied().collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    ids
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
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::config::{FactionRegionConfig, FactionSpaceConfig};
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

    fn faction_config(
        behavior: FactionSpaceBehavior,
        exclude_behavior: FactionExcludeBehavior,
    ) -> AppConfig {
        AppConfig {
            faction_space: FactionSpaceConfig {
                behavior,
                exclude_behavior,
                preferred_factions: vec!["Federation".to_string()],
                excluded_factions: vec!["Empire".to_string()],
                factions: HashMap::from([
                    (
                        "Federation".to_string(),
                        FactionRegionConfig {
                            regions: Vec::new(),
                            region_ids: vec![10000002],
                        },
                    ),
                    (
                        "Empire".to_string(),
                        FactionRegionConfig {
                            regions: Vec::new(),
                            region_ids: vec![10000043],
                        },
                    ),
                ]),
                ..FactionSpaceConfig::default()
            },
            ..AppConfig::default()
        }
    }

    fn sde_without_regions() -> SdeData {
        SdeData::load_from_files(
            fixture_path("systems_small.csv"),
            fixture_path("stargates_small.csv"),
        )
        .expect("SDE fixture should load")
    }

    #[test]
    fn unknown_preferred_faction_errors_clearly() {
        let mut config = faction_config(
            FactionSpaceBehavior::HardInclude,
            FactionExcludeBehavior::Disabled,
        );
        config.faction_space.preferred_factions = vec!["MissingFaction".to_string()];

        let error = resolve_faction_space(&config, &sde_without_regions()).unwrap_err();

        assert!(error.to_string().contains("MissingFaction"));
        assert!(error.to_string().contains("preferred_factions"));
    }

    #[test]
    fn unknown_excluded_faction_errors_clearly() {
        let mut config = faction_config(
            FactionSpaceBehavior::Disabled,
            FactionExcludeBehavior::HardExclude,
        );
        config.faction_space.excluded_factions = vec!["MissingFaction".to_string()];

        let error = resolve_faction_space(&config, &sde_without_regions()).unwrap_err();

        assert!(error.to_string().contains("MissingFaction"));
        assert!(error.to_string().contains("excluded_factions"));
    }

    #[test]
    fn faction_region_names_require_region_data() {
        let mut config = faction_config(
            FactionSpaceBehavior::HardInclude,
            FactionExcludeBehavior::Disabled,
        );
        config
            .faction_space
            .factions
            .get_mut("Federation")
            .unwrap()
            .regions = vec!["The Forge".to_string()];
        config
            .faction_space
            .factions
            .get_mut("Federation")
            .unwrap()
            .region_ids = Vec::new();

        let error = resolve_faction_space(&config, &sde_without_regions()).unwrap_err();

        assert!(error.to_string().contains("regions.csv or regions.json"));
    }

    #[test]
    fn faction_region_ids_work_without_region_data() {
        let config = faction_config(
            FactionSpaceBehavior::HardInclude,
            FactionExcludeBehavior::HardExclude,
        );

        let resolved = resolve_faction_space(&config, &sde_without_regions()).unwrap();

        assert_eq!(resolved.preferred_region_ids, HashSet::from([10000002]));
        assert_eq!(
            resolved.excluded_hard_exclude_region_ids,
            HashSet::from([10000043])
        );
    }

    #[test]
    fn faction_region_names_resolve_with_region_data() {
        let mut config = faction_config(
            FactionSpaceBehavior::HardInclude,
            FactionExcludeBehavior::Disabled,
        );
        config
            .faction_space
            .factions
            .get_mut("Federation")
            .unwrap()
            .regions = vec!["Exordium".to_string()];
        config
            .faction_space
            .factions
            .get_mut("Federation")
            .unwrap()
            .region_ids = Vec::new();

        let resolved = resolve_faction_space(&config, &sde_with_regions()).unwrap();

        assert_eq!(resolved.preferred_region_ids, HashSet::from([10000027]));
    }

    #[test]
    fn empty_enabled_faction_mappings_error_instead_of_filtering_everything() {
        let mut config = faction_config(
            FactionSpaceBehavior::HardInclude,
            FactionExcludeBehavior::Disabled,
        );
        config
            .faction_space
            .factions
            .get_mut("Federation")
            .unwrap()
            .region_ids = Vec::new();

        let error = resolve_faction_space(&config, &sde_without_regions()).unwrap_err();

        assert!(error.to_string().contains("no resolved faction regions"));
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
