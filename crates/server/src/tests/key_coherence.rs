//! secaudit R2b: the signup, password-reset/confirm and change_password key writers must never
//! write a lone key column, or a new public key without the full set.
//!
//! Every test here needs a live Postgres, so every test is `#[ignore]`d and shows as ignored under
//! `cargo test --workspace`. Run them against a disposable database with:
//!
//! `KOMUN_TEST_DATABASE_URL=postgres://... cargo test -p komun-server key_coherence -- --ignored`
//!
//! The tests drive `crate::auth::router` over HTTP only and set up or inspect rows with direct
//! SQL. They name nothing from the fix itself: the refusal messages below are literals fixed by
//! plan dc2069e5, section 2, so the same file compiles against the unfixed tree and the fixed one.
//! Each test builds its own harness, and so its own rate limiter, and sends exactly one request to
//! the route under test, because password-reset/confirm allows only 3 attempts per hour. Each test
//! uses its own user and a unique email, so the tests run in any order against a shared database.
//!
//! All passwords and keys below are synthetic byte patterns, not real material. Even so, nothing
//! holding key bytes derives `Debug`, and every comparison of key bytes is an `assert!` whose
//! message names the column only (R2 review finding F6). Failure messages print the response's
//! `error` field and never the whole body.

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

const SIGNUP_PATH: &str = "/signup";
const RESET_PATH: &str = "/password-reset/confirm";
const CHANGE_PATH: &str = "/password/change";

/// Synthetic verifiers in the shape the client sends: base64 alphabet, at least 43 characters.
/// `VERIFIER` is the seeded password; `NEW_VERIFIER` is the password a reset or a change sets.
const VERIFIER: &str = "c3ludGhldGljLXRlc3QtdmVyaWZpZXItZm9yLWtleS1jb2hlcmVuY2U";
const NEW_VERIFIER: &str = "bmV3LXN5bnRoZXRpYy12ZXJpZmllci1mb3Ita2V5LWNvaGVyZW5jZQ";

/// Synthetic `auth_salt` values, 16 bytes each: the minimum the handlers accept.
const SEEDED_AUTH_SALT: [u8; 16] = [0x5a; 16];
const NEW_AUTH_SALT: [u8; 16] = [0xa5; 16];

/// Above the default `min_password_length` of 12.
const PASSWORD_LENGTH: usize = 16;

/// The refusal messages fixed by plan dc2069e5, section 2. The auth API has no error-code field,
/// so the message string is the code. `PARTIAL_SET_ERROR` is R2's text, which signup reuses, and
/// `PUBLIC_KEY_NEEDS_ALL_ERROR` is the reset guard's existing text, which reset keeps.
const PARTIAL_SET_ERROR: &str = "the encryption keys must be sent together: \
                                 encryption_public_key, encrypted_key_bundle, bundle_salt, \
                                 encrypted_recovery_bundle, recovery_bundle_salt";
const PUBLIC_KEY_NEEDS_ALL_ERROR: &str =
    "a new encryption_public_key must arrive with a new key bundle and recovery bundle";
const BUNDLE_PAIR_ERROR: &str = "encrypted_key_bundle and bundle_salt must be sent together";
const RECOVERY_PAIR_ERROR: &str =
    "encrypted_recovery_bundle and recovery_bundle_salt must be sent together";

const K1_TAG: u8 = 0x10;
const K2_TAG: u8 = 0x60;

/// The five key columns of `users`, in schema order.
///
/// It holds key bytes, so it deliberately derives no `Debug`: no failing assertion can print it.
/// Compare two of these with `assert_key_columns`, which names a differing column and nothing else.
#[derive(sqlx::FromRow)]
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

/// A full synthetic key set. Two different tags differ in every one of the five columns.
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

/// The request-body fields for every present column of `keys`, base64-encoded as the API expects.
fn key_fields(keys: &KeyRow) -> Map<String, Value> {
    let mut fields = Map::new();
    for (name, value) in keys.fields() {
        if let Some(bytes) = value {
            fields.insert(name.to_string(), Value::String(b64(bytes)));
        }
    }
    fields
}

