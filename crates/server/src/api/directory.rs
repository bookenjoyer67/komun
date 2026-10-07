use std::net::SocketAddr;
use std::sync::LazyLock;
use std::time::Instant as StdInstant;

use axum::{
    extract::{ConnectInfo, Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tokio::sync::Mutex as TokioMutex;

use super::outbound;
use super::StatusError;
use crate::auth::{record_audit, require_auth, require_superadmin, AuthUser};
use crate::db::posts::contains_pattern;
use crate::rate_limit::RouteClass;
use crate::AppState;

static REGISTRATIONS: LazyLock<TokioMutex<Vec<StdInstant>>> =
    LazyLock::new(|| TokioMutex::new(Vec::new()));

const URL_REFUSED: &str = "url must be https on a public host";
const NOT_CONFIRMED: &str = "server at url did not confirm this registration";

pub fn router(state: AppState) -> Router {
    // Gate on the resolved `open_registration`, not `[registration] mode`: signup can be
    // invite-only while peer directory registrations stay open.
    let registration_is_open = state.config.open_registration();

    let mut public = Router::new().route("/directory", get(list_servers));

    if registration_is_open {
        public = public.route("/directory/register", post(register_server));
    }

    let protected = Router::new()
        .route("/directory/{url}", delete(remove_server))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_superadmin,
        ));

    let protected_register = if registration_is_open {
        None
    } else {
        Some(
            Router::new()
                .route("/directory/register", post(register_server))
                .layer(middleware::from_fn_with_state(state.clone(), require_auth)),
        )
    };

    let mut router = public.merge(protected);
    if let Some(r) = protected_register {
        router = router.merge(r);
    }
    router.with_state(state)
}

/// The `location_*` fields peers still send are ignored: a stored location comes only from the
/// peer's own `/api/node`.
#[derive(Deserialize)]
pub struct RegisterRequest {
    url: String,
    name: String,
    description: Option<String>,
    version: Option<String>,
    /// Peers advertise whether they accept open registrations; older peers omit it and are treated
    /// as openly registerable.
    #[serde(default)]
    open_registration: Option<bool>,
}

/// What a peer's own `/api/node` says about it; the fields mirror `api::node::NodeInfo`.
#[derive(Deserialize)]
struct PeerNode {
    name: String,
    description: Option<String>,
    version: Option<String>,
    domain: Option<String>,
    location: Option<PeerLocation>,
    open_registration: Option<bool>,
}

#[derive(Deserialize)]
struct PeerLocation {
    name: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
}

#[derive(Serialize, FromRow)]
pub struct DirectoryEntry {
    pub url: String,
    pub name: String,
    pub description: Option<String>,
    pub location_name: Option<String>,
    pub location_lat: Option<f64>,
    pub location_lon: Option<f64>,
    pub version: Option<String>,
    pub open_registration: bool,
    pub last_seen: DateTime<Utc>,
    pub registered_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct DirectoryEntryWithDistance {
    #[serde(flatten)]
    pub entry: DirectoryEntry,
    pub distance_km: Option<f64>,
}

#[derive(Deserialize)]
pub struct SearchParams {
    q: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
    radius: Option<f64>,
}

/// Columns of `directory_entries`, written once so the three queries below cannot drift.
const ENTRY_COLUMNS: &str =
    "url, name, description, location_name, location_lat, location_lon, version, open_registration, last_seen, registered_at";

/// Great-circle distance in km between (`$1`, `$2`) and a row's location.
const DISTANCE_KM: &str = r#"(6371 * acos(
    LEAST(1.0, GREATEST(-1.0,
      cos(radians($1)) * cos(radians(location_lat)) *
      cos(radians(location_lon) - radians($2)) +
      sin(radians($1)) * sin(radians(location_lat))
    ))
  ))"#;

async fn register_server(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<RegisterRequest>,
) -> Response {
    if let Err(limited) = super::per_ip_limit(&state, RouteClass::DirectoryRegister, peer, &headers)
    {
        return limited.into_response();
    }
    match register_verified(&state, input).await {
        Ok(body) => body.into_response(),
        Err(e) => e.into_response(),
    }
}

