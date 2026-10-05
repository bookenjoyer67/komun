//! `PUT /api/auth/me` must not replace stored encryption keys on a session alone.
//!
//! Every test needs a live Postgres, so every test is `#[ignore]`d; run against a disposable
//! database with `KOMUN_TEST_DATABASE_URL=postgres://... cargo test -p komun-server key_change
//! -- --ignored`. Each test creates its own user and rate limiter, so they run in any order. All
//! passwords and keys below are synthetic byte patterns, not real material.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::connect_info::MockConnectInfo,
    http::{header, Method, Request, StatusCode},
    Router,
};
use base64::Engine;
use serde_json::{json, Map, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use crate::config::Config;
use crate::{auth, db, rate_limit, sessions, AppState};

const DATABASE_ENV: &str = "KOMUN_TEST_DATABASE_URL";

/// A synthetic verifier in the shape the client sends: base64, more than 43 characters.
const VERIFIER: &str = "c3ludGhldGljLXRlc3QtdmVyaWZpZXItZm9yLWtleS1jaGFuZ2U";
const WRONG_VERIFIER: &str = "d3JvbmctdGVzdC12ZXJpZmllci1mb3Ita2V5LWNoYW5nZQ";

/// The refusal messages are the API's error codes: it has no error-code field.
const PARTIAL_SET_ERROR: &str = "the encryption keys must be sent together: \
                                 encryption_public_key, encrypted_key_bundle, bundle_salt, \
                                 encrypted_recovery_bundle, recovery_bundle_salt";
const MISSING_VERIFIER_ERROR: &str = "changing existing encryption keys requires current_verifier";
const WRONG_PASSWORD_ERROR: &str = "current password is incorrect";
const RATE_LIMITED_ERROR: &str = "too many attempts, try again later";

/// The audit action a successful replacement records.
const KEYS_REPLACED_ACTION: &str = "auth.keys_replaced";

/// The SignIn quota: 10 attempts per 300 s from one address.
const SIGN_IN_QUOTA: usize = 10;

const K1_TAG: u8 = 0x10;
const K2_TAG: u8 = 0x60;

/// The five key columns of `users`, in schema order.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
struct KeyRow {
    encryption_public_key: Option<Vec<u8>>,
    encrypted_key_bundle: Option<Vec<u8>>,
    bundle_salt: Option<Vec<u8>>,
    encrypted_recovery_bundle: Option<Vec<u8>>,
    recovery_bundle_salt: Option<Vec<u8>>,
}

impl KeyRow {
    fn fields(&self) -> [(&'static str, &Option<Vec<u8>>); 5] {
        [
            ("encryption_public_key", &self.encryption_public_key),
            ("encrypted_key_bundle", &self.encrypted_key_bundle),
            ("bundle_salt", &self.bundle_salt),
            ("encrypted_recovery_bundle", &self.encrypted_recovery_bundle),
            ("recovery_bundle_salt", &self.recovery_bundle_salt),
        ]
    }
}

/// A full synthetic key set; two tags differ in every one of the five columns.
fn key_set(tag: u8) -> KeyRow {
    KeyRow {
        encryption_public_key: Some(vec![tag; 32]),
        encrypted_key_bundle: Some(vec![tag.wrapping_add(1); 72]),
        bundle_salt: Some(vec![tag.wrapping_add(2); 16]),
        encrypted_recovery_bundle: Some(vec![tag.wrapping_add(3); 72]),
        recovery_bundle_salt: Some(vec![tag.wrapping_add(4); 16]),
    }
}

fn no_keys() -> KeyRow {
    KeyRow {
        encryption_public_key: None,
        encrypted_key_bundle: None,
        bundle_salt: None,
        encrypted_recovery_bundle: None,
        recovery_bundle_salt: None,
    }
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(out, "{b:02x}").expect("writing to a String cannot fail");
    }
    out
}

fn key_fields(keys: &KeyRow) -> Map<String, Value> {
    let mut fields = Map::new();
    for (name, value) in keys.fields() {
        if let Some(bytes) = value {
            fields.insert(name.to_string(), Value::String(b64(bytes)));
        }
    }
    fields
}

fn key_body(keys: &KeyRow) -> Value {
    Value::Object(key_fields(keys))
}

fn key_body_with_verifier(keys: &KeyRow, verifier: &str) -> Value {
    let mut fields = key_fields(keys);
    fields.insert(
        "current_verifier".to_string(),
        Value::String(verifier.to_string()),
    );
    Value::Object(fields)
}

struct Harness {
    pool: PgPool,
    app: Router,
}

/// Connects via `KOMUN_TEST_DATABASE_URL`; without it the test panics rather than passing
/// unearned, and the URL is never printed.
async fn live_harness() -> Harness {
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

    let config: Config = toml::from_str("").expect("parse empty config");
    let state = AppState {
        pool: pool.clone(),
        config: Arc::new(config),
        rate_limiter: Arc::new(rate_limit::RateLimiter::new()),
        mailer: Arc::new(None),
        trusted_proxies: Arc::new(Vec::new()),
        salt_pepper: Arc::new(sessions::generate_pepper()),
    };
    let peer = SocketAddr::from(([203, 0, 113, 7], 40_000));
    let app = auth::router(state).layer(MockConnectInfo(peer));
    Harness { pool, app }
}

struct TestUser {
    id: Uuid,
    bearer: String,
}

/// Inserts a verified user with `keys`, then opens a session; the email is unique per call.
async fn seed_user(pool: &PgPool, keys: &KeyRow) -> TestUser {
    let id = Uuid::now_v7();
    let email = format!("r2-key-change-{id}@test.invalid");
    let password_hash = auth::password::hash_verifier(VERIFIER).expect("hash");
    sqlx::query(
        "INSERT INTO users (id, email, email_verified_at, password_hash, auth_salt, display_name,
                            encryption_public_key, encrypted_key_bundle, bundle_salt,
                            encrypted_recovery_bundle, recovery_bundle_salt)
         VALUES ($1, $2, now(), $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(id)
    .bind(email)
    .bind(password_hash)
    .bind(vec![0x5a_u8; 16])
    .bind("R2 test user")
    .bind(&keys.encryption_public_key)
    .bind(&keys.encrypted_key_bundle)
    .bind(&keys.bundle_salt)
    .bind(&keys.encrypted_recovery_bundle)
    .bind(&keys.recovery_bundle_salt)
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

async fn key_row(pool: &PgPool, user_id: Uuid) -> KeyRow {
    sqlx::query_as::<_, KeyRow>(
        "SELECT encryption_public_key, encrypted_key_bundle, bundle_salt,
                encrypted_recovery_bundle, recovery_bundle_salt
         FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .expect("read key columns")
}

/// Every column of the row; `users` has no `updated_at`, so this is the only way to see nothing
/// moved.
async fn full_row(pool: &PgPool, user_id: Uuid) -> Value {
    sqlx::query_scalar::<_, Value>("SELECT to_jsonb(u) FROM users u WHERE u.id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .expect("read full user row")
}

/// Every audit row naming the user as actor or subject, as `(action, detail)`.
async fn audit_rows(pool: &PgPool, user_id: Uuid) -> Vec<(String, Option<Value>)> {
    sqlx::query_as::<_, (String, Option<Value>)>(
        "SELECT action, detail FROM audit_events
         WHERE actor_id = $1 OR subject_id = $1
         ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .expect("read audit rows")
}

async fn put_me(app: &Router, bearer: &str, body: &Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/me")
                .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("the router is infallible");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("read response body");
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

/// The `error` field only: a body may echo key fields on success.
fn error_of(body: &Value) -> &str {
    body.get("error").and_then(Value::as_str).unwrap_or("")
}

/// Asserts no audit row carries the given key bytes, in base64 or hex.
fn assert_no_key_material(rows: &[(String, Option<Value>)], key_sets: &[&KeyRow]) {
    for (action, detail) in rows {
        let text = format!("{action} {}", detail.clone().unwrap_or(Value::Null));
        for keys in key_sets {
            for (name, value) in keys.fields() {
                if let Some(bytes) = value {
                    assert!(
                        !text.contains(&b64(bytes)) && !text.contains(&hex(bytes)),
                        "audit row {action:?} carries {name} key material"
                    );
                }
            }
        }
    }
}

/// Replacing stored keys with no `current_verifier` is refused and moves no key column.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn c1a_replacing_stored_keys_without_a_verifier_is_refused() {
    let h = live_harness().await;
    let k1 = key_set(K1_TAG);
    let k2 = key_set(K2_TAG);
    let user = seed_user(&h.pool, &k1).await;

    let (status, body) = put_me(&h.app, &user.bearer, &key_body(&k2)).await;

    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "error: {:?}",
        error_of(&body)
    );
    assert_eq!(error_of(&body), MISSING_VERIFIER_ERROR);
    assert_eq!(
        key_row(&h.pool, user.id).await,
        k1,
        "a refused change must leave all five key columns as they were"
    );
}

/// Replacing stored keys with a wrong `current_verifier` is refused and moves no key column.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn c1b_replacing_stored_keys_with_a_wrong_verifier_is_refused() {
    let h = live_harness().await;
    let k1 = key_set(K1_TAG);
    let k2 = key_set(K2_TAG);
    let user = seed_user(&h.pool, &k1).await;

    let body = key_body_with_verifier(&k2, WRONG_VERIFIER);
    let (status, body) = put_me(&h.app, &user.bearer, &body).await;

    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "error: {:?}",
        error_of(&body)
    );
    assert_eq!(error_of(&body), WRONG_PASSWORD_ERROR);
    assert_eq!(
        key_row(&h.pool, user.id).await,
        k1,
        "a refused change must leave all five key columns as they were"
    );
}

/// Replacing stored keys with the correct verifier writes all five and audits one row.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn c1c_replacing_stored_keys_with_the_correct_verifier_succeeds_and_is_audited() {
    let h = live_harness().await;
    let k1 = key_set(K1_TAG);
    let k2 = key_set(K2_TAG);
    let user = seed_user(&h.pool, &k1).await;

    let body = key_body_with_verifier(&k2, VERIFIER);
    let (status, body) = put_me(&h.app, &user.bearer, &body).await;

    assert!(status.is_success(), "got {status}: {:?}", error_of(&body));
    assert_eq!(
        key_row(&h.pool, user.id).await,
        k2,
        "a verified replacement must write all five key columns"
    );

    let rows = audit_rows(&h.pool, user.id).await;
    let replaced = rows
        .iter()
        .filter(|(action, _)| action == KEYS_REPLACED_ACTION)
        .count();
    assert_eq!(replaced, 1, "want exactly one {KEYS_REPLACED_ACTION} row");
    assert_no_key_material(&rows, &[&k1, &k2]);
}

/// Wrong verifiers are charged to the SignIn bucket, so the attempt past the quota is 429.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn c1d_wrong_verifiers_are_rate_limited_like_sign_in() {
    let h = live_harness().await;
    let k1 = key_set(K1_TAG);
    let k2 = key_set(K2_TAG);
    let user = seed_user(&h.pool, &k1).await;
    let body = key_body_with_verifier(&k2, WRONG_VERIFIER);

    for attempt in 1..=SIGN_IN_QUOTA {
        let (status, response) = put_me(&h.app, &user.bearer, &body).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "attempt {attempt}: {:?}",
            error_of(&response)
        );
    }

    let (status, response) = put_me(&h.app, &user.bearer, &body).await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "attempt {}: {:?}",
        SIGN_IN_QUOTA + 1,
        error_of(&response)
    );
    assert_eq!(error_of(&response), RATE_LIMITED_ERROR);
    assert_eq!(
        key_row(&h.pool, user.id).await,
        k1,
        "no wrong-verifier attempt may move a key column"
    );
}