/// Adds the present key columns of `keys` to a request body.
fn with_keys(base: Value, keys: &KeyRow) -> Value {
    let Value::Object(mut fields) = base else {
        panic!("a request body is a JSON object");
    };
    fields.extend(key_fields(keys));
    Value::Object(fields)
}

/// A signup body in the shape `signup` in web/src/lib/stores/auth.ts sends.
fn signup_body(email: &str, keys: &KeyRow) -> Value {
    let base = json!({
        "email": email,
        "display_name": "R2b test user",
        "verifier": VERIFIER,
        "auth_salt": b64(&NEW_AUTH_SALT),
        "password_length": PASSWORD_LENGTH,
        "invite_code": null,
    });
    with_keys(base, keys)
}

/// A reset body in the shape `confirmPasswordReset` in web/src/lib/stores/auth.ts sends.
fn reset_body(token: &ResetToken, keys: &KeyRow) -> Value {
    let base = json!({
        "token": token.raw,
        "verifier": NEW_VERIFIER,
        "auth_salt": b64(&NEW_AUTH_SALT),
        "password_length": PASSWORD_LENGTH,
    });
    with_keys(base, keys)
}

/// A change body in the shape `changePassword` in web/src/lib/stores/auth.ts sends.
fn change_body(keys: &KeyRow) -> Value {
    let base = json!({
        "current_verifier": VERIFIER,
        "verifier": NEW_VERIFIER,
        "auth_salt": b64(&NEW_AUTH_SALT),
        "password_length": PASSWORD_LENGTH,
    });
    with_keys(base, keys)
}

/// A unique signup address. `Uuid` renders in lower case, which the `users.email` CHECK requires.
fn signup_email() -> String {
    format!("r2b-signup-{}@test.invalid", Uuid::now_v7())
}

struct Harness {
    pool: PgPool,
    app: Router,
}

/// Connects to the database named by `KOMUN_TEST_DATABASE_URL` and builds the auth router over it,
/// with a fresh rate limiter.
///
/// Without the variable this panics before any database work: an ignored test run on purpose
/// must not report a pass it never earned. The URL itself is never printed, because it may carry a
/// password.
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

/// Inserts a verified user whose password verifier is `VERIFIER` and whose key columns are `keys`,
/// then opens a session for it. The email is unique per call.
async fn seed_user(pool: &PgPool, keys: &KeyRow) -> TestUser {
    let id = Uuid::now_v7();
    let email = format!("r2b-key-coherence-{id}@test.invalid");
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
    .bind(SEEDED_AUTH_SALT.as_slice())
    .bind("R2b test user")
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

/// A password-reset token minted straight into `one_time_tokens`, as the reset mail would carry it.
struct ResetToken {
    raw: String,
    hash: Vec<u8>,
}

async fn mint_reset_token(pool: &PgPool, user_id: Uuid) -> ResetToken {
    let token = sessions::generate_token();
    db::sessions::create_one_time_token(
        pool,
        user_id,
        db::sessions::KIND_PASSWORD_RESET,
        &token.hash,
        sessions::PASSWORD_RESET_TTL_MINUTES,
    )
    .await
    .expect("mint a reset token");
    ResetToken {
        raw: token.raw,
        hash: token.hash,
    }
}

/// True while the reset token has not been consumed.
async fn token_unused(pool: &PgPool, token: &ResetToken) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT used_at IS NULL FROM one_time_tokens WHERE token_hash = $1",
    )
    .bind(&token.hash)
    .fetch_one(pool)
    .await
    .expect("read the reset token")
}

