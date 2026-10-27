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
use crate::config::NodeConfig;
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
const PLACE_PART_MAX_CHARS: usize = 100;

const MAX_QUERY_CHARS: usize = 200;

/// About 55 km north to south: a metro area, not a region.
const VIEWBOX_HALF_DEG: f64 = 0.5;

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

type UpstreamParams = Vec<(&'static str, String)>;

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
    let centre = node_centre(&state.config.node);
    match resolve(&params.q, centre, cache(), limiter(), fetch_nominatim).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(Clone, Copy)]
struct Centre {
    lat: f64,
    lon: f64,
}

/// A partial or out-of-range location yields no centre: a search is never biased towards an
/// invented point.
fn node_centre(node: &NodeConfig) -> Option<Centre> {
    let lat = node.location_lat.filter(|lat| in_range(*lat, 90.0))?;
    let lon = node.location_lon.filter(|lon| in_range(*lon, 180.0))?;
    Some(Centre { lat, lon })
}

/// Nominatim reads the box as lon,lat,lon,lat.
fn viewbox(centre: Centre) -> String {
    let west = (centre.lon - VIEWBOX_HALF_DEG).max(-180.0);
    let south = (centre.lat - VIEWBOX_HALF_DEG).max(-90.0);
    let east = (centre.lon + VIEWBOX_HALF_DEG).min(180.0);
    let north = (centre.lat + VIEWBOX_HALF_DEG).min(90.0);
    format!("{west:.4},{south:.4},{east:.4},{north:.4}")
}

fn is_qualified(query: &str) -> bool {
    query.contains(',')
}

fn upstream_params(query: &str, bounding_box: Option<&str>) -> UpstreamParams {
    let mut params = vec![
        ("q", query.to_string()),
        ("format", "json".to_string()),
        ("limit", "1".to_string()),
        ("addressdetails", "1".to_string()),
    ];
    if let Some(bounding_box) = bounding_box {
        params.push(("viewbox", bounding_box.to_string()));
        params.push(("bounded", "1".to_string()));
    }
    params
}

/// Split from the handler so tests can exercise the limiter and cache with a fake upstream.
async fn resolve<F, Fut>(
    raw_query: &str,
    centre: Option<Centre>,
    cache: &GeocodeCache,
    limiter: &RateLimiter,
    fetch: F,
) -> Result<serde_json::Value, GeocodeError>
where
    F: FnOnce(UpstreamParams) -> Fut,
    Fut: std::future::Future<Output = Result<serde_json::Value, GeocodeError>>,
{
    let query = raw_query.trim();
    if query.is_empty() {
        return Err(GeocodeError::bad_request("q parameter is required"));
    }
    // Before the cache and the upstream, so an over-long query takes neither a cache slot nor
    // this server's one request per second.
    let length = query.chars().count();
    if length > MAX_QUERY_CHARS {
        return Err(GeocodeError::bad_request(format!(
            "q must be {MAX_QUERY_CHARS} characters or fewer (got {length})"
        )));
    }

    // `bounded=1` drops every hit outside the box, so a query that names its own region
    // ("Austin, TX") must go out unbiased or it would miss.
    let bias = centre.filter(|_| !is_qualified(query)).map(viewbox);
    // Keyed by the box actually sent: a moved node misses, and an unbiased query shares one entry.
    let key = format!("{}|{}", bias.as_deref().unwrap_or("-"), normalize_query(query));
    if let Some(hit) = cache.get(&key).await {
        return Ok(hit);
    }

    limiter.acquire().await.map_err(|_| GeocodeError::busy())?;

    let value = fetch(upstream_params(query, bias.as_deref())).await?;
    cache.insert(key, value.clone()).await;
    Ok(value)
}

/// Case-insensitive and whitespace-collapsed, so trivial reformattings share a cache entry.
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

async fn fetch_nominatim(params: UpstreamParams) -> Result<serde_json::Value, GeocodeError> {
    let res = client()
        .get(NOMINATIM_URL)
        .header("User-Agent", user_agent())
        .query(&params)
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
    #[serde(default)]
    address: Option<NominatimAddress>,
}