/// The first key set on an account with none stored is accepted on the session alone.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn c2_first_time_key_setup_needs_only_the_session() {
    let h = live_harness().await;
    let k1 = key_set(K1_TAG);
    let user = seed_user(&h.pool, &no_keys()).await;

    let (status, body) = put_me(&h.app, &user.bearer, &key_body(&k1)).await;

    assert!(status.is_success(), "got {status}: {:?}", error_of(&body));
    assert_eq!(
        key_row(&h.pool, user.id).await,
        k1,
        "a first-time setup must write all five key columns"
    );
}

/// An unchanged re-upload with no verifier succeeds and changes nothing.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn c3_identical_reupload_is_accepted_and_changes_nothing() {
    let h = live_harness().await;
    let k1 = key_set(K1_TAG);
    let user = seed_user(&h.pool, &k1).await;
    let row_before = full_row(&h.pool, user.id).await;
    let audit_before = audit_rows(&h.pool, user.id).await.len();

    let (status, body) = put_me(&h.app, &user.bearer, &key_body(&k1)).await;

    assert!(status.is_success(), "got {status}: {:?}", error_of(&body));
    assert!(
        full_row(&h.pool, user.id).await == row_before,
        "an identical re-upload must leave every users column unchanged"
    );
    assert_eq!(
        audit_rows(&h.pool, user.id).await.len(),
        audit_before,
        "an identical re-upload must not write an audit row"
    );
}