async fn user_id_by_email(pool: &PgPool, email: &str) -> Option<Uuid> {
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE email = $1")
        .bind(email)
        .fetch_optional(pool)
        .await
        .expect("look up the user by email")
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

/// Every column of the user's row, as Postgres renders it. `users` has no `updated_at`, so this
/// snapshot is the only way to see that nothing moved.
async fn full_row(pool: &PgPool, user_id: Uuid) -> Value {
    sqlx::query_scalar::<_, Value>("SELECT to_jsonb(u) FROM users u WHERE u.id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .expect("read full user row")
}

/// Everything a refused request must leave alone: the five key columns, the two password columns
/// and, as a backstop, the whole row. It holds key bytes, so it derives no `Debug`.
struct Snapshot {
    keys: KeyRow,
    password_hash: String,
    auth_salt: Vec<u8>,
    row: Value,
}

const PASSWORD_COLUMNS: &str = "SELECT password_hash, auth_salt FROM users WHERE id = $1";

async fn snapshot(pool: &PgPool, user_id: Uuid) -> Snapshot {
    let (password_hash, auth_salt) = sqlx::query_as::<_, (String, Vec<u8>)>(PASSWORD_COLUMNS)
        .bind(user_id)
        .fetch_one(pool)
        .await
        .expect("read password columns");
    Snapshot {
        keys: key_row(pool, user_id).await,
        password_hash,
        auth_salt,
        row: full_row(pool, user_id).await,
    }
}

/// Compares the five key columns one by one. A mismatch names the column and prints no bytes.
fn assert_key_columns(actual: &KeyRow, expected: &KeyRow, context: &str) {
    for ((name, got), (_, want)) in actual.fields().into_iter().zip(expected.fields()) {
        assert!(got == want, "{context}: {name} is not the expected value");
    }
}

/// Asserts that a refused request moved nothing. Each message names a column, never its value.
fn assert_unchanged(before: &Snapshot, after: &Snapshot) {
    assert_key_columns(&after.keys, &before.keys, "refused request");
    assert!(
        after.password_hash == before.password_hash,
        "password_hash changed"
    );
    assert!(after.auth_salt == before.auth_salt, "auth_salt changed");
    assert!(after.row == before.row, "the users row changed");
}

async fn post_json(
    app: &Router,
    path: &str,
    bearer: Option<&str>,
    body: &Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(bearer) = bearer {
        let value = format!("Bearer {bearer}");
        request = request.header(header::AUTHORIZATION, value);
    }
    let request = request
        .body(Body::from(body.to_string()))
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

/// The `error` field of a response. Assertion messages print this and never the whole body, which
/// on success may echo key fields.
fn error_of(body: &Value) -> &str {
    body.get("error").and_then(Value::as_str).unwrap_or("")
}

/// Asserts a 400 and, when one is given, its error message.
fn assert_bad_request(status: StatusCode, body: &Value, expected_error: Option<&str>) {
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "error: {:?}",
        error_of(body)
    );
    if let Some(expected) = expected_error {
        assert_eq!(error_of(body), expected);
    }
}

fn assert_success(status: StatusCode, body: &Value) {
    assert!(status.is_success(), "got {status}: {:?}", error_of(body));
}

/// SU1: a signup carrying a partial key set is refused with R2's partial-set message, and no
/// `users` row exists for its email afterwards.
async fn signup_with_partial_set_is_refused(sent: KeyRow) {
    let h = live_harness().await;
    let email = signup_email();

    let body = signup_body(&email, &sent);
    let (status, body) = post_json(&h.app, SIGNUP_PATH, None, &body).await;

    assert_bad_request(status, &body, Some(PARTIAL_SET_ERROR));
    assert!(
        user_id_by_email(&h.pool, &email).await.is_none(),
        "a refused signup must not create a user row"
    );
}

/// RC1/RC2: a reset with incoherent key fields, against a user holding K1, is refused. It moves no
/// key column, no password column and nothing else in the row, and the reset token stays unused.
async fn reset_with_incoherent_keys_is_refused(sent: KeyRow, expected_error: &str) {
    let h = live_harness().await;
    let user = seed_user(&h.pool, &key_set(K1_TAG)).await;
    let token = mint_reset_token(&h.pool, user.id).await;
    let before = snapshot(&h.pool, user.id).await;

    let body = reset_body(&token, &sent);
    let (status, body) = post_json(&h.app, RESET_PATH, None, &body).await;

    assert_bad_request(status, &body, Some(expected_error));
    assert_unchanged(&before, &snapshot(&h.pool, user.id).await);
    assert!(
        token_unused(&h.pool, &token).await,
        "a refused reset must leave the reset token unused"
    );
}

/// CP1/CP2: a password change carrying one of the bundle pair alone is refused, and the user's row
/// is unchanged. `expected_error` is `None` where the plan does not pin the message.
async fn change_with_a_lone_bundle_column_is_refused(
    stored: KeyRow,
    sent: KeyRow,
    expected_error: Option<&str>,
) {
    let h = live_harness().await;
    let user = seed_user(&h.pool, &stored).await;
    let before = snapshot(&h.pool, user.id).await;

    let body = change_body(&sent);
    let (status, body) = post_json(&h.app, CHANGE_PATH, Some(&user.bearer), &body).await;

    assert_bad_request(status, &body, expected_error);
    assert_unchanged(&before, &snapshot(&h.pool, user.id).await);
}

/// SU1a: signup with `encryption_public_key` alone is refused.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn su1a_signup_with_the_public_key_alone_is_refused() {
    let k1 = key_set(K1_TAG);
    let sent = KeyRow {
        encryption_public_key: k1.encryption_public_key,
        ..no_keys()
    };
    signup_with_partial_set_is_refused(sent).await;
}

/// SU1b: signup with the key-bundle pair alone is refused.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn su1b_signup_with_the_bundle_pair_alone_is_refused() {
    let k1 = key_set(K1_TAG);
    let sent = KeyRow {
        encrypted_key_bundle: k1.encrypted_key_bundle,
        bundle_salt: k1.bundle_salt,
        ..no_keys()
    };
    signup_with_partial_set_is_refused(sent).await;
}

