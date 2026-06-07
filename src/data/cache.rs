use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::data::esi_activity::{EsiActivityClient, SystemActivity};

pub const DEFAULT_ACTIVITY_CACHE_PATH: &str = ".cache/eve-ded-route/activity.json";
pub const DEFAULT_ACTIVITY_CACHE_MINUTES: i64 = 15;

#[derive(Clone, Debug)]
pub struct CacheEntry<T> {
    pub value: T,
    pub fetched_at: DateTime<Utc>,
}

impl<T> CacheEntry<T> {
    pub fn is_fresh(&self, ttl_minutes: i64) -> bool {
        Utc::now() - self.fetched_at < Duration::minutes(ttl_minutes)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ActivityCacheFile {
    pub fetched_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub activity_by_system_id: HashMap<i32, SystemActivity>,
}

impl ActivityCacheFile {
    pub fn new(
        fetched_at: DateTime<Utc>,
        cache_duration: Duration,
        activity_by_system_id: HashMap<i32, SystemActivity>,
    ) -> Self {
        Self {
            fetched_at,
            expires_at: fetched_at + cache_duration,
            activity_by_system_id,
        }
    }

    pub fn is_fresh_at(&self, now: DateTime<Utc>) -> bool {
        now < self.expires_at
    }
}

#[derive(Clone, Debug)]
pub struct ActivityCacheOptions {
    pub path: PathBuf,
    pub cache_duration: Duration,
    pub allow_stale_on_fetch_error: bool,
}

impl ActivityCacheOptions {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            path: activity_cache_path(config),
            cache_duration: Duration::minutes(config.esi.activity_cache_minutes as i64),
            allow_stale_on_fetch_error: config.esi.allow_stale_activity_cache,
        }
    }
}

pub fn activity_cache_path(config: &AppConfig) -> PathBuf {
    config
        .esi
        .activity_cache_path
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_ACTIVITY_CACHE_PATH))
}

pub async fn load_system_activity(config: &AppConfig) -> Result<HashMap<i32, SystemActivity>> {
    let options = ActivityCacheOptions::from_config(config);
    let client = EsiActivityClient::new();
    load_system_activity_with_fetcher(&options, || async { client.fetch_activity().await }).await
}

pub async fn load_system_activity_with_fetcher<F, Fut>(
    options: &ActivityCacheOptions,
    fetch_activity: F,
) -> Result<HashMap<i32, SystemActivity>>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<HashMap<i32, SystemActivity>>>,
{
    let cached = read_activity_cache(&options.path)?;
    let now = Utc::now();

    if let Some(cache) = &cached {
        if cache.is_fresh_at(now) {
            return Ok(cache.activity_by_system_id.clone());
        }
    }

    match fetch_activity().await {
        Ok(activity_by_system_id) => {
            let cache = ActivityCacheFile::new(now, options.cache_duration, activity_by_system_id);
            write_activity_cache(&options.path, &cache)?;
            Ok(cache.activity_by_system_id)
        }
        Err(fetch_error) => {
            if options.allow_stale_on_fetch_error {
                if let Some(cache) = cached {
                    tracing::warn!(
                        cache_path = %options.path.display(),
                        error = %fetch_error,
                        "using stale ESI activity cache after refresh failed"
                    );
                    return Ok(cache.activity_by_system_id);
                }
            }

            Err(anyhow!(
                "failed to fetch ESI activity and no usable cache is available at {} (enable stale activity cache only if offline fallback is acceptable): {fetch_error:#}",
                options.path.display()
            ))
        }
    }
}

pub fn read_activity_cache(path: &Path) -> Result<Option<ActivityCacheFile>> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse activity cache {}", path.display()))
            .map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("failed to read activity cache {}", path.display()))
        }
    }
}