#[derive(Deserialize, Default)]
struct NominatimAddress {
    city: Option<String>,
    town: Option<String>,
    village: Option<String>,
    hamlet: Option<String>,
    state: Option<String>,
    #[serde(rename = "ISO3166-2-lvl4")]
    iso_subdivision: Option<String>,
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
    let address = hit.address.unwrap_or_default();
    let city = [address.city, address.town, address.village, address.hamlet]
        .into_iter()
        .find_map(place_part);
    let state = address
        .iso_subdivision
        .as_deref()
        .and_then(us_postal_code)
        .map(String::from)
        .or_else(|| place_part(address.state));
    Ok(serde_json::json!({
        "lat": hit.lat,
        "lon": hit.lon,
        "display_name": display_name,
        "city": city,
        "state": state,
    }))
}

fn place_part(value: Option<String>) -> Option<String> {
    let value = value?;
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    Some(value.chars().take(PLACE_PART_MAX_CHARS).collect())
}

/// Nominatim spells the state out and carries the two-letter postal code only inside the US
/// ISO 3166-2 code, so anything else keeps the full name.
fn us_postal_code(iso: &str) -> Option<&str> {
    let code = iso.strip_prefix("US-")?;
    let is_postal = code.len() == 2 && code.bytes().all(|b| b.is_ascii_uppercase());
    is_postal.then_some(code)
}

fn coordinate(raw: &str, bound: f64) -> Option<f64> {
    raw.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| in_range(*value, bound))
}