/// SU1c: signup with every key column except `recovery_bundle_salt` is refused.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn su1c_signup_without_the_recovery_bundle_salt_is_refused() {
    let sent = KeyRow {
        recovery_bundle_salt: None,
        ..key_set(K1_TAG)
    };
    signup_with_partial_set_is_refused(sent).await;
}

/// SU1d: signup with the recovery pair alone is refused.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn su1d_signup_with_the_recovery_pair_alone_is_refused() {
    let k1 = key_set(K1_TAG);
    let sent = KeyRow {
        encrypted_recovery_bundle: k1.encrypted_recovery_bundle,
        recovery_bundle_salt: k1.recovery_bundle_salt,
        ..no_keys()
    };
    signup_with_partial_set_is_refused(sent).await;
}

/// SU2: signup with all five key columns, the web client's shape, writes all five.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn su2_signup_with_all_five_keys_writes_them() {
    let h = live_harness().await;
    let email = signup_email();
    let k1 = key_set(K1_TAG);

    let body = signup_body(&email, &k1);
    let (status, body) = post_json(&h.app, SIGNUP_PATH, None, &body).await;

    assert_success(status, &body);
    let id = user_id_by_email(&h.pool, &email)
        .await
        .expect("an accepted signup creates the user row");
    let after = key_row(&h.pool, id).await;
    assert_key_columns(&after, &k1, "signup with all five");
}

/// SU3: signup with no key column is accepted and leaves all five NULL.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn su3_signup_with_no_keys_leaves_all_five_null() {
    let h = live_harness().await;
    let email = signup_email();

    let body = signup_body(&email, &no_keys());
    let (status, body) = post_json(&h.app, SIGNUP_PATH, None, &body).await;

    assert_success(status, &body);
    let id = user_id_by_email(&h.pool, &email)
        .await
        .expect("an accepted signup creates the user row");
    let after = key_row(&h.pool, id).await;
    assert_key_columns(&after, &no_keys(), "signup with none");
}

