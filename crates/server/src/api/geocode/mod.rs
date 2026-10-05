//! Nominatim geocode proxy.
//!
//! The endpoint is small, but Nominatim's usage policy makes three things
//! mandatory: at most one request per second, a `User-Agent` that names the
//! deployment and gives a contact, and no repeat hammering of identical
//! queries. The process-wide [`limiter`] and [`cache`] below enforce the first
//! and third; [`build_user_agent`] builds the header for the second.

mod cache;
mod limiter;

use std::sync::OnceLock;
use std::time::Duration;

use axum::{
    extract::Query,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use cache::GeocodeCache;
use limiter::RateLimiter;

const NOMINATIM_URL: &str = "https://nominatim.openstreetmap.org/search";

/// Nominatim's usage policy caps clients at one request per second.
const MIN_REQUEST_INTERVAL: Duration = Duration::from_secs(1);
/// Places change slowly; an hour keeps the cache useful without going stale.
const CACHE_TTL: Duration = Duration::from_secs(60 * 60);
/// Hard cap on cached queries so the map cannot be grown without bound.
const CACHE_CAPACITY: usize = 512;

/// The header used before an operator configures a contact. It identifies the
/// deployment the same way the old hardcoded value did, with the version taken
/// from the crate rather than pinned in source.
const DEFAULT_USER_AGENT: &str = concat!(
    "Komun/",
    env!("CARGO_PKG_VERSION"),
    " (nominatim proxy; local-listings app)"
);

#[derive(Deserialize)]
pub struct GeocodeParams {
    q: String,
}

pub async fn geocode(
    Query(params): Query<GeocodeParams>,
) -> Result<Json<serde_json::Value>, GeocodeError> {
    let value = resolve(&params.q, cache(), limiter(), fetch_nominatim).await?;
    Ok(Json(value))
}

/// Core lookup path, split out from the handler so the limiter and cache can be
/// exercised with a fake upstream in tests.
async fn resolve<F, Fut>(
    raw_query: &str,
    cache: &GeocodeCache,
    limiter: &RateLimiter,
    fetch: F,
) -> Result<serde_json::Value, GeocodeError>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<serde_json::Value, GeocodeError>>,
{
    let query = raw_query.trim();
    if query.is_empty() {
        return Err(GeocodeError::bad_request("q parameter is required"));
    }

    let key = normalize_query(query);
    if let Some(hit) = cache.get(&key).await {
        return Ok(hit);
    }

    // Queue behind the process-wide limiter; this awaits rather than dropping.
    limiter.acquire().await;

    let value = fetch(query.to_string()).await?;
    cache.insert(key, value.clone()).await;
    Ok(value)
}

/// Cache key: case-insensitive and whitespace-collapsed, so trivial
/// reformattings of the same place share an entry.
fn normalize_query(query: &str) -> String {
    query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn limiter() -> &'static RateLimiter {
    static LIMITER: OnceLock<RateLimiter> = OnceLock::new();
    LIMITER.get_or_init(|| RateLimiter::new(MIN_REQUEST_INTERVAL))
}

fn cache() -> &'static GeocodeCache {
    static CACHE: OnceLock<GeocodeCache> = OnceLock::new();
    CACHE.get_or_init(|| GeocodeCache::new(CACHE_TTL, CACHE_CAPACITY))
}

/// Process-wide `User-Agent`, resolved once at first use.
///
/// The canonical `[geocode] contact` field is requested for `config.rs` via
/// `.dispatch/hub-requests/B1.md`. The route is mounted without `AppState` (no
/// edit to `api/mod.rs` is allowed), so this module reads the same section from
/// the config file itself, and treats a missing section as "unset".
fn user_agent() -> &'static str {
    static USER_AGENT: OnceLock<String> = OnceLock::new();
    USER_AGENT.get_or_init(|| build_user_agent(configured_contact().as_deref()))
}