/// The directory stores no registrant identity, so the peer's own `/api/node` is the proof of
/// control: an anonymous caller can at most refresh an entry to what that server publishes.
async fn register_verified(
    state: &AppState,
    input: RegisterRequest,
) -> Result<Json<serde_json::Value>, StatusError> {
    let url = input.url.trim_end_matches('/').to_string();
    let host = registrable_host(&url)
        .ok_or_else(|| StatusError::with_status(StatusCode::BAD_REQUEST, URL_REFUSED))?;

    let node = match outbound::fetch_public(&format!("{url}/api/node"), outbound::NODE_INFO).await {
        Ok(body) => serde_json::from_slice::<PeerNode>(&body).ok(),
        Err(e) if e.is_refused() => {
            return Err(StatusError::with_status(
                StatusCode::BAD_REQUEST,
                URL_REFUSED,
            ));
        }
        Err(e) => {
            tracing::debug!("directory registration: peer node info unavailable: {e}");
            None
        }
    };
    let Some(node) = node.filter(|node| node_matches(&host, &input, node)) else {
        return Err(StatusError::with_status(
            StatusCode::UNPROCESSABLE_ENTITY,
            NOT_CONFIRMED,
        ));
    };

    // Counted only after verification, so unverifiable attempts cannot exhaust it.
    {
        let mut registrations = REGISTRATIONS.lock().await;
        let window_start = StdInstant::now() - std::time::Duration::from_secs(3600);
        registrations.retain(|t| *t > window_start);
        if registrations.len() >= 20 {
            return Err(StatusError::with_status(
                StatusCode::TOO_MANY_REQUESTS,
                "rate limit exceeded: max 20 server registrations per hour",
            ));
        }
        registrations.push(StdInstant::now());
    }

    // The peer's `null` location means "none"; falling back to the body would let any caller pin
    // a location on someone else's entry.
    let (location_name, location_lat, location_lon) = match node.location {
        Some(location) => (location.name, location.lat, location.lon),
        None => (None, None, None),
    };
    let description = node.description.or(input.description);
    let version = node.version.or(input.version);
    // The column default and this fallback agree: peers that omit the field are openly
    // registerable.
    let open_registration = node
        .open_registration
        .or(input.open_registration)
        .unwrap_or(true);

    sqlx::query(
        r#"INSERT INTO directory_entries (url, name, description, location_name, location_lat, location_lon, version, open_registration, last_seen)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
           ON CONFLICT (url) DO UPDATE SET
             name = EXCLUDED.name,
             description = EXCLUDED.description,
             location_name = EXCLUDED.location_name,
             location_lat = EXCLUDED.location_lat,
             location_lon = EXCLUDED.location_lon,
             version = EXCLUDED.version,
             open_registration = EXCLUDED.open_registration,
             last_seen = now()"#,
    )
    .bind(&url)
    .bind(&input.name)
    .bind(description)
    .bind(location_name)
    .bind(location_lat)
    .bind(location_lon)
    .bind(version)
    .bind(open_registration)
    .execute(&state.pool)
    .await?;

    Ok(Json(
        serde_json::json!({"status": "registered", "url": url}),
    ))
}

/// The URL's host, if it may be registered. A query or fragment is refused because
/// `{url}/api/node` would not reach the node.
fn registrable_host(url: &str) -> Option<String> {
    let parsed = outbound::check_url(url).ok()?;
    if parsed.scheme() != "https" || parsed.query().is_some() || parsed.fragment().is_some() {
        return None;
    }
    parsed.host_str().map(str::to_owned)
}

fn node_matches(url_host: &str, input: &RegisterRequest, node: &PeerNode) -> bool {
    let name_matches = node.name.trim() == input.name.trim();
    let domain_matches = match node.domain.as_deref() {
        Some(domain) => domain.eq_ignore_ascii_case(url_host),
        None => true,
    };
    name_matches && domain_matches
}