/// RC1a: a reset with `encrypted_key_bundle` alone is refused.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn rc1a_reset_with_the_key_bundle_alone_is_refused() {
    let k2 = key_set(K2_TAG);
    let sent = KeyRow {
        encrypted_key_bundle: k2.encrypted_key_bundle,
        ..no_keys()
    };
    reset_with_incoherent_keys_is_refused(sent, BUNDLE_PAIR_ERROR).await;
}

/// RC1b: a reset with `bundle_salt` alone is refused.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn rc1b_reset_with_the_bundle_salt_alone_is_refused() {
    let k2 = key_set(K2_TAG);
    let sent = KeyRow {
        bundle_salt: k2.bundle_salt,
        ..no_keys()
    };
    reset_with_incoherent_keys_is_refused(sent, BUNDLE_PAIR_ERROR).await;
}

/// RC1c: a reset with `encrypted_recovery_bundle` alone is refused.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn rc1c_reset_with_the_recovery_bundle_alone_is_refused() {
    let k2 = key_set(K2_TAG);
    let sent = KeyRow {
        encrypted_recovery_bundle: k2.encrypted_recovery_bundle,
        ..no_keys()
    };
    reset_with_incoherent_keys_is_refused(sent, RECOVERY_PAIR_ERROR).await;
}

/// RC1d: a reset with `recovery_bundle_salt` alone is refused.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn rc1d_reset_with_the_recovery_bundle_salt_alone_is_refused() {
    let k2 = key_set(K2_TAG);
    let sent = KeyRow {
        recovery_bundle_salt: k2.recovery_bundle_salt,
        ..no_keys()
    };
    reset_with_incoherent_keys_is_refused(sent, RECOVERY_PAIR_ERROR).await;
}

/// RC2a: a reset with a new `encryption_public_key` alone is refused with the existing message.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn rc2a_reset_with_the_public_key_alone_is_refused() {
    let k2 = key_set(K2_TAG);
    let sent = KeyRow {
        encryption_public_key: k2.encryption_public_key,
        ..no_keys()
    };
    reset_with_incoherent_keys_is_refused(sent, PUBLIC_KEY_NEEDS_ALL_ERROR).await;
}

/// RC2b: a reset with a new public key and the bundle pair, but no recovery pair, is refused.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn rc2b_reset_with_a_public_key_and_only_the_bundle_pair_is_refused() {
    let k2 = key_set(K2_TAG);
    let sent = KeyRow {
        encryption_public_key: k2.encryption_public_key,
        encrypted_key_bundle: k2.encrypted_key_bundle,
        bundle_salt: k2.bundle_salt,
        ..no_keys()
    };
    reset_with_incoherent_keys_is_refused(sent, PUBLIC_KEY_NEEDS_ALL_ERROR).await;
}

/// RC3: a reset with a new bundle pair, the recovery-code shape (auth.ts:451-452), writes the pair
/// and leaves the public key and the recovery pair as they were.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn rc3_reset_with_a_new_bundle_pair_writes_only_the_pair() {
    let h = live_harness().await;
    let k1 = key_set(K1_TAG);
    let k2 = key_set(K2_TAG);
    let user = seed_user(&h.pool, &k1).await;
    let token = mint_reset_token(&h.pool, user.id).await;
    let sent = KeyRow {
        encrypted_key_bundle: k2.encrypted_key_bundle,
        bundle_salt: k2.bundle_salt,
        ..no_keys()
    };

    let body = reset_body(&token, &sent);
    let (status, body) = post_json(&h.app, RESET_PATH, None, &body).await;

    assert_success(status, &body);
    let expected = KeyRow {
        encrypted_key_bundle: sent.encrypted_key_bundle,
        bundle_salt: sent.bundle_salt,
        ..k1
    };
    let after = key_row(&h.pool, user.id).await;
    assert_key_columns(&after, &expected, "reset with a bundle pair");
}

