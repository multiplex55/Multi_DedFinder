use std::collections::{BTreeSet, HashMap};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const ESI_BASE_URL: &str = "https://esi.evetech.net/latest";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SystemActivity {
    pub system_id: i32,
    pub jumps_last_hour: u32,
    pub npc_kills_last_hour: u32,
    pub ship_kills_last_hour: u32,
    pub pod_kills_last_hour: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SystemJumps {
    pub system_id: i32,
    pub ship_jumps: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SystemKills {
    pub system_id: i32,
    pub npc_kills: u32,
    pub ship_kills: u32,
    pub pod_kills: u32,
}

#[derive(Clone, Debug)]
pub struct EsiActivityClient {
    http: reqwest::Client,
    base_url: String,
}

impl Default for EsiActivityClient {
    fn default() -> Self {
        Self::new()
    }
}

impl EsiActivityClient {
    pub fn new() -> Self {
        Self::with_base_url(ESI_BASE_URL)
    }

    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    pub async fn fetch_system_jumps(&self) -> Result<Vec<SystemJumps>> {
        let url = format!("{}/universe/system_jumps/", self.base_url);
        self.http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("failed to request ESI system jumps from {url}"))?
            .error_for_status()
            .with_context(|| format!("ESI system jumps request failed for {url}"))?
            .json::<Vec<SystemJumps>>()
            .await
            .context("failed to decode ESI system jumps response")
    }

    pub async fn fetch_system_kills(&self) -> Result<Vec<SystemKills>> {
        let url = format!("{}/universe/system_kills/", self.base_url);
        self.http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("failed to request ESI system kills from {url}"))?
            .error_for_status()
            .with_context(|| format!("ESI system kills request failed for {url}"))?
            .json::<Vec<SystemKills>>()
            .await
            .context("failed to decode ESI system kills response")
    }

    pub async fn fetch_activity(&self) -> Result<HashMap<i32, SystemActivity>> {
        let jumps = self.fetch_system_jumps().await?;
        let kills = self.fetch_system_kills().await?;
        Ok(merge_activity(&jumps, &kills))
    }
}

pub fn map_system_jumps_json(contents: &str) -> Result<Vec<SystemJumps>> {
    serde_json::from_str(contents).context("failed to parse ESI system jumps JSON")
}

pub fn map_system_kills_json(contents: &str) -> Result<Vec<SystemKills>> {
    serde_json::from_str(contents).context("failed to parse ESI system kills JSON")
}

pub fn merge_activity(
    jumps: &[SystemJumps],
    kills: &[SystemKills],
) -> HashMap<i32, SystemActivity> {
    let jumps_by_system_id: HashMap<i32, u32> = jumps
        .iter()
        .map(|jumps| (jumps.system_id, jumps.ship_jumps))
        .collect();
    let kills_by_system_id: HashMap<i32, &SystemKills> =
        kills.iter().map(|kills| (kills.system_id, kills)).collect();

    let system_ids: BTreeSet<i32> = jumps_by_system_id
        .keys()
        .chain(kills_by_system_id.keys())
        .copied()
        .collect();

    system_ids
        .into_iter()
        .map(|system_id| {
            let kills = kills_by_system_id.get(&system_id).copied();
            (
                system_id,
                SystemActivity {
                    system_id,
                    jumps_last_hour: jumps_by_system_id.get(&system_id).copied().unwrap_or(0),
                    npc_kills_last_hour: kills.map(|kills| kills.npc_kills).unwrap_or(0),
                    ship_kills_last_hour: kills.map(|kills| kills.ship_kills).unwrap_or(0),
                    pod_kills_last_hour: kills.map(|kills| kills.pod_kills).unwrap_or(0),
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const JUMPS_JSON: &str = r#"
[
  {"system_id": 30000142, "ship_jumps": 123},
  {"system_id": 30000144, "ship_jumps": 7}
]
"#;

    const KILLS_JSON: &str = r#"
[
  {"system_id": 30000142, "npc_kills": 11, "ship_kills": 2, "pod_kills": 1},
  {"system_id": 30000145, "npc_kills": 5, "ship_kills": 0, "pod_kills": 0}
]
"#;

    #[test]
    fn maps_system_jumps_response() {
        let jumps = map_system_jumps_json(JUMPS_JSON).expect("jumps fixture should parse");

        assert_eq!(
            jumps,
            vec![
                SystemJumps {
                    system_id: 30000142,
                    ship_jumps: 123,
                },
                SystemJumps {
                    system_id: 30000144,
                    ship_jumps: 7,
                },
            ]
        );
    }

    #[test]
    fn maps_system_kills_response() {
        let kills = map_system_kills_json(KILLS_JSON).expect("kills fixture should parse");

        assert_eq!(
            kills,
            vec![
                SystemKills {
                    system_id: 30000142,
                    npc_kills: 11,
                    ship_kills: 2,
                    pod_kills: 1,
                },
                SystemKills {
                    system_id: 30000145,
                    npc_kills: 5,
                    ship_kills: 0,
                    pod_kills: 0,
                },
            ]
        );
    }

    #[test]
    fn merges_jump_and_kill_activity_by_system_id() {
        let activity = merge_activity(
            &[SystemJumps {
                system_id: 30000142,
                ship_jumps: 123,
            }],
            &[SystemKills {
                system_id: 30000142,
                npc_kills: 11,
                ship_kills: 2,
                pod_kills: 1,
            }],
        );

        assert_eq!(
            activity.get(&30000142),
            Some(&SystemActivity {
                system_id: 30000142,
                jumps_last_hour: 123,
                npc_kills_last_hour: 11,
                ship_kills_last_hour: 2,
                pod_kills_last_hour: 1,
            })
        );
    }

    #[test]
    fn missing_counts_default_to_zero() {
        let jumps = map_system_jumps_json(JUMPS_JSON).expect("jumps fixture should parse");
        let kills = map_system_kills_json(KILLS_JSON).expect("kills fixture should parse");
        let activity = merge_activity(&jumps, &kills);

        assert_eq!(activity.get(&30000144).unwrap().npc_kills_last_hour, 0);
        assert_eq!(activity.get(&30000144).unwrap().ship_kills_last_hour, 0);
        assert_eq!(activity.get(&30000144).unwrap().pod_kills_last_hour, 0);
        assert_eq!(activity.get(&30000145).unwrap().jumps_last_hour, 0);
    }
}
