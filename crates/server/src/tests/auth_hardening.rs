//! Account routes: the reset bundle is read with the token in a request body, never in a URL; a
//! reset or verification request answers without waiting on mail delivery; an unverified account
//! keeps its profile read and its session routes but cannot change its profile or avatar; a
//! refused signup leaves its invite use unspent; replacing an avatar removes the previous file.
//!
//! Every test needs a live Postgres, so every test is `#[ignore]`d; run against a disposable
//! database with `KOMUN_TEST_DATABASE_URL=postgres://... cargo test -p komun-server
//! auth_hardening -- --ignored`. Each test builds its own harness, rate limiter, users and avatar
//! directory, so they run in any order. All verifiers, keys and tokens are synthetic.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    extract::connect_info::MockConnectInfo,
    http::{header, Method, Request, StatusCode},
    Router,
};
use base64::Engine;
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tokio::net::TcpListener;
use tower::ServiceExt;
use uuid::Uuid;

use crate::auth::email::Mailer;
use crate::config::Config;
use crate::{auth, db, rate_limit, sessions, AppState};

const DATABASE_ENV: &str = "KOMUN_TEST_DATABASE_URL";

/// A synthetic verifier in the shape the client sends; it is not real material.
const VERIFIER: &str = "c3ludGhldGljLXRlc3QtdmVyaWZpZXItZm9yLWF1dGgtaGFyZGVuaW5n";
const AUTH_SALT: [u8; 16] = [0x5a; 16];
/// Above the default `min_password_length` of 12.
const PASSWORD_LENGTH: usize = 16;
const DISPLAY_NAME: &str = "Auth hardening test user";

/// The refusal and uniform messages are the API's error codes: it has no error-code field.
const RESET_REQUESTED: &str = "if that address has an account here, a reset link is on its way";
const VERIFICATION_REQUESTED: &str =
    "if that address has an unverified account here, a new link is on its way";
const RESET_LINK_REFUSED: &str = "this reset link is invalid, already used, or expired";
const INVITE_REFUSED: &str = "that invite code is not valid or has been used up";

/// Ample for a local round trip, and far below any SMTP client timeout.
const ANSWER_WITHIN: Duration = Duration::from_secs(2);

const BUNDLE_PATH: &str = "/password-reset/bundle";
const INVITE_CONFIG: &str = "[registration]\nmode = \"invite\"\n";
const BOUNDARY: &str = "komun-auth-hardening-boundary";
const RESPONSE_LIMIT: usize = 64 * 1024;

const USES_REMAINING: &str = "SELECT uses_remaining FROM invites WHERE code = $1";
const DISPLAY_NAME_OF: &str = "SELECT display_name FROM users WHERE id = $1";
const AVATAR_PATH_OF: &str = "SELECT avatar_path FROM users WHERE id = $1";

struct Harness {
    pool: PgPool,
    app: Router,
    media: PathBuf,
}

impl Harness {
    fn avatars(&self) -> PathBuf {
        self.media.join("avatars")
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.media);
    }
}

/// Connects via `KOMUN_TEST_DATABASE_URL`; without it the test panics rather than passing
/// unearned, and the URL is never printed. Avatars land in a directory of the test's own, never
/// under `data/`.
async fn live_harness(config_toml: &str) -> Harness {
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

    let media_name = format!("komun-auth-hardening-{}", Uuid::now_v7());
    let media = std::env::temp_dir().join(media_name);
    let mut config: Config = toml::from_str(config_toml).expect("parse test config");
    config.media.avatar_dir = media.join("avatars").to_string_lossy().into_owned();
    std::fs::create_dir_all(&config.media.avatar_dir)
        .expect("create the test avatar directory");
    let mailer = Mailer::from_config(&config).expect("build the test mailer");

    let state = AppState {
        pool: pool.clone(),
        config: Arc::new(config),
        rate_limiter: Arc::new(rate_limit::RateLimiter::new()),
        mailer: Arc::new(mailer),
        trusted_proxies: Arc::new(Vec::new()),
        salt_pepper: Arc::new(sessions::generate_pepper()),
    };
    let peer = SocketAddr::from(([203, 0, 113, 10], 40_000));
    let app = auth::router(state).layer(MockConnectInfo(peer));
    Harness { pool, app, media }
}