fn in_range(value: f64, bound: f64) -> bool {
    value.is_finite() && (-bound..=bound).contains(&value)
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
    use crate::config::NodeConfig;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::time::Instant;

    type Params = Vec<(&'static str, String)>;

    const ST_LOUIS: Centre = Centre {
        lat: 38.627,
        lon: -90.199,
    };
    const ST_LOUIS_VIEWBOX: &str = "-90.6990,38.1270,-89.6990,39.1270";

    fn sent_q(params: &Params) -> String {
        params
            .iter()
            .find(|(key, _)| *key == "q")
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    }

    fn expected(pairs: &[(&'static str, &str)]) -> Params {
        pairs
            .iter()
            .map(|(key, value)| (*key, value.to_string()))
            .collect()
    }

    async fn params_sent_for(query: &str, centre: Option<Centre>) -> Params {
        let cache = GeocodeCache::new(Duration::from_secs(60), 8);
        let limiter = RateLimiter::new(Duration::from_millis(1));
        let captured: Arc<Mutex<Option<Params>>> = Arc::new(Mutex::new(None));
        let sink = captured.clone();
        resolve(query, centre, &cache, &limiter, move |params: Params| {
            *sink.lock().expect("capture lock") = Some(params);
            async { Ok(serde_json::json!({})) }
        })
        .await
        .expect("the fake upstream answers");
        let sent = captured.lock().expect("capture lock").take();
        sent.expect("the query reached the upstream")
    }

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
            move |params: Params| {
                let calls = calls.clone();
                let q = sent_q(&params);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(serde_json::json!({ "display_name": q }))
                }
            }
        };

        let first = resolve("Oakland", None, &cache, &limiter, counting_fetch())
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(first["display_name"], "Oakland");

        let second = resolve("  oakland ", None, &cache, &limiter, counting_fetch())
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

        let result = resolve("   ", None, &cache, &limiter, move |params: Params| {
            let calls = calls_for_fetch.clone();
            let q = sent_q(&params);
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(serde_json::json!({ "display_name": q }))
            }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    /// Two-byte characters at the cap, so a cap counted in bytes would refuse the accepted query.
    #[tokio::test]
    async fn a_query_over_200_characters_is_refused_before_any_upstream_call() {
        let cache = GeocodeCache::new(Duration::from_secs(60), 8);
        let limiter = RateLimiter::new(Duration::from_millis(1));
        let calls = Arc::new(AtomicUsize::new(0));
        let counting_fetch = || {
            let calls = calls.clone();
            move |params: Params| {
                let calls = calls.clone();
                let q = sent_q(&params);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(serde_json::json!({ "display_name": q }))
                }
            }
        };

        let refused = resolve(&"a".repeat(201), None, &cache, &limiter, counting_fetch()).await;
        let err = refused.expect_err("a 201-character q");
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let accepted = resolve(&"é".repeat(200), None, &cache, &limiter, counting_fetch()).await;
        assert!(accepted.is_ok(), "a 200-character q");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
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
                resolve(query, None, &cache, &limiter, move |params: Params| {
                    let calls = calls.clone();
                    let q = sent_q(&params);
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

    #[tokio::test]
    async fn a_bare_query_from_a_located_node_is_bounded_to_a_viewbox_around_it() {
        let sent = params_sent_for("63101", Some(ST_LOUIS)).await;
        assert_eq!(
            sent,
            expected(&[
                ("q", "63101"),
                ("format", "json"),
                ("limit", "1"),
                ("addressdetails", "1"),
                ("viewbox", ST_LOUIS_VIEWBOX),
                ("bounded", "1"),
            ])
        );
    }

    /// Nominatim reads `viewbox` as lon,lat,lon,lat; a lat-first box silently searches elsewhere.
    #[tokio::test]
    async fn viewbox_puts_longitude_first() {
        let sent_box = viewbox(ST_LOUIS);
        assert_eq!(sent_box, ST_LOUIS_VIEWBOX);
        let first: f64 = sent_box
            .split(',')
            .next()
            .expect("a first component")
            .parse()
            .expect("a number");
        assert!(
            !(-90.0..=90.0).contains(&first),
            "the first component {first} must be the longitude"
        );

        let sent = params_sent_for("63101", Some(ST_LOUIS)).await;
        assert!(sent.contains(&("viewbox", ST_LOUIS_VIEWBOX.to_string())));
        assert!(sent.contains(&("addressdetails", "1".to_string())));
    }

    #[tokio::test]
    async fn a_node_without_coordinates_sends_no_viewbox_or_bounded() {
        let node = |lat: Option<f64>, lon: Option<f64>| NodeConfig {
            location_lat: lat,
            location_lon: lon,
            ..NodeConfig::default()
        };
        assert!(node_centre(&node(Some(38.627), Some(-90.199))).is_some());
        for (lat, lon) in [
            (None, None),
            (Some(38.627), None),
            (None, Some(-90.199)),
            (Some(91.0), Some(-90.199)),
            (Some(38.627), Some(181.0)),
            (Some(f64::NAN), Some(-90.199)),
        ] {
            assert!(
                node_centre(&node(lat, lon)).is_none(),
                "no centre may be invented from {lat:?},{lon:?}"
            );
        }

        let sent = params_sent_for("63101", None).await;
        assert_eq!(
            sent,
            expected(&[
                ("q", "63101"),
                ("format", "json"),
                ("limit", "1"),
                ("addressdetails", "1"),
            ])
        );
    }

    /// `bounded=1` would turn "Austin, TX" asked from St. Louis into a miss.
    #[tokio::test]
    async fn qualified_queries_are_never_bounded() {
        for query in ["63101, USA", "Austin, TX"] {
            let sent = params_sent_for(query, Some(ST_LOUIS)).await;
            assert_eq!(
                sent,
                expected(&[
                    ("q", query),
                    ("format", "json"),
                    ("limit", "1"),
                    ("addressdetails", "1"),
                ]),
                "{query}"
            );
        }
    }

    #[tokio::test]
    async fn moving_the_node_misses_the_cache() {
        const KANSAS_CITY: Centre = Centre {
            lat: 39.0997,
            lon: -94.5786,
        };
        let cache = GeocodeCache::new(Duration::from_secs(60), 8);
        let limiter = RateLimiter::new(Duration::from_millis(1));
        let calls = Arc::new(AtomicUsize::new(0));
        let counting_fetch = || {
            let calls = calls.clone();
            move |params: Params| {
                let q = sent_q(&params);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(serde_json::json!({ "display_name": q }))
                }
            }
        };

        resolve("63101", Some(ST_LOUIS), &cache, &limiter, counting_fetch())
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        resolve("63101", Some(KANSAS_CITY), &cache, &limiter, counting_fetch())
            .await
            .unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "a moved node must not be served the old area's answer"
        );

        resolve("63101", Some(ST_LOUIS), &cache, &limiter, counting_fetch())
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        resolve("63101", None, &cache, &limiter, counting_fetch())
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);

        resolve("Austin, TX", Some(ST_LOUIS), &cache, &limiter, counting_fetch())
            .await
            .unwrap();
        resolve("Austin, TX", Some(KANSAS_CITY), &cache, &limiter, counting_fetch())
            .await
            .unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            4,
            "a qualified query is sent identically from anywhere, so it shares one entry"
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

    fn hit_with_address(address: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!([{
            "lat": "38.6270",
            "lon": "-90.1994",
            "display_name": "63101, Saint Louis, Missouri, United States",
            "address": address,
        }]))
        .expect("serialise fixture")
    }

    fn place_parts(body: &[u8]) -> (serde_json::Value, serde_json::Value) {
        let value = parse_nominatim(200, body).expect("a well-formed hit");
        (value["city"].clone(), value["state"].clone())
    }

    /// T10
    #[test]
    fn nominatim_hits_are_typed_range_checked_and_truncated() {
        let value = parse_nominatim(200, &hit("37.8044", "-122.2712", "Oakland, California"))
            .expect("a well-formed hit");
        assert_eq!(value["lat"], "37.8044", "lat stays the upstream string");
        assert_eq!(value["lon"], "-122.2712", "lon stays the upstream string");
        assert_eq!(value["display_name"], "Oakland, California");
        assert_eq!(value["city"], "Oakland");
        assert!(value["state"].is_null());
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["city", "display_name", "lat", "lon", "state"],
            "only lat, lon, display_name, city and state"
        );

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

    #[test]
    fn city_falls_back_through_town_village_hamlet() {
        for (address, city) in [
            (serde_json::json!({ "city": "St. Louis", "town": "Kirkwood" }), "St. Louis"),
            (serde_json::json!({ "town": "Kirkwood", "village": "Times Beach" }), "Kirkwood"),
            (serde_json::json!({ "village": "Times Beach", "hamlet": "Labadie" }), "Times Beach"),
            (serde_json::json!({ "hamlet": "Labadie" }), "Labadie"),
            (serde_json::json!({ "city": "  ", "town": "Kirkwood" }), "Kirkwood"),
        ] {
            let (got, _) = place_parts(&hit_with_address(address.clone()));
            assert_eq!(got, city, "{address}");
        }
    }

    #[test]
    fn state_is_the_postal_code_when_nominatim_gives_a_us_iso_code() {
        let (city, state) = place_parts(&hit_with_address(serde_json::json!({
            "city": "St. Louis",
            "state": "Missouri",
            "ISO3166-2-lvl4": "US-MO",
        })));
        assert_eq!(city, "St. Louis");
        assert_eq!(state, "MO");
    }

    #[test]
    fn state_without_a_us_iso_code_is_the_full_state_name() {
        for (address, expected_state) in [
            (serde_json::json!({ "state": "Missouri" }), "Missouri"),
            (
                serde_json::json!({ "state": "Ontario", "ISO3166-2-lvl4": "CA-ON" }),
                "Ontario",
            ),
            (
                serde_json::json!({ "state": "Missouri", "ISO3166-2-lvl4": "US-M" }),
                "Missouri",
            ),
        ] {
            let (_, state) = place_parts(&hit_with_address(address.clone()));
            assert_eq!(state, expected_state, "{address}");
        }
    }

    #[test]
    fn an_over_long_city_or_state_is_truncated() {
        let long = "é".repeat(300);
        let (city, state) = place_parts(&hit_with_address(serde_json::json!({
            "city": long,
            "state": long,
        })));
        for part in [city, state] {
            let part = part.as_str().expect("a string");
            assert_eq!(part.chars().count(), PLACE_PART_MAX_CHARS);
        }
    }

    /// The keys are always present, so the client can tell "no city" from an old server.
    #[test]
    fn a_hit_without_address_has_null_city_and_state() {
        let bare = serde_json::to_vec(&serde_json::json!([{
            "lat": "38.6270",
            "lon": "-90.1994",
            "display_name": "63101, Saint Louis, Missouri, United States",
        }]))
        .expect("serialise fixture");
        for body in [
            bare,
            hit_with_address(serde_json::json!({ "country": "United States" })),
        ] {
            let value = parse_nominatim(200, &body).expect("a well-formed hit");
            let object = value.as_object().expect("an object");
            assert!(object.contains_key("city") && object.contains_key("state"));
            assert!(value["city"].is_null());
            assert!(value["state"].is_null());
        }
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
