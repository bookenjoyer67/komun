//! The anonymous routes that make this server fetch a URL must refuse a private target before any
//! connection or query, and `DELETE /directory/{url}` must be superadmin-only and audited.
//!
//! T14 and T15 need no database: the pool is lazy and points at a closed port, so a handler that
//! reached the database answers 500 instead of the refusal under test. T16 needs a live Postgres
//! and is `#[ignore]`d; run it against a disposable database with
//! `KOMUN_TEST_DATABASE_URL=postgres://... cargo test -p komun-server outbound_routes -- --ignored`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    extract::connect_info::MockConnectInfo,
    http::{header, Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use crate::config::Config;
use crate::{api, auth, db, rate_limit, sessions, AppState};

const DATABASE_ENV: &str = "KOMUN_TEST_DATABASE_URL";

const DIRECTORY_CONFIG: &str = "[discovery]\ndirectory_enabled = true\nopen_registration = true\n";

/// The refusal messages are the API's error codes: it has no error-code field.
const LINK_PREVIEW_REFUSED: &str = "url not allowed";
const REGISTER_REFUSED: &str = "url must be https on a public host";
const SUPERADMIN_REQUIRED: &str = "superadmin access required";

const DIRECTORY_REMOVE_ACTION: &str = "directory.remove";

/// A synthetic verifier in the shape the client sends; it is not real material.
const VERIFIER: &str = "c3ludGhldGljLXRlc3QtdmVyaWZpZXItZm9yLXI3LXJvdXRlcw";

fn state_with(pool: PgPool, config_toml: &str) -> AppState {
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
fn unreachable_pool() -> PgPool {
    PgPoolOptions::new()
        .acquire_timeout(Duration::from_secs(2))
        .connect_lazy("postgres://komun:komun@127.0.0.1:1/komun_unreachable")
        .expect("a lazy pool does not connect at build time")
}

fn app(state: AppState) -> Router {
    let peer = SocketAddr::from(([203, 0, 113, 9], 40_000));
    api::router(state).layer(MockConnectInfo(peer))
}

/// Percent-encodes everything outside the RFC 3986 unreserved set.
fn escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() * 3);
    for b in raw.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

async fn send(
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

fn error_of(body: &Value) -> &str {
    body.get("error").and_then(Value::as_str).unwrap_or("")
}

/// T14: loopback, IPv4-mapped loopback and non-http schemes are refused as 400, not fetched.
#[tokio::test]
async fn t14_link_preview_refuses_private_and_non_http_targets() {
    let app = app(state_with(unreachable_pool(), ""));

    for target in [
        "http://127.0.0.1:1/",
        "http://[::ffff:127.0.0.1]:1/",
        "javascript:alert(1)",
        "file:///etc/passwd",
    ] {
        let uri = format!("/link-preview?url={}", escape(target));
        let (status, body) = send(&app, Method::GET, &uri, None, None).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{target}: {:?}",
            error_of(&body)
        );
        assert_eq!(error_of(&body), LINK_PREVIEW_REFUSED, "{target}");
    }
}

/// T15: register refuses plain http, a loopback https host and a non-http scheme as 400, before
/// the database is touched.
#[tokio::test]
async fn t15_directory_register_refuses_non_https_and_private_hosts() {
    let app = app(state_with(unreachable_pool(), DIRECTORY_CONFIG));

    for url in [
        "http://127.0.0.1:1",
        "https://127.0.0.1:1",
        "javascript:alert(1)",
    ] {
        let body = json!({ "url": url, "name": "x" });
        let (status, response) =
            send(&app, Method::POST, "/directory/register", None, Some(&body)).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{url}: {:?}",
            error_of(&response)
        );
        assert_eq!(error_of(&response), REGISTER_REFUSED, "{url}");
    }
}

struct LiveHarness {
    pool: PgPool,
    app: Router,
}

/// Connects via `KOMUN_TEST_DATABASE_URL`; without it the test panics rather than passing
/// unearned, and the URL is never printed.
async fn live_harness() -> LiveHarness {
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

    let app = app(state_with(pool.clone(), DIRECTORY_CONFIG));
    LiveHarness { pool, app }
}

struct TestUser {
    id: Uuid,
    bearer: String,
}

/// Inserts a verified user with `role`, then opens a session; the email is unique per call.
async fn seed_user(pool: &PgPool, role: &str) -> TestUser {
    let id = Uuid::now_v7();
    let email = format!("r7-directory-{id}@test.invalid");
    let password_hash = auth::password::hash_verifier(VERIFIER).expect("hash");
    sqlx::query(
        "INSERT INTO users (id, email, email_verified_at, password_hash, auth_salt, display_name, role)
         VALUES ($1, $2, now(), $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(email)
    .bind(password_hash)
    .bind(vec![0x5a_u8; 16])
    .bind("R7 test user")
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

async fn entry_exists(pool: &PgPool, url: &str) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM directory_entries WHERE url = $1)")
        .bind(url)
        .fetch_one(pool)
        .await
        .expect("read directory entry")
}

async fn audit_actions(pool: &PgPool, actor: Uuid) -> Vec<String> {
    sqlx::query_scalar::<_, String>("SELECT action FROM audit_events WHERE actor_id = $1")
        .bind(actor)
        .fetch_all(pool)
        .await
        .expect("read audit rows")
}

/// T16: a signed-in normal user is refused and the entry stays; a superadmin removes it and one
/// `directory.remove` audit row names them.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn t16_directory_delete_is_superadmin_only_and_audited() {
    let h = live_harness().await;
    let entry = format!("https://r7-{}.test.invalid", Uuid::now_v7());
    sqlx::query("INSERT INTO directory_entries (url, name) VALUES ($1, $2)")
        .bind(&entry)
        .bind("R7 test peer")
        .execute(&h.pool)
        .await
        .expect("insert directory entry");
    let user = seed_user(&h.pool, "user").await;
    let admin = seed_user(&h.pool, "superadmin").await;
    let path = format!("/directory/{}", escape(&entry));

    let (status, body) = send(&h.app, Method::DELETE, &path, Some(&user.bearer), None).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "error: {:?}",
        error_of(&body)
    );
    assert_eq!(error_of(&body), SUPERADMIN_REQUIRED);
    assert!(
        entry_exists(&h.pool, &entry).await,
        "a refused delete must leave the entry"
    );
    assert!(
        !audit_actions(&h.pool, user.id)
            .await
            .iter()
            .any(|a| a == DIRECTORY_REMOVE_ACTION),
        "a refused delete must not be audited as a removal"
    );

    let (status, body) = send(&h.app, Method::DELETE, &path, Some(&admin.bearer), None).await;
    assert!(status.is_success(), "got {status}: {:?}", error_of(&body));
    assert!(
        !entry_exists(&h.pool, &entry).await,
        "a superadmin delete must remove the entry"
    );
    let removals = audit_actions(&h.pool, admin.id)
        .await
        .iter()
        .filter(|a| *a == DIRECTORY_REMOVE_ACTION)
        .count();
    assert_eq!(
        removals, 1,
        "want exactly one {DIRECTORY_REMOVE_ACTION} row"
    );
}
