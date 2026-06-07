use chrono::{DateTime, Duration, Utc};

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
