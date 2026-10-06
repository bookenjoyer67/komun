//! Nominatim geocode proxy. Nominatim's usage policy requires at most one request per second, a
//! `User-Agent` naming the deployment with a contact, and no repeat queries — enforced by
//! [`limiter`], [`build_user_agent`] and [`cache`].

mod cache;
mod limiter;

use std::net::SocketAddr;
use std::sync::OnceLock;
use std::time::Duration;

use axum::{
    extract::{ConnectInfo, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use super::outbound;
use crate::rate_limit::RouteClass;
use crate::AppState;
use cache::GeocodeCache;
use limiter::RateLimiter;

const NOMINATIM_URL: &str = "https://nominatim.openstreetmap.org/search";

/// Nominatim's usage policy caps clients at one request per second.
const MIN_REQUEST_INTERVAL: Duration = Duration::from_secs(1);
/// Places change slowly; an hour keeps the cache useful without going stale.
const CACHE_TTL: Duration = Duration::from_secs(60 * 60);
/// Hard cap on cached queries so the map cannot be grown without bound.
const CACHE_CAPACITY: usize = 512;

const DISPLAY_NAME_MAX_CHARS: usize = 256;

/// The client sees only these; the upstream detail goes to the log, without the query.
const UPSTREAM_UNAVAILABLE: &str = "geocoding service unavailable";
const BUSY: &str = "geocoding busy, try again shortly";

/// Fallback header when no contact is configured; the version comes from the crate rather than
/// source.
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
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(params): Query<GeocodeParams>,
) -> Response {
    if let Err(limited) = super::per_ip_limit(&state, RouteClass::Geocode, peer, &headers) {
        return limited.into_response();
    }
    match resolve(&params.q, cache(), limiter(), fetch_nominatim).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => e.into_response(),
    }
}

/// Split from the handler so tests can exercise the limiter and cache with a fake upstream.
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

    limiter.acquire().await.map_err(|_| GeocodeError::busy())?;

    let value = fetch(query.to_string()).await?;
    cache.insert(key, value.clone()).await;
    Ok(value)
}

/// Cache key: case-insensitive and whitespace-collapsed, so trivial reformattings share an entry.
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

/// Redirects are refused: the upstream is fixed, so a redirect is either a misconfiguration or
/// an attempt to point this server somewhere else. The environment's proxy is honoured, because
/// the destination never varies.
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .expect("the geocode HTTP client needs only the TLS backend reqwest::Client::new uses")
    })
}

/// Process-wide `User-Agent`, resolved once: a contact change takes a restart.
fn user_agent() -> &'static str {
    static USER_AGENT: OnceLock<String> = OnceLock::new();
    USER_AGENT.get_or_init(|| build_user_agent(configured_contact().as_deref()))
}

/// Operator contact, preferring `KOMUN_GEOCODE_CONTACT` over `[geocode] contact`; a missing file
/// or key is fine.
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

/// Resolved through [`configured_contact`], as the `User-Agent` is: a check of the loaded `Config`
/// alone would miss `KOMUN_GEOCODE_CONTACT` and disagree with the header actually sent.
pub(crate) fn contact_is_missing() -> bool {
    missing_contact(configured_contact().as_deref())
}

fn missing_contact(contact: Option<&str>) -> bool {
    contact
        .map(str::trim)
        .filter(|contact| !contact.is_empty())
        .is_none()
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

/// Extracts `[geocode] contact`; unknown keys and a missing section are ignored, so a stale config
/// still loads.
fn parse_contact(contents: &str) -> Option<String> {
    toml::from_str::<ConfigFile>(contents)
        .ok()
        .and_then(|file| file.geocode.contact)
        .map(|contact| contact.trim().to_string())
        .filter(|contact| !contact.is_empty())
}

/// The `User-Agent` Nominatim requires; without a contact it falls back to the generic agent so an
/// unconfigured node still works.
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
    let res = client()
        .get(NOMINATIM_URL)
        .header("User-Agent", user_agent())
        .query(&[("q", query.as_str()), ("format", "json"), ("limit", "1")])
        .send()
        .await
        .map_err(|e| {
            tracing::warn!("geocode upstream unreachable: {}", e.without_url());
            GeocodeError::upstream()
        })?;

    let status = res.status().as_u16();
    let body = outbound::read_capped(res, outbound::JSON_BODY_CAP, false)
        .await
        .map_err(|e| {
            tracing::warn!("geocode upstream body unusable: {e}");
            GeocodeError::upstream()
        })?;

    parse_nominatim(status, &body)
}

/// Nominatim sends coordinates as strings, and the client tests them for truthiness before
/// parsing, so they are checked here but passed on unchanged.
#[derive(Deserialize)]
struct NominatimHit {
    lat: String,
    lon: String,
    display_name: String,
}

