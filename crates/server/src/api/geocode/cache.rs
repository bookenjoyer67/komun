//! Bounded, TTL'd result cache for geocode lookups.

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::Instant;

struct Entry {
    value: serde_json::Value,
    stored_at: Instant,
}

/// A fixed-capacity cache keyed on the normalised query. Entries older than `ttl` are misses, the
/// oldest is evicted once `capacity` is reached, and a hit never touches the network or a limiter
/// slot.
pub struct GeocodeCache {
    ttl: Duration,
    capacity: usize,
    entries: Mutex<HashMap<String, Entry>>,
}

impl GeocodeCache {
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            ttl,
            capacity: capacity.max(1),
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub async fn get(&self, key: &str) -> Option<serde_json::Value> {
        let mut entries = self.entries.lock().await;
        match entries.get(key) {
            Some(entry) if entry.stored_at.elapsed() < self.ttl => Some(entry.value.clone()),
            Some(_) => {
                entries.remove(key);
                None
            }
            None => None,
        }
    }

    pub async fn insert(&self, key: String, value: serde_json::Value) {
        let mut entries = self.entries.lock().await;

        if entries.len() >= self.capacity {
            let oldest = entries
                .iter()
                .min_by_key(|(_, entry)| entry.stored_at)
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                entries.remove(&oldest);
            }
        }

        entries.insert(
            key,
            Entry {
                value,
                stored_at: Instant::now(),
            },
        );
    }

    #[cfg(test)]
    pub async fn len(&self) -> usize {
        self.entries.lock().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cache_is_bounded() {
        let cache = GeocodeCache::new(Duration::from_secs(60), 2);
        for i in 0..5 {
            cache
                .insert(format!("query-{i}"), serde_json::json!({ "i": i }))
                .await;
        }
        assert!(cache.len().await <= 2);
    }

    #[tokio::test]
    async fn expired_entries_are_misses() {
        let cache = GeocodeCache::new(Duration::from_millis(10), 4);
        cache
            .insert("oakland".into(), serde_json::json!({ "lat": "1" }))
            .await;
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert!(cache.get("oakland").await.is_none());
    }
}
