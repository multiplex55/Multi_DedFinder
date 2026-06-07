use crate::config::FilterConfig;
use crate::model::system::SolarSystem;

pub fn filter_candidates<'a>(
    systems: &'a [SolarSystem],
    config: &FilterConfig,
) -> Vec<&'a SolarSystem> {
    systems
        .iter()
        .filter(|system| !config.highsec_only || system.is_highsec_at(config.min_security_status))
        .collect()
}