async fn list_servers(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Vec<DirectoryEntryWithDistance>>, StatusError> {
    let entries = if let (Some(lat), Some(lon)) = (params.lat, params.lon) {
        let radius = params.radius.unwrap_or(50.0);

        let rows = sqlx::query_as::<_, DirectoryEntryWithDist>(&format!(
            r#"SELECT {ENTRY_COLUMNS}, {DISTANCE_KM} AS distance_km
               FROM directory_entries
               WHERE location_lat IS NOT NULL AND location_lon IS NOT NULL
               AND {DISTANCE_KM} < $3
               ORDER BY distance_km
               LIMIT 20"#
        ))
        .bind(lat)
        .bind(lon)
        .bind(radius)
        .fetch_all(&state.pool)
        .await?;

        rows.into_iter().map(Into::into).collect()
    } else if let Some(ref q) = params.q {
        let pattern = contains_pattern(q);
        let rows = sqlx::query_as::<_, DirectoryEntry>(&format!(
            r#"SELECT {ENTRY_COLUMNS}
               FROM directory_entries
               WHERE name ILIKE $1 ESCAPE '\'
                  OR location_name ILIKE $1 ESCAPE '\'
                  OR description ILIKE $1 ESCAPE '\'
               ORDER BY last_seen DESC
               LIMIT 20"#
        ))
        .bind(&pattern)
        .fetch_all(&state.pool)
        .await?;

        rows.into_iter()
            .map(|e| DirectoryEntryWithDistance {
                entry: e,
                distance_km: None,
            })
            .collect()
    } else {
        let rows = sqlx::query_as::<_, DirectoryEntry>(&format!(
            r#"SELECT {ENTRY_COLUMNS}
               FROM directory_entries
               ORDER BY last_seen DESC
               LIMIT 20"#
        ))
        .fetch_all(&state.pool)
        .await?;

        rows.into_iter()
            .map(|e| DirectoryEntryWithDistance {
                entry: e,
                distance_km: None,
            })
            .collect()
    };

    Ok(Json(entries))
}

async fn remove_server(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(url): Path<String>,
) -> Result<Json<serde_json::Value>, StatusError> {
    let removed = sqlx::query("DELETE FROM directory_entries WHERE url = $1")
        .bind(&url)
        .execute(&state.pool)
        .await?
        .rows_affected();

    record_audit(
        &state.pool,
        Some(auth.user_id),
        "directory.remove",
        None,
        serde_json::json!({ "url": url, "removed": removed }),
    )
    .await;

    Ok(Json(serde_json::json!({"status": "removed"})))
}

#[derive(FromRow)]
struct DirectoryEntryWithDist {
    url: String,
    name: String,
    description: Option<String>,
    location_name: Option<String>,
    location_lat: Option<f64>,
    location_lon: Option<f64>,
    version: Option<String>,
    open_registration: bool,
    last_seen: DateTime<Utc>,
    registered_at: DateTime<Utc>,
    distance_km: Option<f64>,
}

impl From<DirectoryEntryWithDist> for DirectoryEntryWithDistance {
    fn from(r: DirectoryEntryWithDist) -> Self {
        DirectoryEntryWithDistance {
            entry: DirectoryEntry {
                url: r.url,
                name: r.name,
                description: r.description,
                location_name: r.location_name,
                location_lat: r.location_lat,
                location_lon: r.location_lon,
                version: r.version,
                open_registration: r.open_registration,
                last_seen: r.last_seen,
                registered_at: r.registered_at,
            },
            distance_km: r.distance_km,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(name: &str) -> RegisterRequest {
        serde_json::from_value(json!({ "url": "https://komun.example.org", "name": name }))
            .expect("register request")
    }

    fn node(name: &str, domain: Option<&str>) -> PeerNode {
        serde_json::from_value(json!({
            "name": name,
            "description": "d",
            "version": "0.1.0",
            "domain": domain,
            "location": null,
            "listed": true,
            "open_registration": true,
        }))
        .expect("peer node")
    }

    /// T12
    #[test]
    fn a_peer_must_confirm_its_name_and_domain() {
        let host = "komun.example.org";
        assert!(node_matches(
            host,
            &request("Oakland Komun"),
            &node("Oakland Komun", Some(host))
        ));

        assert!(
            !node_matches(
                host,
                &request("Oakland Komun"),
                &node("Someone Else", Some(host))
            ),
            "a name mismatch must not confirm"
        );
        assert!(
            !node_matches(
                host,
                &request("Oakland Komun"),
                &node("Oakland Komun", Some("other.example.org"))
            ),
            "a domain mismatch must not confirm"
        );
        assert!(
            node_matches(
                host,
                &request("Oakland Komun"),
                &node("Oakland Komun", None)
            ),
            "a peer without a public_url publishes no domain; the name decides"
        );
        assert!(
            node_matches(
                host,
                &request("Oakland Komun"),
                &node("Oakland Komun", Some("KOMUN.Example.ORG"))
            ),
            "host names compare case-insensitively"
        );
    }

    #[test]
    fn only_https_without_query_or_fragment_is_registrable() {
        assert_eq!(
            registrable_host("https://komun.example.org").as_deref(),
            Some("komun.example.org")
        );
        for url in [
            "http://komun.example.org",
            "https://komun.example.org/?x=1",
            "https://komun.example.org/#top",
            "javascript:alert(1)",
            "https://user:pw@komun.example.org",
        ] {
            assert_eq!(registrable_host(url), None, "{url} must be refused");
        }
    }
}