pub fn write_activity_cache(path: &Path, cache: &ActivityCacheFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create activity cache directory {}",
                parent.display()
            )
        })?;
    }

    let contents =
        serde_json::to_string_pretty(cache).context("failed to serialize activity cache")?;
    fs::write(path, contents)
        .with_context(|| format!("failed to write activity cache {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    fn activity(system_id: i32, jumps_last_hour: u32) -> SystemActivity {
        SystemActivity {
            system_id,
            jumps_last_hour,
            npc_kills_last_hour: 0,
            ship_kills_last_hour: 0,
            pod_kills_last_hour: 0,
        }
    }

    fn cache_path(test_name: &str) -> PathBuf {
        let unique = format!(
            "eve-ded-route-{test_name}-{}-{}.json",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap()
        );
        std::env::temp_dir().join(unique)
    }

    fn options(path: PathBuf, allow_stale_on_fetch_error: bool) -> ActivityCacheOptions {
        ActivityCacheOptions {
            path,
            cache_duration: Duration::minutes(DEFAULT_ACTIVITY_CACHE_MINUTES),
            allow_stale_on_fetch_error,
        }
    }

    #[tokio::test]
    async fn fresh_cache_is_used_without_fetching() {
        let path = cache_path("fresh");
        let mut cached_activity = HashMap::new();
        cached_activity.insert(30000142, activity(30000142, 42));
        let cache = ActivityCacheFile::new(Utc::now(), Duration::minutes(15), cached_activity);
        write_activity_cache(&path, &cache).expect("cache should write");

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_fetcher = Arc::clone(&calls);
        let loaded =
            load_system_activity_with_fetcher(&options(path.clone(), false), move || async move {
                calls_for_fetcher.fetch_add(1, Ordering::SeqCst);
                Ok(HashMap::new())
            })
            .await
            .expect("fresh cache should load");

        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(loaded.get(&30000142).unwrap().jumps_last_hour, 42);
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn expired_cache_triggers_fetch_path_and_rewrites_cache() {
        let path = cache_path("expired-fetch");
        let mut stale_activity = HashMap::new();
        stale_activity.insert(30000142, activity(30000142, 1));
        let cache = ActivityCacheFile::new(
            Utc::now() - Duration::minutes(30),
            Duration::minutes(15),
            stale_activity,
        );
        write_activity_cache(&path, &cache).expect("cache should write");

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_fetcher = Arc::clone(&calls);
        let loaded =
            load_system_activity_with_fetcher(&options(path.clone(), false), move || async move {
                calls_for_fetcher.fetch_add(1, Ordering::SeqCst);
                let mut fetched_activity = HashMap::new();
                fetched_activity.insert(30000142, activity(30000142, 99));
                Ok(fetched_activity)
            })
            .await
            .expect("expired cache should refresh");

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(loaded.get(&30000142).unwrap().jumps_last_hour, 99);
        assert_eq!(
            read_activity_cache(&path)
                .unwrap()
                .unwrap()
                .activity_by_system_id
                .get(&30000142)
                .unwrap()
                .jumps_last_hour,
            99
        );
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn stale_cache_is_rejected_unless_configured() {
        let path = cache_path("stale-reject");
        let mut stale_activity = HashMap::new();
        stale_activity.insert(30000142, activity(30000142, 1));
        let cache = ActivityCacheFile::new(
            Utc::now() - Duration::minutes(30),
            Duration::minutes(15),
            stale_activity,
        );
        write_activity_cache(&path, &cache).expect("cache should write");

        let result = load_system_activity_with_fetcher(&options(path.clone(), false), || async {
            Err(anyhow!("network unavailable"))
        })
        .await;

        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("failed to fetch ESI activity"));
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn stale_cache_is_used_when_explicitly_configured() {
        let path = cache_path("stale-allowed");
        let mut stale_activity = HashMap::new();
        stale_activity.insert(30000142, activity(30000142, 7));
        let cache = ActivityCacheFile::new(
            Utc::now() - Duration::minutes(30),
            Duration::minutes(15),
            stale_activity,
        );
        write_activity_cache(&path, &cache).expect("cache should write");

        let loaded = load_system_activity_with_fetcher(&options(path.clone(), true), || async {
            Err(anyhow!("network unavailable"))
        })
        .await
        .expect("stale cache should be allowed");

        assert_eq!(loaded.get(&30000142).unwrap().jumps_last_hour, 7);
        let _ = fs::remove_file(path);
    }
}
