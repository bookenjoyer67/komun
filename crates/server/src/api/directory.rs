use std::sync::LazyLock;
use std::time::Instant as StdInstant;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    middleware,
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tokio::sync::Mutex as TokioMutex;

use super::StatusError;
use crate::auth::require_auth;
use crate::AppState;

static REGISTRATIONS: LazyLock<TokioMutex<Vec<StdInstant>>> =
    LazyLock::new(|| TokioMutex::new(Vec::new()));

pub fn router(state: AppState) -> Router {
    // B5: the gate is the resolved `open_registration`, not `[registration] mode` directly. An
    // operator may sign users up by invite only and still accept peer registrations into the
    // directory by setting `open_registration = true`; unset falls back to the signup mode.
    let registration_is_open = state.config.open_registration();

    let mut public = Router::new().route("/directory", get(list_servers));

    if registration_is_open {
        public = public.route("/directory/register", post(register_server));
    }

    let protected = Router::new()
        .route("/directory/{url}", delete(remove_server))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth));

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

#[derive(Deserialize)]
pub struct RegisterRequest {
    url: String,
    name: String,
    description: Option<String>,
    location_name: Option<String>,
    location_lat: Option<f64>,
    location_lon: Option<f64>,
    version: Option<String>,
    /// Peers advertise whether they accept open registrations. Older peers omit it
    /// (`#[serde(default)]`), and are treated as openly registerable.
    #[serde(default)]
    open_registration: Option<bool>,
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

/// Great-circle distance in km from `$1`/`$2` to a row's `location_lat`/`location_lon`.
const DISTANCE_KM: &str = r#"(6371 * acos(
    LEAST(1.0, GREATEST(-1.0,
      cos(radians($1)) * cos(radians(location_lat)) *
      cos(radians(location_lon) - radians($2)) +
      sin(radians($1)) * sin(radians(location_lat))
    ))
  ))"#;

async fn register_server(
    State(state): State<AppState>,
    Json(input): Json<RegisterRequest>,
) -> Result<Json<serde_json::Value>, StatusError> {
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

    let url = input.url.trim_end_matches('/').to_string();

    // A3.2: `communities_count` and `community_locations` are not columns of the squashed
    // `directory_entries`. A server registering itself advertises one location, its own.
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
    .bind(&input.description)
    .bind(&input.location_name)
    .bind(input.location_lat)
    .bind(input.location_lon)
    .bind(&input.version)
    // Old peers omit the field; the column default and our fallback agree: openly registerable.
    .bind(input.open_registration.unwrap_or(true))
    .execute(&state.pool)
    .await?;

    Ok(Json(
        serde_json::json!({"status": "registered", "url": url}),
    ))
}

async fn list_servers(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Vec<DirectoryEntryWithDistance>>, StatusError> {
    // The nearby case used to run a second query that expanded each entry's
    // `community_locations` JSONB into one result per community, then deduplicated the two
    // result sets by url. With one location per server there is one query and nothing to merge.
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
        let pattern = format!("%{}%", q);
        let rows = sqlx::query_as::<_, DirectoryEntry>(&format!(
            r#"SELECT {ENTRY_COLUMNS}
               FROM directory_entries
               WHERE name ILIKE $1 OR location_name ILIKE $1 OR description ILIKE $1
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
    axum::extract::Path(url): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, StatusError> {
    sqlx::query("DELETE FROM directory_entries WHERE url = $1")
        .bind(&url)
        .execute(&state.pool)
        .await?;
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