/// Reads the operator contact, preferring `KOMUN_GEOCODE_CONTACT` over the
/// `[geocode] contact` key in the config file. A missing file or key is fine.
fn configured_contact() -> Option<String> {
    if let Ok(value) = std::env::var("KOMUN_GEOCODE_CONTACT") {
        let value = value.trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }

    let path = std::env::var("KOMUN_CONFIG").unwrap_or_else(|_| "config.toml".to_string());
    let contents = std::fs::read_to_string(path).ok()?;
    parse_contact(&contents)
}

#[derive(Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    geocode: GeocodeSection,
}

#[derive(Deserialize, Default)]
struct GeocodeSection {
    contact: Option<String>,
}

/// Extracts `[geocode] contact` from a full config file. Unknown keys and a
/// missing section are ignored, so a stale config still loads.
fn parse_contact(contents: &str) -> Option<String> {
    toml::from_str::<ConfigFile>(contents)
        .ok()
        .and_then(|file| file.geocode.contact)
        .map(|contact| contact.trim().to_string())
        .filter(|contact| !contact.is_empty())
}

/// Builds the `User-Agent` Nominatim requires: an identifier plus a contact.
/// Without a contact it falls back to the historical generic agent so an
/// unconfigured node keeps working.
fn build_user_agent(contact: Option<&str>) -> String {
    match contact.map(str::trim).filter(|contact| !contact.is_empty()) {
        Some(contact) => format!(
            "Komun/{} (+{}; nominatim proxy)",
            env!("CARGO_PKG_VERSION"),
            contact
        ),
        None => DEFAULT_USER_AGENT.to_string(),
    }
}

async fn fetch_nominatim(query: String) -> Result<serde_json::Value, GeocodeError> {
    let client = reqwest::Client::new();
    let res = client
        .get(NOMINATIM_URL)
        .header("User-Agent", user_agent())
        .query(&[("q", query.as_str()), ("format", "json"), ("limit", "1")])
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| GeocodeError::bad_gateway(format!("geocoding service unreachable: {}", e)))?;

    if !res.status().is_success() {
        return Err(GeocodeError::bad_gateway(format!(
            "geocoding service returned {}",
            res.status()
        )));
    }

    let results: Vec<serde_json::Value> = res.json().await.map_err(|e| {
        GeocodeError::bad_gateway(format!("failed to parse geocoding response: {}", e))
    })?;

    match results.first() {
        Some(result) => Ok(serde_json::json!({
            "lat": result["lat"],
            "lon": result["lon"],
            "display_name": result["display_name"],
        })),
        None => Err(GeocodeError::not_found("location not found")),
    }
}

#[derive(Debug)]
pub(crate) struct GeocodeError {
    status: axum::http::StatusCode,
    message: String,
}

impl GeocodeError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: axum::http::StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn bad_gateway(message: impl Into<String>) -> Self {
        Self {
            status: axum::http::StatusCode::BAD_GATEWAY,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: axum::http::StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }
}