/// RC4: a reset with all five new key columns, the no-recovery-code shape (auth.ts:454-458),
/// writes all five.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn rc4_reset_with_all_five_new_keys_writes_them() {
    let h = live_harness().await;
    let k2 = key_set(K2_TAG);
    let user = seed_user(&h.pool, &key_set(K1_TAG)).await;
    let token = mint_reset_token(&h.pool, user.id).await;

    let body = reset_body(&token, &k2);
    let (status, body) = post_json(&h.app, RESET_PATH, None, &body).await;

    assert_success(status, &body);
    let after = key_row(&h.pool, user.id).await;
    assert_key_columns(&after, &k2, "reset with all five");
}

/// CP1a: a password change with `encrypted_key_bundle` alone, on an account with no keys, is
/// refused.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn cp1a_change_with_the_key_bundle_alone_and_no_stored_keys_is_refused() {
    let k2 = key_set(K2_TAG);
    let sent = KeyRow {
        encrypted_key_bundle: k2.encrypted_key_bundle,
        ..no_keys()
    };
    change_with_a_lone_bundle_column_is_refused(no_keys(), sent, Some(BUNDLE_PAIR_ERROR)).await;
}

/// CP1b: a password change with `bundle_salt` alone, on an account with no keys, is refused.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn cp1b_change_with_the_bundle_salt_alone_and_no_stored_keys_is_refused() {
    let k2 = key_set(K2_TAG);
    let sent = KeyRow {
        bundle_salt: k2.bundle_salt,
        ..no_keys()
    };
    change_with_a_lone_bundle_column_is_refused(no_keys(), sent, Some(BUNDLE_PAIR_ERROR)).await;
}

/// CP2a: a password change with `encrypted_key_bundle` alone, on an account holding K1, is
/// refused. The message is not pinned: the existing guard refuses this shape before the fix.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn cp2a_change_with_the_key_bundle_alone_over_stored_keys_is_refused() {
    let k2 = key_set(K2_TAG);
    let sent = KeyRow {
        encrypted_key_bundle: k2.encrypted_key_bundle,
        ..no_keys()
    };
    change_with_a_lone_bundle_column_is_refused(key_set(K1_TAG), sent, None).await;
}

/// CP2b: a password change with `bundle_salt` alone, on an account holding K1, is refused.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn cp2b_change_with_the_bundle_salt_alone_over_stored_keys_is_refused() {
    let k2 = key_set(K2_TAG);
    let sent = KeyRow {
        bundle_salt: k2.bundle_salt,
        ..no_keys()
    };
    change_with_a_lone_bundle_column_is_refused(key_set(K1_TAG), sent, None).await;
}

/// CP3: a password change with a new bundle pair, the web client's shape (auth.ts:537-541), writes
/// the pair and leaves the public key and the recovery pair as they were.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn cp3_change_with_a_new_bundle_pair_writes_only_the_pair() {
    let h = live_harness().await;
    let k1 = key_set(K1_TAG);
    let k2 = key_set(K2_TAG);
    let user = seed_user(&h.pool, &k1).await;
    let sent = KeyRow {
        encrypted_key_bundle: k2.encrypted_key_bundle,
        bundle_salt: k2.bundle_salt,
        ..no_keys()
    };

    let body = change_body(&sent);
    let (status, body) = post_json(&h.app, CHANGE_PATH, Some(&user.bearer), &body).await;

    assert_success(status, &body);
    let expected = KeyRow {
        encrypted_key_bundle: sent.encrypted_key_bundle,
        bundle_salt: sent.bundle_salt,
        ..k1
    };
    let after = key_row(&h.pool, user.id).await;
    assert_key_columns(&after, &expected, "change with a bundle pair");
}

/// CP4: a password change with no key field, on an account with no keys, is accepted and leaves
/// all five NULL.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn cp4_change_with_no_key_fields_and_no_stored_keys_is_accepted() {
    let h = live_harness().await;
    let user = seed_user(&h.pool, &no_keys()).await;

    let body = change_body(&no_keys());
    let (status, body) = post_json(&h.app, CHANGE_PATH, Some(&user.bearer), &body).await;

    assert_success(status, &body);
    let after = key_row(&h.pool, user.id).await;
    assert_key_columns(&after, &no_keys(), "change with no key field");
}