/// Accepts connections and never writes a byte. Both loopback families listen on one port, so
/// `localhost` stalls whichever address it resolves to first.
async fn stalled_mail_server() -> u16 {
    let v4 = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind a local port");
    let port = v4.local_addr().expect("read the local address").port();
    tokio::spawn(hold_connections(v4));
    if let Ok(v6) = TcpListener::bind((Ipv6Addr::LOCALHOST, port)).await {
        tokio::spawn(hold_connections(v6));
    }
    port
}

async fn hold_connections(listener: TcpListener) {
    let mut held = Vec::new();
    while let Ok((stream, _)) = listener.accept().await {
        held.push(stream);
    }
}

fn stalled_mail_config(port: u16) -> String {
    format!(
        "[email]\nsmtp_host = \"localhost\"\nsmtp_port = {port}\n\
         from = \"Komun <noreply@test.invalid>\"\nstarttls = false\n\
         public_url = \"https://komun.test.invalid\"\n"
    )
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// An address no account holds.
fn unused_email() -> String {
    format!("auth-hardening-unused-{}@test.invalid", Uuid::now_v7())
}

struct TestUser {
    id: Uuid,
    email: String,
    bearer: String,
}

async fn seed_user(pool: &PgPool, verified: bool) -> TestUser {
    let id = Uuid::now_v7();
    let email = format!("auth-hardening-{id}@test.invalid");
    let password_hash = auth::password::hash_verifier(VERIFIER).expect("hash");
    sqlx::query(
        "INSERT INTO users (id, email, email_verified_at, password_hash, auth_salt, display_name)
         VALUES ($1, $2, CASE WHEN $3 THEN now() END, $4, $5, $6)",
    )
    .bind(id)
    .bind(&email)
    .bind(verified)
    .bind(password_hash)
    .bind(AUTH_SALT.as_slice())
    .bind(DISPLAY_NAME)
    .execute(pool)
    .await
    .expect("insert test user");

    let bearer = open_session(pool, id).await;
    TestUser { id, email, bearer }
}

async fn open_session(pool: &PgPool, user_id: Uuid) -> String {
    let token = sessions::generate_token();
    db::sessions::create(pool, user_id, &token.hash, 1, None, None, None)
        .await
        .expect("create test session");
    token.raw
}

/// A full synthetic key set, so the account has a recovery bundle to read.
async fn store_key_set(pool: &PgPool, user_id: Uuid) {
    sqlx::query(
        "UPDATE users SET encryption_public_key = $2, encrypted_key_bundle = $3, bundle_salt = $4,
                          encrypted_recovery_bundle = $5, recovery_bundle_salt = $6
         WHERE id = $1",
    )
    .bind(user_id)
    .bind(vec![0x10_u8; 32])
    .bind(vec![0x11_u8; 72])
    .bind(vec![0x12_u8; 16])
    .bind(vec![0x13_u8; 72])
    .bind(vec![0x14_u8; 16])
    .execute(pool)
    .await
    .expect("store test keys");
}

async fn mint_reset_token(pool: &PgPool, user_id: Uuid) -> String {
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
    token.raw
}

async fn seed_invite(pool: &PgPool, created_by: Uuid, uses: i32) -> String {
    let code = format!("auth-hardening-{}", Uuid::now_v7().simple());
    sqlx::query(
        "INSERT INTO invites (code, created_by, uses_remaining, expires_at)
         VALUES ($1, $2, $3, now() + interval '1 day')",
    )
    .bind(&code)
    .bind(created_by)
    .bind(uses)
    .execute(pool)
    .await
    .expect("insert test invite");
    code
}

async fn uses_remaining(pool: &PgPool, code: &str) -> Option<i32> {
    sqlx::query_scalar::<_, Option<i32>>(USES_REMAINING)
        .bind(code)
        .fetch_one(pool)
        .await
        .expect("read the invite")
}

async fn display_name_of(pool: &PgPool, user_id: Uuid) -> String {
    sqlx::query_scalar::<_, String>(DISPLAY_NAME_OF)
        .bind(user_id)
        .fetch_one(pool)
        .await
        .expect("read the display name")
}

async fn avatar_path_of(pool: &PgPool, user_id: Uuid) -> Option<String> {
    sqlx::query_scalar::<_, Option<String>>(AVATAR_PATH_OF)
        .bind(user_id)
        .fetch_one(pool)
        .await
        .expect("read the avatar path")
}

fn files_in(dir: &Path) -> usize {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries.count(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(e) => panic!("read the avatar directory: {e}"),
    }
}

fn png(side: u32) -> Vec<u8> {
    let pixels = image::RgbImage::from_pixel(side, side, image::Rgb([48, 128, 192]));
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(pixels)
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("encode test PNG");
    out.into_inner()
}

fn builder(method: Method, uri: &str, bearer: Option<&str>) -> axum::http::request::Builder {
    let builder = Request::builder().method(method).uri(uri);
    match bearer {
        Some(bearer) => builder.header(header::AUTHORIZATION, format!("Bearer {bearer}")),
        None => builder,
    }
}

fn json_request(method: Method, uri: &str, bearer: Option<&str>, body: &Value) -> Request<Body> {
    builder(method, uri, bearer)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("build request")
}

fn empty_request(method: Method, uri: &str, bearer: Option<&str>) -> Request<Body> {
    builder(method, uri, bearer)
        .body(Body::empty())
        .expect("build request")
}

fn avatar_request(bearer: &str, data: &[u8]) -> Request<Body> {
    let head = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"image\"\r\n\
         Content-Type: image/png\r\n\r\n"
    );
    let mut body = head.into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    let content_type = format!("multipart/form-data; boundary={BOUNDARY}");
    builder(Method::POST, "/me/avatar", Some(bearer))
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .expect("build request")
}

