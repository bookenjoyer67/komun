//! Bounded requests: post text fields and search terms have caps, `%` and `_` in a search term
//! match themselves, an image is decoded only within fixed limits and only as the type it claims,
//! an upload up to the configured size is accepted and a failed one leaves no file behind, the
//! hourly caps hold under concurrent requests, conversation lists come in pages, and a stalled or
//! oversized request body is cut off.
//!
//! Every test needs a live Postgres, so every test is `#[ignore]`d; run against a disposable
//! database with `KOMUN_TEST_DATABASE_URL=postgres://... cargo test -p komun-server
//! resource_limits -- --ignored`. Each test builds its own harness, users, posts and media
//! directory, so they run in any order. All data is synthetic.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use axum::{
    body::{Body, Bytes},
    extract::connect_info::MockConnectInfo,
    http::{header, Method, Request, StatusCode},
    Router,
};
use base64::Engine;
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use crate::config::Config;
use crate::{api, auth, db, rate_limit, sessions, AppState};

const DATABASE_ENV: &str = "KOMUN_TEST_DATABASE_URL";

/// A synthetic verifier in the shape the client sends; it is not real material.
const VERIFIER: &str = "c3ludGhldGljLXRlc3QtdmVyaWZpZXItZm9yLXIxMi1saW1pdHM";

const DIRECTORY_CONFIG: &str = "[discovery]\ndirectory_enabled = true\n";

/// Low enough that a race over the hourly count shows within `RACERS` requests.
const HOURLY_CAP: usize = 3;
const AVATARS_PER_HOUR: usize = 5;
const RACERS: usize = 8;

const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
const BOUNDARY: &str = "komun-r12-limits-boundary";
const STALLED_PREFIX: &[u8] = b"{\"kind\":\"n";

const POSTS_BY_AUTHOR: &str = "SELECT COUNT(*) FROM posts WHERE author_id = $1";
const MESSAGES_BY_SENDER: &str = "SELECT COUNT(*) FROM messages WHERE sender_id = $1";
const MATCHES_BY_RESPONDER: &str = "SELECT COUNT(*) FROM matches WHERE responder_id = $1";
const AVATAR_UPLOADS: &str = "SELECT COUNT(*) FROM avatar_uploads WHERE user_id = $1";

/// Written out by hand: the server's image build need not carry a GIF encoder.
const GIF_1X1: &[u8] = &[
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xff, 0xff, 0xff, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
];

fn low_hourly_caps() -> String {
    format!(
        "[security]\nmax_posts_per_hour = {HOURLY_CAP}\nmax_messages_per_hour = {HOURLY_CAP}\n\
         max_matches_per_hour = {HOURLY_CAP}\n"
    )
}

struct Harness {
    pool: PgPool,
    app: Router,
    media: PathBuf,
}

impl Harness {
    fn post_images(&self) -> PathBuf {
        self.media.join("post-images")
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.media);
    }
}

/// Connects via `KOMUN_TEST_DATABASE_URL`; without it the test panics rather than passing
/// unearned, and the URL is never printed. Uploads land in a directory of the test's own, never
/// under `data/`.
async fn live_harness(config_toml: &str) -> Harness {
    let Ok(url) = std::env::var(DATABASE_ENV) else {
        panic!("{DATABASE_ENV} is not set: point it at a disposable Postgres database");
    };
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&url)
        .await
        .expect("connect to the test database");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("apply migrations to the test database");

    let media_name = format!("komun-r12-{}", Uuid::now_v7());
    let media = std::env::temp_dir().join(media_name);
    let mut config: Config = toml::from_str(config_toml).expect("parse test config");
    config.media.post_images_dir = path_string(&media.join("post-images"));
    config.media.avatar_dir = path_string(&media.join("avatars"));

    let state = AppState {
        pool: pool.clone(),
        config: Arc::new(config),
        rate_limiter: Arc::new(rate_limit::RateLimiter::new()),
        mailer: Arc::new(None),
        trusted_proxies: Arc::new(Vec::new()),
        salt_pepper: Arc::new(sessions::generate_pepper()),
    };
    let peer = SocketAddr::from(([203, 0, 113, 12], 40_000));
    let app = api::router(state).layer(MockConnectInfo(peer));
    Harness { pool, app, media }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

struct TestUser {
    id: Uuid,
    bearer: String,
}