impl IntoResponse for GeocodeError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::time::Instant;

    #[test]
    fn normalize_query_collapses_case_and_whitespace() {
        assert_eq!(normalize_query("  Oakland,   CA  "), "oakland, ca");
        assert_eq!(normalize_query("Oakland,\nCA"), "oakland, ca");
    }

    #[test]
    fn user_agent_includes_configured_contact() {
        let ua = build_user_agent(Some("ops@komun.example"));
        assert!(ua.contains("ops@komun.example"), "User-Agent was {ua:?}");
        assert!(ua.contains("Komun/"), "User-Agent was {ua:?}");
    }

    #[test]
    fn user_agent_default_keeps_existing_behaviour() {
        // The `[geocode] contact` config field is pending (hub request B1), so
        // assert against the default value explicitly: that is what a node
        // without the section actually sends today. It no longer pins a version
        // literal, so the source carries no hardcoded agent string.
        assert_eq!(build_user_agent(None), DEFAULT_USER_AGENT);
        assert!(DEFAULT_USER_AGENT.starts_with("Komun/"));
        assert!(DEFAULT_USER_AGENT.contains("nominatim proxy"));
    }

    #[test]
    fn nonsense_contact_still_boots_clean() {
        // A bogus contact is not rejected during config parsing, so the process
        // still starts and simply sends the bogus header.
        let config = "[geocode]\ncontact = \"not a real address!!!\"\n";
        let contact = parse_contact(config);
        assert_eq!(contact.as_deref(), Some("not a real address!!!"));
        let ua = build_user_agent(contact.as_deref());
        assert!(
            ua.contains("not a real address!!!"),
            "User-Agent was {ua:?}"
        );
    }

    #[test]
    fn blank_or_missing_contact_falls_back() {
        assert_eq!(parse_contact("[geocode]\ncontact = \"   \"\n"), None);
        assert_eq!(parse_contact("[node]\nname = \"x\"\n"), None);
        assert!(build_user_agent(Some("   ")).contains("nominatim proxy"));
    }

    #[tokio::test]
    async fn cache_hit_serves_without_upstream_call() {
        let cache = GeocodeCache::new(Duration::from_secs(60), 8);
        let limiter = RateLimiter::new(Duration::from_millis(1));
        let calls = Arc::new(AtomicUsize::new(0));

        let counting_fetch = || {
            let calls = calls.clone();
            move |q: String| {
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(serde_json::json!({ "display_name": q }))
                }
            }
        };

        let first = resolve("Oakland", &cache, &limiter, counting_fetch())
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(first["display_name"], "Oakland");

        // Same place, reformatted: normalisation makes the second a cache hit.
        let second = resolve("  oakland ", &cache, &limiter, counting_fetch())
            .await
            .unwrap();
        assert_eq!(second["display_name"], "Oakland");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "a cache hit must not call upstream"
        );
    }

    #[tokio::test]
    async fn empty_query_is_rejected_before_any_upstream_call() {
        let cache = GeocodeCache::new(Duration::from_secs(60), 8);
        let limiter = RateLimiter::new(Duration::from_millis(1));
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_fetch = calls.clone();

        let result = resolve("   ", &cache, &limiter, move |q: String| {
            let calls = calls_for_fetch.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(serde_json::json!({ "display_name": q }))
            }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    // The clock is paused, so `Instant::now()` and both sleeps below read
    // tokio's timer rather than the wall clock. The 50 ms checkpoint and the
    // 200 ms slot are then exact, instead of being eaten into by the first
    // lookup's own latency. `acquire()` is already on that timer and is
    // unchanged.
    #[tokio::test(start_paused = true)]
    async fn limiter_queues_second_lookup_instead_of_firing_both() {
        let cache = Arc::new(GeocodeCache::new(Duration::from_secs(60), 8));
        let limiter = Arc::new(RateLimiter::new(Duration::from_millis(200)));
        let calls = Arc::new(AtomicUsize::new(0));
        let start = Instant::now();

        let spawn = |query: &'static str| {
            let cache = cache.clone();
            let limiter = limiter.clone();
            let calls = calls.clone();
            tokio::spawn(async move {
                resolve(query, &cache, &limiter, move |q: String| {
                    let calls = calls.clone();
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(serde_json::json!({ "display_name": q }))
                    }
                })
                .await
                .map(|_| Instant::now())
            })
        };

        let first = spawn("alpha");
        let second = spawn("beta");

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the second lookup reached upstream before its one-per-second slot"
        );

        let a = first.await.unwrap().unwrap();
        let b = second.await.unwrap().unwrap();
        let (earliest, latest) = if a <= b { (a, b) } else { (b, a) };

        assert!(
            latest.duration_since(earliest) >= Duration::from_millis(200),
            "queued lookup fired too early ({earliest:?} -> {latest:?})"
        );
        assert!(latest.duration_since(start) >= Duration::from_millis(200));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "the queued lookup must still run"
        );
    }
}