fn reset_request(email: &str) -> Request<Body> {
    let body = json!({ "email": email });
    json_request(Method::POST, "/password-reset", None, &body)
}

fn signup_request(email: &str, invite_code: &str) -> Request<Body> {
    let body = json!({
        "email": email,
        "display_name": DISPLAY_NAME,
        "verifier": VERIFIER,
        "auth_salt": b64(&AUTH_SALT),
        "password_length": PASSWORD_LENGTH,
        "invite_code": invite_code,
    });
    json_request(Method::POST, "/signup", None, &body)
}

struct Reply {
    status: StatusCode,
    bytes: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes).unwrap_or(Value::Null)
    }

    /// The `error` field only: a success body may carry key material.
    fn error(&self) -> String {
        self.json()
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    }
}

async fn call(app: &Router, request: Request<Body>) -> Reply {
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router is infallible");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), RESPONSE_LIMIT)
        .await
        .expect("read response body")
        .to_vec();
    Reply { status, bytes }
}

/// Real time, not paused time: the wait being bounded is a network wait.
async fn call_within(app: &Router, request: Request<Body>, context: &str) -> Reply {
    match tokio::time::timeout(ANSWER_WITHIN, call(app, request)).await {
        Ok(reply) => reply,
        Err(_) => panic!("{context}: no answer within {ANSWER_WITHIN:?}"),
    }
}

fn assert_status(reply: &Reply, want: StatusCode, context: &str) {
    assert_eq!(reply.status, want, "{context}: error {:?}", reply.error());
}

fn assert_success(reply: &Reply, context: &str) {
    assert!(
        reply.status.is_success(),
        "{context}: got {}, error {:?}",
        reply.status,
        reply.error()
    );
}