async fn seed_user(pool: &PgPool, display_name: &str) -> TestUser {
    let id = Uuid::now_v7();
    let email = format!("r12-limits-{id}@test.invalid");
    let password_hash = auth::password::hash_verifier(VERIFIER).expect("hash");
    sqlx::query(
        "INSERT INTO users (id, email, email_verified_at, password_hash, auth_salt, display_name, role)
         VALUES ($1, $2, now(), $3, $4, $5, 'user')",
    )
    .bind(id)
    .bind(email)
    .bind(password_hash)
    .bind(vec![0x5a_u8; 16])
    .bind(display_name)
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

/// Seeded straight into `posts`, so a test starts from the state under test without a route that
/// may itself refuse it.
async fn seed_post(pool: &PgPool, author: Uuid, kind: &str, title: &str) -> Uuid {
    let id = Uuid::now_v7();
    // `chk_posts_market_fields` allows price and condition on market kinds only.
    let market = matches!(kind, "listing" | "want");
    let category = if market { "electronics" } else { "food" };
    sqlx::query(
        "INSERT INTO posts (id, author_id, kind, category, title, status, visibility,
                            market_listed, price_cents, currency, item_condition)
         VALUES ($1, $2, $3, $4, $5, 'active', 'public', $6, $7, $8, $9)",
    )
    .bind(id)
    .bind(author)
    .bind(kind)
    .bind(category)
    .bind(title)
    .bind(market)
    .bind(market.then_some(2500_i64))
    .bind(market.then_some("EUR"))
    .bind(market.then_some("good"))
    .execute(pool)
    .await
    .expect("insert test post");
    id
}

async fn seed_match(pool: &PgPool, post: Uuid, responder: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO matches (id, post_id, responder_id, status) VALUES ($1, $2, $3, 'proposed')",
    )
    .bind(id)
    .bind(post)
    .bind(responder)
    .execute(pool)
    .await
    .expect("insert test match");
    id
}

async fn seed_directory_entry(pool: &PgPool, name: &str) -> String {
    let url = format!("https://r12-{}.test.invalid", Uuid::now_v7());
    sqlx::query("INSERT INTO directory_entries (url, name) VALUES ($1, $2)")
        .bind(&url)
        .bind(name)
        .execute(pool)
        .await
        .expect("insert directory entry");
    url
}

async fn count(pool: &PgPool, sql: &str, id: Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count rows")
}

async fn image_count(pool: &PgPool, post: Uuid) -> i32 {
    sqlx::query_scalar::<_, i32>(
        "SELECT COALESCE(array_length(images, 1), 0) FROM posts WHERE id = $1",
    )
    .bind(post)
    .fetch_one(pool)
    .await
    .expect("count post images")
}

/// Every column of the row: the only way to see that a refusal moved nothing.
async fn post_row(pool: &PgPool, id: Uuid) -> Value {
    sqlx::query_scalar::<_, Value>("SELECT to_jsonb(p) FROM posts p WHERE p.id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("read full post row")
}

fn files_in(dir: &Path) -> usize {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries.count(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(e) => panic!("read the media directory: {e}"),
    }
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

fn builder(method: Method, uri: &str, bearer: Option<&str>) -> axum::http::request::Builder {
    let builder = Request::builder().method(method).uri(uri);
    match bearer {
        Some(bearer) => builder.header(header::AUTHORIZATION, format!("Bearer {bearer}")),
        None => builder,
    }
}

fn get(uri: &str, bearer: Option<&str>) -> Request<Body> {
    builder(Method::GET, uri, bearer)
        .body(Body::empty())
        .expect("build request")
}

fn json_request(method: Method, uri: &str, bearer: &str, body: &Value) -> Request<Body> {
    builder(method, uri, Some(bearer))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("build request")
}

struct Part<'a> {
    name: &'a str,
    content_type: &'a str,
    data: &'a [u8],
}

fn image(data: &[u8]) -> Part<'_> {
    Part {
        name: "image",
        content_type: "image/png",
        data,
    }
}