fn parse_nominatim(status: u16, body: &[u8]) -> Result<serde_json::Value, GeocodeError> {
    if !(200..300).contains(&status) {
        tracing::warn!("geocode upstream returned status {status}");
        return Err(GeocodeError::upstream());
    }

    // The serde_json error is logged by kind and position only: its message can quote the body.
    let hits: Vec<NominatimHit> = serde_json::from_slice(body).map_err(|e| {
        tracing::warn!(
            "geocode upstream response unparseable: {:?} at line {} column {}",
            e.classify(),
            e.line(),
            e.column()
        );
        GeocodeError::upstream()
    })?;

    let Some(hit) = hits.into_iter().next() else {
        return Err(GeocodeError::not_found("location not found"));
    };

    if coordinate(&hit.lat, 90.0).is_none() || coordinate(&hit.lon, 180.0).is_none() {
        tracing::warn!("geocode upstream returned coordinates out of range");
        return Err(GeocodeError::upstream());
    }

    let display_name: String = hit
        .display_name
        .chars()
        .take(DISPLAY_NAME_MAX_CHARS)
        .collect();
    Ok(serde_json::json!({
        "lat": hit.lat,
        "lon": hit.lon,
        "display_name": display_name,
    }))
}

fn coordinate(raw: &str, bound: f64) -> Option<f64> {
    raw.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && (-bound..=bound).contains(value))
}

#[derive(Debug)]
pub(crate) struct GeocodeError {
    status: StatusCode,
    message: String,
    retry_after_seconds: Option<u64>,
}

impl GeocodeError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
            retry_after_seconds: None,
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
            retry_after_seconds: None,
        }
    }

    fn upstream() -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: UPSTREAM_UNAVAILABLE.into(),
            retry_after_seconds: None,
        }
    }

    fn busy() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: BUSY.into(),
            retry_after_seconds: Some(limiter::MAX_BACKLOG.as_secs()),
        }
    }
}

impl IntoResponse for GeocodeError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response();
        if let Some(seconds) = self.retry_after_seconds {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
        }
        response
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
        assert_eq!(build_user_agent(None), DEFAULT_USER_AGENT);
        assert!(DEFAULT_USER_AGENT.starts_with("Komun/"));
        assert!(DEFAULT_USER_AGENT.contains("nominatim proxy"));
    }

    #[test]
    fn nonsense_contact_still_boots_clean() {
        // A bogus contact is not rejected during config parsing, so the process still starts.
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

    /// Blank counts as missing, as it does for the `User-Agent`: a warning that disagreed with the
    /// header actually sent would mislead the operator.
    #[test]
    fn startup_warns_exactly_when_no_contact_is_resolved() {
        assert!(missing_contact(None));
        assert!(missing_contact(Some("")));
        assert!(missing_contact(Some("   ")));
        assert!(!missing_contact(Some("ops@komun.example")));
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

    // The paused clock makes `Instant::now()` and the sleeps below read tokio's timer, so the
    // 50 ms checkpoint and 200 ms slot are exact.
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

    fn hit(lat: &str, lon: &str, display_name: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!([{
            "place_id": 1,
            "lat": lat,
            "lon": lon,
            "display_name": display_name,
            "address": { "city": "Oakland" },
        }]))
        .expect("serialise fixture")
    }

    /// T10
    #[test]
    fn nominatim_hits_are_typed_range_checked_and_truncated() {
        let value = parse_nominatim(200, &hit("37.8044", "-122.2712", "Oakland, California"))
            .expect("a well-formed hit");
        assert_eq!(value["lat"], "37.8044", "lat stays the upstream string");
        assert_eq!(value["lon"], "-122.2712", "lon stays the upstream string");
        assert_eq!(value["display_name"], "Oakland, California");
        let keys: Vec<&str> = value
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys.len(), 3, "only lat, lon and display_name: {keys:?}");

        for (lat, lon) in [("91", "0"), ("0", "181"), ("NaN", "0"), ("north", "0")] {
            let err = parse_nominatim(200, &hit(lat, lon, "x")).expect_err("out of range");
            assert_eq!(err.status, StatusCode::BAD_GATEWAY, "{lat},{lon}");
        }

        let long = "é".repeat(300);
        let value = parse_nominatim(200, &hit("1", "1", &long)).expect("a long name");
        let name = value["display_name"].as_str().expect("a string");
        assert_eq!(name.chars().count(), DISPLAY_NAME_MAX_CHARS);

        let err = parse_nominatim(200, b"[]").expect_err("no hits");
        assert_eq!(err.status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn an_oversize_nominatim_body_is_refused() {
        let response = axum::http::Response::builder()
            .status(200)
            .body(vec![b' '; outbound::JSON_BODY_CAP + 1])
            .expect("build response");
        let result = outbound::read_capped(
            reqwest::Response::from(response),
            outbound::JSON_BODY_CAP,
            false,
        )
        .await;
        assert!(result.is_err(), "a body over 64 KiB must be refused");
    }

    /// T11
    #[test]
    fn upstream_failures_reach_the_client_as_fixed_text() {
        for (status, body) in [(500, &b""[..]), (200, &b"garbage"[..])] {
            let err = parse_nominatim(status, body).expect_err("an upstream failure");
            assert_eq!(err.status, StatusCode::BAD_GATEWAY);
            assert_eq!(err.message, UPSTREAM_UNAVAILABLE);
            assert!(!err.message.contains("500"), "status leaked");
            assert!(!err.message.contains("expected"), "parser detail leaked");
        }
    }

    #[test]
    fn a_busy_limiter_is_a_503_with_retry_after() {
        let response = GeocodeError::busy().into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(response.headers().contains_key(header::RETRY_AFTER));
    }
}