// ---------------------------------------------------------------------------------------------
// The reset bundle is read with the token in a request body.
// ---------------------------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn reset_bundle_is_returned_for_a_token_in_a_post_body() {
    let h = live_harness("").await;
    let user = seed_user(&h.pool, true).await;
    store_key_set(&h.pool, user.id).await;
    let token = mint_reset_token(&h.pool, user.id).await;

    let body = json!({ "token": token });
    let request = json_request(Method::POST, BUNDLE_PATH, None, &body);
    let reply = call(&h.app, request).await;

    assert_status(&reply, StatusCode::OK, "bundle read by POST");
    let bundle = reply.json()["encrypted_recovery_bundle"].clone();
    assert!(
        bundle.as_str().is_some_and(|b| !b.is_empty()),
        "the reply must carry encrypted_recovery_bundle"
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn reset_bundle_is_not_served_for_a_token_in_the_url() {
    let h = live_harness("").await;
    let user = seed_user(&h.pool, true).await;
    store_key_set(&h.pool, user.id).await;
    let token = mint_reset_token(&h.pool, user.id).await;

    let uri = format!("{BUNDLE_PATH}?token={token}");
    let reply = call(&h.app, empty_request(Method::GET, &uri, None)).await;

    let context = "bundle read by GET";
    assert_status(&reply, StatusCode::METHOD_NOT_ALLOWED, context);
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn reset_bundle_post_with_an_unknown_token_is_refused() {
    let h = live_harness("").await;

    let body = json!({ "token": sessions::generate_token().raw });
    let request = json_request(Method::POST, BUNDLE_PATH, None, &body);
    let reply = call(&h.app, request).await;

    let context = "bundle read with an unknown token";
    assert_status(&reply, StatusCode::BAD_REQUEST, context);
    assert_eq!(reply.error(), RESET_LINK_REFUSED);
}

// ---------------------------------------------------------------------------------------------
// Reset and verification requests answer without waiting on mail delivery.
// ---------------------------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn password_reset_for_a_known_address_answers_without_waiting_for_mail() {
    let port = stalled_mail_server().await;
    let h = live_harness(&stalled_mail_config(port)).await;
    let user = seed_user(&h.pool, true).await;

    let request = reset_request(&unused_email());
    let unknown = call_within(&h.app, request, "reset for an unknown address").await;
    let request = reset_request(&user.email);
    let known = call_within(&h.app, request, "reset for a known address").await;

    assert_status(&known, StatusCode::OK, "reset for a known address");
    assert_eq!(known.json()["message"], json!(RESET_REQUESTED));
    assert!(
        known.bytes == unknown.bytes,
        "a known and an unknown address must get byte-identical replies"
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn password_reset_for_an_unknown_address_answers_with_the_uniform_message() {
    let port = stalled_mail_server().await;
    let h = live_harness(&stalled_mail_config(port)).await;

    let request = reset_request(&unused_email());
    let reply = call_within(&h.app, request, "reset for an unknown address").await;

    assert_status(&reply, StatusCode::OK, "reset for an unknown address");
    assert_eq!(reply.json()["message"], json!(RESET_REQUESTED));
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn resend_verification_answers_without_waiting_for_mail() {
    let port = stalled_mail_server().await;
    let h = live_harness(&stalled_mail_config(port)).await;
    let user = seed_user(&h.pool, false).await;

    let body = json!({ "email": user.email });
    let request = json_request(Method::POST, "/resend-verification", None, &body);
    let reply = call_within(&h.app, request, "resend-verification").await;

    assert_status(&reply, StatusCode::OK, "resend-verification");
    assert_eq!(reply.json()["message"], json!(VERIFICATION_REQUESTED));
}

// ---------------------------------------------------------------------------------------------
// An unverified account cannot change its profile or avatar, and keeps its session routes.
// ---------------------------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn unverified_account_cannot_change_its_profile() {
    let h = live_harness("").await;
    let user = seed_user(&h.pool, false).await;

    let body = json!({ "display_name": "x" });
    let request = json_request(Method::PUT, "/me", Some(&user.bearer), &body);
    let reply = call(&h.app, request).await;

    let context = "PUT /me from an unverified account";
    assert_status(&reply, StatusCode::FORBIDDEN, context);
    assert_eq!(display_name_of(&h.pool, user.id).await, DISPLAY_NAME);
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn unverified_account_cannot_upload_an_avatar() {
    let h = live_harness("").await;
    let user = seed_user(&h.pool, false).await;

    let reply = call(&h.app, avatar_request(&user.bearer, &png(64))).await;

    let context = "POST /me/avatar from an unverified account";
    assert_status(&reply, StatusCode::FORBIDDEN, context);
    assert_eq!(avatar_path_of(&h.pool, user.id).await, None);
    assert_eq!(files_in(&h.avatars()), 0, "{context}: no file written");
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn unverified_account_keeps_its_profile_sessions_and_sign_out() {
    let h = live_harness("").await;
    let user = seed_user(&h.pool, false).await;
    let second = open_session(&h.pool, user.id).await;

    let me = empty_request(Method::GET, "/me", Some(&user.bearer));
    assert_success(&call(&h.app, me).await, "GET /me");

    let list = empty_request(Method::GET, "/sessions", Some(&user.bearer));
    assert_success(&call(&h.app, list).await, "GET /sessions");

    let signout = empty_request(Method::POST, "/signout", Some(&second));
    assert_success(&call(&h.app, signout).await, "POST /signout");
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn verified_account_can_change_its_profile() {
    let h = live_harness("").await;
    let user = seed_user(&h.pool, true).await;

    let body = json!({ "display_name": "x" });
    let request = json_request(Method::PUT, "/me", Some(&user.bearer), &body);
    let reply = call(&h.app, request).await;

    assert_status(&reply, StatusCode::OK, "PUT /me from a verified account");
    assert_eq!(display_name_of(&h.pool, user.id).await, "x");
}

// ---------------------------------------------------------------------------------------------
// A signup spends an invite use only when it creates the account.
// ---------------------------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn signup_refused_for_a_taken_address_leaves_the_invite_use_unspent() {
    let h = live_harness(INVITE_CONFIG).await;
    let inviter = seed_user(&h.pool, true).await;
    let existing = seed_user(&h.pool, true).await;
    let code = seed_invite(&h.pool, inviter.id, 1).await;

    let reply = call(&h.app, signup_request(&existing.email, &code)).await;

    assert_status(&reply, StatusCode::CONFLICT, "signup with a taken address");
    assert_eq!(
        uses_remaining(&h.pool, &code).await,
        Some(1),
        "a refused signup must leave the invite use unspent"
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn signup_with_a_fresh_address_spends_one_invite_use() {
    let h = live_harness(INVITE_CONFIG).await;
    let inviter = seed_user(&h.pool, true).await;
    let code = seed_invite(&h.pool, inviter.id, 1).await;

    let reply = call(&h.app, signup_request(&unused_email(), &code)).await;

    assert_success(&reply, "signup with a fresh address");
    assert_eq!(uses_remaining(&h.pool, &code).await, Some(0));
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn signup_with_an_unknown_invite_is_refused() {
    let h = live_harness(INVITE_CONFIG).await;
    let code = format!("auth-hardening-missing-{}", Uuid::now_v7().simple());

    let reply = call(&h.app, signup_request(&unused_email(), &code)).await;

    let context = "signup with an unknown invite";
    assert_status(&reply, StatusCode::FORBIDDEN, context);
    assert_eq!(reply.error(), INVITE_REFUSED);
}

// ---------------------------------------------------------------------------------------------
// Replacing an avatar removes the previous file; a failed upload keeps the current one.
// ---------------------------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn replacing_an_avatar_removes_the_previous_file() {
    let h = live_harness("").await;
    let user = seed_user(&h.pool, true).await;
    let avatars = h.avatars();

    let first = call(&h.app, avatar_request(&user.bearer, &png(64))).await;
    assert_status(&first, StatusCode::OK, "first upload");
    let first_path = avatar_path_of(&h.pool, user.id)
        .await
        .expect("an accepted upload sets avatar_path");
    assert!(avatars.join(&first_path).is_file(), "first avatar file");

    let second = call(&h.app, avatar_request(&user.bearer, &png(65))).await;
    assert_status(&second, StatusCode::OK, "second upload");
    let second_path = avatar_path_of(&h.pool, user.id)
        .await
        .expect("an accepted upload sets avatar_path");

    assert_ne!(second_path, first_path, "each upload gets a new file name");
    assert!(avatars.join(&second_path).is_file(), "new avatar file");
    assert!(
        !avatars.join(&first_path).exists(),
        "the replaced avatar file must be removed"
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_failed_avatar_upload_keeps_the_current_one() {
    let h = live_harness("").await;
    let user = seed_user(&h.pool, true).await;

    let first = call(&h.app, avatar_request(&user.bearer, &png(64))).await;
    assert_status(&first, StatusCode::OK, "first upload");
    let current = avatar_path_of(&h.pool, user.id)
        .await
        .expect("an accepted upload sets avatar_path");

    let failed = call(&h.app, avatar_request(&user.bearer, b"not an image")).await;
    assert!(
        !failed.status.is_success(),
        "an upload that is not an image must be refused"
    );

    let after = avatar_path_of(&h.pool, user.id).await;
    assert_eq!(after.as_deref(), Some(current.as_str()));
    assert!(
        h.avatars().join(&current).is_file(),
        "a failed upload must leave the live avatar file"
    );
}