fn multipart_body(parts: &[Part<'_>]) -> Vec<u8> {
    let mut body = Vec::new();
    for part in parts {
        let head = format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"; \
             filename=\"{name}\"\r\nContent-Type: {kind}\r\n\r\n",
            name = part.name,
            kind = part.content_type,
        );
        body.extend_from_slice(head.as_bytes());
        body.extend_from_slice(part.data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

fn multipart_request(uri: &str, bearer: &str, parts: &[Part<'_>]) -> Request<Body> {
    let content_type = format!("multipart/form-data; boundary={BOUNDARY}");
    builder(Method::POST, uri, Some(bearer))
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(multipart_body(parts)))
        .expect("build request")
}

async fn call(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router is infallible");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), RESPONSE_LIMIT)
        .await
        .expect("read response body");
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

/// Spawned rather than awaited in turn, so the requests overlap on several worker threads.
async fn all_at_once(app: &Router, requests: Vec<Request<Body>>) -> Vec<StatusCode> {
    let tasks: Vec<_> = requests
        .into_iter()
        .map(|request| {
            let app = app.clone();
            tokio::spawn(async move { call(&app, request).await.0 })
        })
        .collect();
    let mut statuses = Vec::with_capacity(tasks.len());
    for task in tasks {
        statuses.push(task.await.expect("request task"));
    }
    statuses
}

fn successes(statuses: &[StatusCode]) -> usize {
    statuses.iter().filter(|status| status.is_success()).count()
}

fn error_of(body: &Value) -> &str {
    body.get("error").and_then(Value::as_str).unwrap_or("")
}

fn assert_status(got: StatusCode, want: StatusCode, body: &Value, context: &str) {
    assert_eq!(got, want, "{context}: error {:?}", error_of(body));
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let pixels = image::RgbImage::from_pixel(width, height, image::Rgb([48, 128, 192]));
    encode_png(pixels)
}

/// Pseudo-random pixels, so deflate cannot shrink the file below its raw size.
fn incompressible_png(side: u32) -> Vec<u8> {
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let len = (side * side * 3) as usize;
    let raw: Vec<u8> = (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    let pixels = image::RgbImage::from_raw(side, side, raw).expect("raw size");
    encode_png(pixels)
}

fn encode_png(pixels: image::RgbImage) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(pixels)
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("encode test PNG");
    out.into_inner()
}

/// A synthetic sealed box: the server stores the bytes and never reads them.
fn sealed_message() -> Value {
    let engine = base64::engine::general_purpose::STANDARD;
    json!({
        "ciphertext": engine.encode([0x42_u8; 48]),
        "nonce": engine.encode([0x24_u8; 24]),
    })
}

// ---------------------------------------------------------------------------------------------
// Post text fields are capped, on create and on update, and refused rather than truncated.
// ---------------------------------------------------------------------------------------------

fn new_post(title: &str) -> Value {
    json!({ "kind": "need", "category": "food", "title": title })
}

async fn create(h: &Harness, user: &TestUser, body: &Value) -> (StatusCode, Value) {
    let request = json_request(Method::POST, "/posts", &user.bearer, body);
    call(&h.app, request).await
}

async fn create_is_refused(field: &str, value: Value) {
    let h = live_harness("").await;
    let author = seed_user(&h.pool, "R12 test user").await;
    let mut body = new_post("R12 capped post");
    body[field] = value;

    let (got, reply) = create(&h, &author, &body).await;

    let context = format!("create with {field} over its cap");
    assert_status(got, StatusCode::BAD_REQUEST, &reply, &context);
    assert_eq!(
        count(&h.pool, POSTS_BY_AUTHOR, author.id).await,
        0,
        "a refused create must store no post"
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn create_refuses_a_title_over_200_characters() {
    create_is_refused("title", json!("a".repeat(201))).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn create_refuses_a_body_over_10000_characters() {
    create_is_refused("body", json!("a".repeat(10_001))).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn create_refuses_a_location_name_over_200_characters() {
    create_is_refused("location_name", json!("a".repeat(201))).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn create_refuses_a_contact_method_over_200_characters() {
    create_is_refused("contact_method", json!("a".repeat(201))).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn create_refuses_more_than_10_tags() {
    create_is_refused("tags", json!(vec!["tag"; 11])).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn create_refuses_a_tag_over_32_characters() {
    create_is_refused("tags", json!(["a".repeat(33)])).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn create_refuses_a_negative_quantity() {
    create_is_refused("quantity", json!(-1)).await;
}

/// Two-byte characters, so a cap counted in bytes would refuse what a cap in characters accepts.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn create_accepts_every_field_at_its_cap() {
    let h = live_harness("").await;
    let author = seed_user(&h.pool, "R12 test user").await;
    let tag = "é".repeat(32);
    let body = json!({
        "kind": "need",
        "category": "food",
        "title": "é".repeat(200),
        "body": "é".repeat(10_000),
        "location_name": "é".repeat(200),
        "contact_method": "é".repeat(200),
        "tags": vec![tag; 10],
        "quantity": 1_000_000,
    });

    let (got, reply) = create(&h, &author, &body).await;

    let context = "create with every field at its cap";
    assert_status(got, StatusCode::OK, &reply, context);
    for field in ["title", "body", "location_name", "contact_method", "tags"] {
        assert_eq!(reply[field], body[field], "{field} is stored whole");
    }
    assert_eq!(reply["quantity"], body["quantity"]);
}

async fn update_is_refused(field: &str, value: Value) {
    let h = live_harness("").await;
    let author = seed_user(&h.pool, "R12 test user").await;
    let post = seed_post(&h.pool, author.id, "need", "R12 post to edit").await;
    let before = post_row(&h.pool, post).await;
    let mut edit = json!({});
    edit[field] = value;

    let uri = format!("/posts/{post}");
    let request = json_request(Method::PATCH, &uri, &author.bearer, &edit);
    let (got, reply) = call(&h.app, request).await;

    let context = format!("update with {field} over its cap");
    assert_status(got, StatusCode::BAD_REQUEST, &reply, &context);
    assert_eq!(
        post_row(&h.pool, post).await,
        before,
        "a refused update must leave the post unchanged"
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn update_refuses_a_title_over_200_characters() {
    update_is_refused("title", json!("a".repeat(201))).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn update_refuses_a_body_over_10000_characters() {
    update_is_refused("body", json!("a".repeat(10_001))).await;
}

// ---------------------------------------------------------------------------------------------
// `%` and `_` in a search term match themselves.
// ---------------------------------------------------------------------------------------------

/// A prefix no other row carries, so another test's data can neither match these terms nor
/// crowd the seeded rows out of a limited reply.
fn unique_token() -> String {
    format!("r12x{}", Uuid::now_v7().simple())
}

fn returned<'a>(reply: &'a Value, key: &str) -> Vec<&'a Value> {
    reply
        .as_array()
        .map(|items| items.iter().map(|item| &item[key]).collect())
        .unwrap_or_default()
}

fn assert_only_the_literal_match(
    reply: &Value,
    key: &str,
    literal: &Value,
    wildcard: &Value,
    term: &str,
) {
    let found = returned(reply, key);
    assert!(
        found.contains(&literal),
        "{term:?}: the row that contains the term literally must be returned"
    );
    assert!(
        !found.contains(&wildcard),
        "{term:?}: a row matching only a wildcard reading of the term must not be returned"
    );
}

async fn post_search_is_literal(literal_title: &str, wildcard_title: &str, term: &str) {
    let h = live_harness("").await;
    let author = seed_user(&h.pool, "R12 test user").await;
    let literal = seed_post(&h.pool, author.id, "need", literal_title).await;
    let wildcard = seed_post(&h.pool, author.id, "need", wildcard_title).await;

    let uri = format!("/posts?limit=200&q={}", escape(term));
    let (got, reply) = call(&h.app, get(&uri, None)).await;

    assert_status(got, StatusCode::OK, &reply, "post search");
    assert_only_the_literal_match(&reply, "id", &json!(literal), &json!(wildcard), term);
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn post_search_matches_a_percent_sign_literally() {
    let token = unique_token();
    post_search_is_literal(
        &format!("{token}100% wool"),
        &format!("{token}100 wool"),
        &format!("{token}100%"),
    )
    .await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn post_search_matches_an_underscore_literally() {
    let token = unique_token();
    post_search_is_literal(
        &format!("{token}_a wool"),
        &format!("{token}xa wool"),
        &format!("{token}_a"),
    )
    .await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn user_search_matches_percent_and_underscore_literally() {
    let h = live_harness("").await;
    let token = unique_token();
    let percent = seed_user(&h.pool, &format!("{token}100% wool")).await;
    let no_percent = seed_user(&h.pool, &format!("{token}100 wool")).await;
    let underscore = seed_user(&h.pool, &format!("{token}_a wool")).await;
    let no_underscore = seed_user(&h.pool, &format!("{token}xa wool")).await;

    for (term, literal, wildcard) in [
        (format!("{token}100%"), &percent, &no_percent),
        (format!("{token}_a"), &underscore, &no_underscore),
    ] {
        let uri = format!("/search/users?q={}", escape(&term));
        let (got, reply) = call(&h.app, get(&uri, None)).await;

        assert_status(got, StatusCode::OK, &reply, "user search");
        let (literal, wildcard) = (json!(literal.id), json!(wildcard.id));
        assert_only_the_literal_match(&reply, "id", &literal, &wildcard, &term);
    }
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn directory_search_matches_percent_and_underscore_literally() {
    let h = live_harness(DIRECTORY_CONFIG).await;
    let token = unique_token();
    let percent = seed_directory_entry(&h.pool, &format!("{token}100% wool")).await;
    let no_percent = seed_directory_entry(&h.pool, &format!("{token}100 wool")).await;
    let underscore = seed_directory_entry(&h.pool, &format!("{token}_a wool")).await;
    let no_underscore = seed_directory_entry(&h.pool, &format!("{token}xa wool")).await;

    for (term, literal, wildcard) in [
        (format!("{token}100%"), &percent, &no_percent),
        (format!("{token}_a"), &underscore, &no_underscore),
    ] {
        let uri = format!("/directory?q={}", escape(&term));
        let (got, reply) = call(&h.app, get(&uri, None)).await;

        assert_status(got, StatusCode::OK, &reply, "directory search");
        let (literal, wildcard) = (json!(literal), json!(wildcard));
        assert_only_the_literal_match(&reply, "url", &literal, &wildcard, &term);
    }
}

// ---------------------------------------------------------------------------------------------
// An image is decoded only within fixed limits and only as the type it claims.
// ---------------------------------------------------------------------------------------------

struct Uploader {
    h: Harness,
    author: TestUser,
    post: Uuid,
}

async fn uploader(config_toml: &str) -> Uploader {
    let h = live_harness(config_toml).await;
    let author = seed_user(&h.pool, "R12 test user").await;
    let post = seed_post(&h.pool, author.id, "need", "R12 post with images").await;
    Uploader { h, author, post }
}

async fn upload_post_images(u: &Uploader, parts: &[Part<'_>]) -> (StatusCode, Value) {
    let uri = format!("/posts/{}/images", u.post);
    call(&u.h.app, multipart_request(&uri, &u.author.bearer, parts)).await
}

async fn upload_avatar(h: &Harness, user: &TestUser, parts: &[Part<'_>]) -> (StatusCode, Value) {
    let request = multipart_request("/auth/me/avatar", &user.bearer, parts);
    call(&h.app, request).await
}

async fn post_image_is_refused(data: &[u8], context: &str) {
    let u = uploader("").await;

    let (got, reply) = upload_post_images(&u, &[image(data)]).await;

    assert_status(got, StatusCode::BAD_REQUEST, &reply, context);
    assert_eq!(
        image_count(&u.h.pool, u.post).await,
        0,
        "{context}: a refused image must not be attached"
    );
}

async fn avatar_is_refused(data: &[u8], context: &str) {
    let h = live_harness("").await;
    let user = seed_user(&h.pool, "R12 test user").await;

    let (got, reply) = upload_avatar(&h, &user, &[image(data)]).await;

    assert_status(got, StatusCode::BAD_REQUEST, &reply, context);
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn post_image_wider_than_6000_px_is_refused() {
    post_image_is_refused(&png(6001, 1), "a 6001x1 PNG post image").await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn avatar_wider_than_6000_px_is_refused() {
    avatar_is_refused(&png(6001, 1), "a 6001x1 PNG avatar").await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn post_image_of_another_type_than_claimed_is_refused() {
    post_image_is_refused(GIF_1X1, "a GIF post image sent as image/png").await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn avatar_of_another_type_than_claimed_is_refused() {
    avatar_is_refused(GIF_1X1, "a GIF avatar sent as image/png").await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn post_image_of_800_by_600_is_accepted() {
    let u = uploader("").await;
    let picture = png(800, 600);

    let (got, reply) = upload_post_images(&u, &[image(&picture)]).await;

    assert_status(got, StatusCode::OK, &reply, "an 800x600 PNG post image");
    assert_eq!(image_count(&u.h.pool, u.post).await, 1);
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn avatar_of_800_by_600_is_accepted() {
    let h = live_harness("").await;
    let user = seed_user(&h.pool, "R12 test user").await;
    let picture = png(800, 600);

    let (got, reply) = upload_avatar(&h, &user, &[image(&picture)]).await;

    assert_status(got, StatusCode::OK, &reply, "an 800x600 PNG avatar");
}

// ---------------------------------------------------------------------------------------------
// An upload up to the configured size is accepted, and a skipped part is reported.
// ---------------------------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn post_image_up_to_the_configured_size_is_accepted() {
    let u = uploader("").await;
    let configured = Config::default().media.max_post_image_bytes as usize;
    let picture = incompressible_png(940);
    assert!(
        picture.len() >= 2_500_000 && picture.len() < configured,
        "the fixture must be about 2.5 MiB and under the configured {configured} bytes, got {}",
        picture.len()
    );

    let (got, reply) = upload_post_images(&u, &[image(&picture)]).await;

    assert_status(got, StatusCode::OK, &reply, "a PNG under the size cap");
    assert_eq!(reply["images"].as_array().map(Vec::len), Some(1));
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_skipped_part_is_reported_with_its_reason() {
    let u = uploader("").await;
    let picture = png(800, 600);
    let caption = Part {
        name: "caption",
        content_type: "text/plain",
        data: b"R12 synthetic caption",
    };

    let (got, reply) = upload_post_images(&u, &[caption, image(&picture)]).await;

    assert_status(got, StatusCode::OK, &reply, "a text part beside a PNG");
    assert_eq!(reply["images"].as_array().map(Vec::len), Some(1));
    let skipped = reply["skipped"].as_array().cloned().unwrap_or_default();
    assert_eq!(skipped.len(), 1, "the text part must be reported: {reply}");
    assert_eq!(skipped[0]["name"], json!("caption"));
    let reason = skipped[0]["reason"].as_str().unwrap_or("");
    assert!(!reason.is_empty(), "the reason is missing: {reply}");
}

// ---------------------------------------------------------------------------------------------
// A failed upload leaves no file behind, and concurrent uploads respect max_post_images.
// ---------------------------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_failed_upload_leaves_no_file_behind() {
    let u = uploader("").await;
    let picture = png(800, 600);
    let parts = [image(&picture), image(b"not an image")];

    let (got, reply) = upload_post_images(&u, &parts).await;

    let context = "an upload whose second part is not an image";
    assert_status(got, StatusCode::BAD_REQUEST, &reply, context);
    assert_eq!(
        files_in(&u.h.post_images()),
        0,
        "a failed upload must remove every file it wrote"
    );
    assert_eq!(image_count(&u.h.pool, u.post).await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn concurrent_uploads_never_exceed_max_post_images() {
    let u = uploader("").await;
    let max = Config::default().media.max_post_images as i32;
    let picture = png(64, 64);
    let uri = format!("/posts/{}/images", u.post);
    let requests = (0..RACERS)
        .map(|_| multipart_request(&uri, &u.author.bearer, &[image(&picture)]))
        .collect();

    let statuses = all_at_once(&u.h.app, requests).await;

    let attached = image_count(&u.h.pool, u.post).await;
    assert!(
        attached <= max,
        "{attached} images attached against a cap of {max}; statuses {statuses:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// The hourly caps hold under concurrent requests.
// ---------------------------------------------------------------------------------------------

fn assert_within_cap(statuses: &[StatusCode], stored: i64, cap: usize, what: &str) {
    let accepted = successes(statuses);
    assert!(
        accepted <= cap,
        "{accepted} {what} accepted against a cap of {cap}; statuses {statuses:?}"
    );
    assert!(
        stored <= cap as i64,
        "{stored} {what} stored against a cap of {cap}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn concurrent_post_creates_never_exceed_the_hourly_cap() {
    let h = live_harness(&low_hourly_caps()).await;
    let author = seed_user(&h.pool, "R12 test user").await;
    let requests = (0..RACERS)
        .map(|i| {
            let body = new_post(&format!("R12 race {i}"));
            json_request(Method::POST, "/posts", &author.bearer, &body)
        })
        .collect();

    let statuses = all_at_once(&h.app, requests).await;

    let stored = count(&h.pool, POSTS_BY_AUTHOR, author.id).await;
    assert_within_cap(&statuses, stored, HOURLY_CAP, "posts");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn concurrent_messages_never_exceed_the_hourly_cap() {
    let h = live_harness(&low_hourly_caps()).await;
    let author = seed_user(&h.pool, "R12 test user").await;
    let responder = seed_user(&h.pool, "R12 test user").await;
    let post = seed_post(&h.pool, author.id, "need", "R12 thread").await;
    let thread = seed_match(&h.pool, post, responder.id).await;
    let uri = format!("/conversations/{thread}/messages");
    let message = sealed_message();
    let requests = (0..RACERS)
        .map(|_| json_request(Method::POST, &uri, &responder.bearer, &message))
        .collect();

    let statuses = all_at_once(&h.app, requests).await;

    let stored = count(&h.pool, MESSAGES_BY_SENDER, responder.id).await;
    assert_within_cap(&statuses, stored, HOURLY_CAP, "messages");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn concurrent_responses_never_exceed_the_hourly_cap() {
    let h = live_harness(&low_hourly_caps()).await;
    let author = seed_user(&h.pool, "R12 test user").await;
    let responder = seed_user(&h.pool, "R12 test user").await;
    let message = sealed_message();
    let mut requests = Vec::with_capacity(RACERS);
    for i in 0..RACERS {
        let post = seed_post(&h.pool, author.id, "need", &format!("R12 post {i}")).await;
        let uri = format!("/posts/{post}/respond");
        let request = json_request(Method::POST, &uri, &responder.bearer, &message);
        requests.push(request);
    }

    let statuses = all_at_once(&h.app, requests).await;

    let stored = count(&h.pool, MATCHES_BY_RESPONDER, responder.id).await;
    assert_within_cap(&statuses, stored, HOURLY_CAP, "responses");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn concurrent_avatar_uploads_never_exceed_the_hourly_cap() {
    let h = live_harness("").await;
    let user = seed_user(&h.pool, "R12 test user").await;
    let picture = png(64, 64);
    let requests = (0..RACERS)
        .map(|_| multipart_request("/auth/me/avatar", &user.bearer, &[image(&picture)]))
        .collect();

    let statuses = all_at_once(&h.app, requests).await;

    let stored = count(&h.pool, AVATAR_UPLOADS, user.id).await;
    assert_within_cap(&statuses, stored, AVATARS_PER_HOUR, "avatar uploads");
}

// ---------------------------------------------------------------------------------------------
// Conversation lists come in pages, and a page size above the maximum is refused.
// ---------------------------------------------------------------------------------------------

struct Thread {
    h: Harness,
    author: TestUser,
    responder: TestUser,
    id: Uuid,
}

async fn thread_on(kind: &str) -> Thread {
    let h = live_harness("").await;
    let author = seed_user(&h.pool, "R12 test user").await;
    let responder = seed_user(&h.pool, "R12 test user").await;
    let post = seed_post(&h.pool, author.id, kind, "R12 thread").await;
    let id = seed_match(&h.pool, post, responder.id).await;
    Thread {
        h,
        author,
        responder,
        id,
    }
}

/// The message's ciphertext is its four-byte sequence number.
fn sequence_number(message: &Value) -> i32 {
    let encoded = message["ciphertext"].as_str().expect("a ciphertext");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .expect("base64 ciphertext");
    let bytes: [u8; 4] = bytes.try_into().expect("a four-byte sequence number");
    i32::from_be_bytes(bytes)
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_conversation_returns_its_newest_100_messages_oldest_first() {
    let t = thread_on("need").await;
    // A higher sequence number is a newer message.
    sqlx::query(
        "INSERT INTO messages (id, match_id, sender_id, ciphertext, created_at)
         SELECT gen_random_uuid(), $1, $2, int4send(g), now() - make_interval(secs => (150 - g)::float8)
         FROM generate_series(1, 150) AS g",
    )
    .bind(t.id)
    .bind(t.responder.id)
    .execute(&t.h.pool)
    .await
    .expect("insert 150 test messages");

    let uri = format!("/conversations/{}", t.id);
    let (got, reply) = call(&t.h.app, get(&uri, Some(&t.author.bearer))).await;

    assert_status(got, StatusCode::OK, &reply, "a thread of 150 messages");
    let sequence: Vec<i32> = reply["messages"]
        .as_array()
        .map(|messages| messages.iter().map(sequence_number).collect())
        .unwrap_or_default();
    let newest: Vec<i32> = (51..=150).collect();
    assert_eq!(sequence, newest, "the newest 100 messages, oldest first");
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn the_conversation_list_returns_50_by_default() {
    let h = live_harness("").await;
    let author = seed_user(&h.pool, "R12 test user").await;
    let responder = seed_user(&h.pool, "R12 test user").await;
    sqlx::query(
        "INSERT INTO posts (id, author_id, kind, category, title, status, visibility)
         SELECT gen_random_uuid(), $1, 'need', 'food', 'R12 thread ' || g, 'active', 'public'
         FROM generate_series(1, 60) AS g",
    )
    .bind(author.id)
    .execute(&h.pool)
    .await
    .expect("insert 60 test posts");
    sqlx::query(
        "INSERT INTO matches (id, post_id, responder_id, status)
         SELECT gen_random_uuid(), id, $2, 'proposed' FROM posts WHERE author_id = $1",
    )
    .bind(author.id)
    .bind(responder.id)
    .execute(&h.pool)
    .await
    .expect("insert 60 test matches");

    let request = get("/me/conversations", Some(&responder.bearer));
    let (got, reply) = call(&h.app, request).await;

    assert_status(got, StatusCode::OK, &reply, "a list of 60 conversations");
    assert_eq!(reply.as_array().map(Vec::len), Some(50));
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn the_offer_list_returns_200_by_default() {
    let t = thread_on("listing").await;
    sqlx::query(
        "INSERT INTO match_offers (id, match_id, actor_id, kind, amount_cents, currency)
         SELECT gen_random_uuid(), $1, $2, 'offer', 2500, 'EUR' FROM generate_series(1, 250)",
    )
    .bind(t.id)
    .bind(t.responder.id)
    .execute(&t.h.pool)
    .await
    .expect("insert 250 test offers");

    let uri = format!("/conversations/{}/offers", t.id);
    let (got, reply) = call(&t.h.app, get(&uri, Some(&t.author.bearer))).await;

    assert_status(got, StatusCode::OK, &reply, "a thread of 250 offers");
    assert_eq!(reply.as_array().map(Vec::len), Some(200));
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_page_size_above_the_maximum_is_refused() {
    let t = thread_on("listing").await;

    for uri in [
        "/me/conversations?limit=201".to_string(),
        format!("/conversations/{}?limit=501", t.id),
        format!("/conversations/{}/offers?limit=501", t.id),
    ] {
        let (got, reply) = call(&t.h.app, get(&uri, Some(&t.responder.bearer))).await;
        assert_status(got, StatusCode::BAD_REQUEST, &reply, &uri);
    }
}

// ---------------------------------------------------------------------------------------------
// A stalled request body is answered 408, and an oversized JSON body 413.
// ---------------------------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_stalled_request_body_is_answered_408() {
    let h = live_harness("").await;
    let author = seed_user(&h.pool, "R12 test user").await;

    let mut sent = false;
    let mut paused = false;
    let stalled = futures_util::stream::poll_fn(move |_| {
        if !sent {
            sent = true;
            let chunk = Bytes::from_static(STALLED_PREFIX);
            return Poll::Ready(Some(Ok::<_, std::io::Error>(chunk)));
        }
        // The clock stops only once the handler is reading the body, after the session lookup:
        // virtual time that jumped over a database wait would let any timeout pass this test.
        if !paused {
            paused = true;
            tokio::time::pause();
        }
        Poll::Pending
    });
    let request = builder(Method::POST, "/posts", Some(&author.bearer))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from_stream(stalled))
        .expect("build request");

    let started = tokio::time::Instant::now();
    let pending = h.app.clone().oneshot(request);
    let answer = tokio::time::timeout(Duration::from_secs(60), pending).await;

    let Ok(response) = answer else {
        panic!("a stalled request body must be answered; nothing came within 60 s");
    };
    let response = response.expect("the router is infallible");
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    assert!(
        started.elapsed() <= Duration::from_secs(31),
        "answered after {:?}",
        started.elapsed()
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_json_body_over_256_kib_is_refused_with_413() {
    let h = live_harness("").await;
    let author = seed_user(&h.pool, "R12 test user").await;
    let mut body = new_post("R12 oversized post");
    body["body"] = json!("a".repeat(300 * 1024));

    let (got, reply) = create(&h, &author, &body).await;

    let context = "a 300 KiB JSON body";
    assert_status(got, StatusCode::PAYLOAD_TOO_LARGE, &reply, context);
    assert_eq!(
        count(&h.pool, POSTS_BY_AUTHOR, author.id).await,
        0,
        "a refused body must store no post"
    );
}
