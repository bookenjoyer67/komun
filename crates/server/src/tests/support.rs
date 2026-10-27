//! Harness pieces shared by the ops and admin tests.
//!
//! Live tests read `KOMUN_TEST_DATABASE_URL`, run on the host only, and never print the URL.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    extract::connect_info::MockConnectInfo,
    http::{header, Method, Request, StatusCode},
    Router,
};
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use crate::config::Config;
use crate::{api, auth, db, rate_limit, sessions, AppState};

const DATABASE_ENV: &str = "KOMUN_TEST_DATABASE_URL";

/// A synthetic verifier in the shape the client sends; it is not real material.
const VERIFIER: &str = "c3ludGhldGljLXRlc3QtdmVyaWZpZXItZm9yLXIxNC1vcHM";

pub(crate) fn state_with(pool: PgPool, config_toml: &str) -> AppState {
    let config: Config = toml::from_str(config_toml).expect("parse test config");
    AppState {
        pool,
        config: Arc::new(config),
        rate_limiter: Arc::new(rate_limit::RateLimiter::new()),
        mailer: Arc::new(None),
        trusted_proxies: Arc::new(Vec::new()),
        salt_pepper: Arc::new(sessions::generate_pepper()),
    }
}

/// Port 1 refuses at once, so a handler that reaches the database fails fast instead of hanging.
pub(crate) fn unreachable_pool() -> PgPool {
    PgPoolOptions::new()
        .acquire_timeout(Duration::from_secs(2))
        .connect_lazy("postgres://komun:komun@127.0.0.1:1/komun_unreachable")
        .expect("a lazy pool does not connect at build time")
}

pub(crate) fn app(state: AppState) -> Router {
    let peer = SocketAddr::from(([203, 0, 113, 14], 40_000));
    api::router(state).layer(MockConnectInfo(peer))
}

pub(crate) struct LiveHarness {
    pub(crate) pool: PgPool,
    pub(crate) app: Router,
}

/// Without the variable the test panics rather than passing unearned.
pub(crate) async fn live_harness() -> LiveHarness {
    let Ok(url) = std::env::var(DATABASE_ENV) else {
        panic!("{DATABASE_ENV} is not set: point it at a disposable Postgres database");
    };
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("connect to the test database");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("apply migrations to the test database");

    let app = app(state_with(pool.clone(), ""));
    LiveHarness { pool, app }
}

pub(crate) struct TestUser {
    pub(crate) id: Uuid,
    pub(crate) bearer: String,
}

/// A verified user with a live session; the email is unique per call, the display name is not.
pub(crate) async fn seed_user(pool: &PgPool, role: &str, display_name: &str) -> TestUser {
    let id = Uuid::now_v7();
    let email = format!("r14-ops-{id}@test.invalid");
    let password_hash = auth::password::hash_verifier(VERIFIER).expect("hash");
    sqlx::query(
        "INSERT INTO users (id, email, email_verified_at, password_hash, auth_salt, display_name, role)
         VALUES ($1, $2, now(), $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(&email)
    .bind(password_hash)
    .bind(vec![0x5a_u8; 16])
    .bind(display_name)
    .bind(role)
    .execute(pool)
    .await
    .expect("insert test user");

    let token = sessions::generate_token();
    db::sessions::create(pool, id, &token.hash, 1, None, None, None)
        .await
        .expect("create test session");
    TestUser {
        id,
        bearer: token.raw,
    }
}

pub(crate) async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    bearer: Option<&str>,
    body: Option<&Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(bearer) = bearer {
        request = request.header(header::AUTHORIZATION, format!("Bearer {bearer}"));
    }
    let request = match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    }
    .expect("build request");

    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router is infallible");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("read response body");
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

pub(crate) fn error_of(body: &Value) -> &str {
    body.get("error").and_then(Value::as_str).unwrap_or("")
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct AuditRow {
    pub(crate) actor_id: Option<Uuid>,
    pub(crate) subject_id: Option<Uuid>,
}

/// Rows for `action` that name `who` as actor or subject; fresh ids keep counts exact on a shared
/// database.
pub(crate) async fn audit_rows(pool: &PgPool, action: &str, who: Uuid) -> Vec<AuditRow> {
    sqlx::query_as::<_, AuditRow>(
        "SELECT actor_id, subject_id FROM audit_events
         WHERE action = $1 AND (actor_id = $2 OR subject_id = $2)",
    )
    .bind(action)
    .bind(who)
    .fetch_all(pool)
    .await
    .expect("read audit rows")
}