/// A partial key set against stored keys is refused and moves no key column.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn c4a_partial_key_set_over_stored_keys_is_refused() {
    let h = live_harness().await;
    let k1 = key_set(K1_TAG);
    let k2 = key_set(K2_TAG);
    let user = seed_user(&h.pool, &k1).await;
    let partial = KeyRow {
        encryption_public_key: k2.encryption_public_key.clone(),
        ..no_keys()
    };

    let (status, body) = put_me(&h.app, &user.bearer, &key_body(&partial)).await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "error: {:?}",
        error_of(&body)
    );
    assert_eq!(error_of(&body), PARTIAL_SET_ERROR);
    assert_eq!(
        key_row(&h.pool, user.id).await,
        k1,
        "a partial set must leave all five key columns as they were"
    );
}

/// A partial key set on an account with no keys is refused and leaves all five NULL.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn c4b_partial_key_set_with_no_keys_stored_is_refused() {
    let h = live_harness().await;
    let k1 = key_set(K1_TAG);
    let user = seed_user(&h.pool, &no_keys()).await;
    let partial = KeyRow {
        encryption_public_key: k1.encryption_public_key.clone(),
        ..no_keys()
    };

    let (status, body) = put_me(&h.app, &user.bearer, &key_body(&partial)).await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "error: {:?}",
        error_of(&body)
    );
    assert_eq!(error_of(&body), PARTIAL_SET_ERROR);
    assert_eq!(
        key_row(&h.pool, user.id).await,
        no_keys(),
        "a partial set must leave all five key columns NULL"
    );
}

/// A partial key set alongside a profile field refuses the whole request, profile included.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn c4c_partial_key_set_with_a_profile_field_refuses_the_whole_request() {
    let h = live_harness().await;
    let k1 = key_set(K1_TAG);
    let k2 = key_set(K2_TAG);
    let user = seed_user(&h.pool, &k1).await;
    let row_before = full_row(&h.pool, user.id).await;
    let bundle = k2
        .encrypted_key_bundle
        .as_deref()
        .expect("a full key set has a bundle");
    let body = json!({
        "display_name": "x",
        "encrypted_key_bundle": b64(bundle),
    });

    let (status, body) = put_me(&h.app, &user.bearer, &body).await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "error: {:?}",
        error_of(&body)
    );
    assert_eq!(error_of(&body), PARTIAL_SET_ERROR);
    let row_after = full_row(&h.pool, user.id).await;
    assert_eq!(
        row_after.get("display_name"),
        row_before.get("display_name"),
        "a refused request must not write its profile fields"
    );
    assert_eq!(
        key_row(&h.pool, user.id).await,
        k1,
        "a partial set must leave all five key columns as they were"
    );
}
