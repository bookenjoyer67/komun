//! The deal and moderation guards on posts: an accept carries the terms on the table, an author
//! cannot update a post under moderation or reopen a sold listing, a deal completes only on an open
//! post, a single-post read shows only what the feed would show, a response opens only on a public,
//! active, unsold post, the buyer is shown only to the two parties, and a completed want records
//! its author as the buyer.
//!
//! Every test needs a live Postgres, so every test is `#[ignore]`d; run against a disposable
//! database with `KOMUN_TEST_DATABASE_URL=postgres://... cargo test -p komun-server
//! deal_and_moderation -- --ignored --test-threads=1`. Each test builds its own harness, users and
//! posts, so they run in any order. All data is synthetic.

use std::net::SocketAddr;
use std::sync::Arc;

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
use tower::ServiceExt;
use uuid::Uuid;

use crate::config::Config;
use crate::{api, auth, db, rate_limit, sessions, AppState};

const DATABASE_ENV: &str = "KOMUN_TEST_DATABASE_URL";

/// A synthetic verifier in the shape the client sends; it is not real material.
const VERIFIER: &str = "c3ludGhldGljLXRlc3QtdmVyaWZpZXItZm9yLXI2LWRlYWxz";

const OFFERED_CENTS: i64 = 2500;
const OFFERED_CURRENCY: &str = "EUR";
const OTHER_CURRENCY: &str = "USD";
const CONTACT_METHOD: &str = "reply in the thread";

const MATCHES_ON_POST: &str = "SELECT COUNT(*) FROM matches WHERE post_id = $1";
const ACCEPT_ROWS: &str =
    "SELECT COUNT(*) FROM match_offers WHERE match_id = $1 AND kind = 'accept'";
const RESPONSE_NOTIFICATIONS: &str =
    "SELECT COUNT(*) FROM notifications WHERE user_id = $1 AND kind = 'response'";

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
    let peer = SocketAddr::from(([203, 0, 113, 6], 40_000));
    let app = api::router(state).layer(MockConnectInfo(peer));
    Harness { pool, app }
}

struct TestUser {
    id: Uuid,
    bearer: String,
}

async fn seed_user(pool: &PgPool, role: &str) -> TestUser {
    let id = Uuid::now_v7();
    let email = format!("r6-deal-{id}@test.invalid");
    let password_hash = auth::password::hash_verifier(VERIFIER).expect("hash");
    sqlx::query(
        "INSERT INTO users (id, email, email_verified_at, password_hash, auth_salt, display_name, role)
         VALUES ($1, $2, now(), $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(email)
    .bind(password_hash)
    .bind(vec![0x5a_u8; 16])
    .bind("R6 test user")
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

#[derive(Clone, Copy)]
struct PostSeed {
    kind: &'static str,
    status: &'static str,
    visibility: &'static str,
}

impl PostSeed {
    fn public(kind: &'static str, status: &'static str) -> Self {
        PostSeed {
            kind,
            status,
            visibility: "public",
        }
    }

    fn private(kind: &'static str, status: &'static str) -> Self {
        PostSeed {
            kind,
            status,
            visibility: "private",
        }
    }
}

struct SeededPost {
    id: Uuid,
    title: String,
}

/// Seeded straight into `posts`, so a test starts from the state under test without a route that
/// may itself be guarded.
async fn seed_post(pool: &PgPool, author: Uuid, seed: PostSeed) -> SeededPost {
    let id = Uuid::now_v7();
    let title = format!("r6-{}-{id}", seed.kind);
    // `chk_posts_market_fields` allows price and condition on market kinds only.
    let market = matches!(seed.kind, "listing" | "want");
    let category = if market { "electronics" } else { "food" };
    sqlx::query(
        "INSERT INTO posts (id, author_id, kind, category, title, body, status, visibility,
                            contact_method, market_listed, price_cents, currency, item_condition)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    )
    .bind(id)
    .bind(author)
    .bind(seed.kind)
    .bind(category)
    .bind(&title)
    .bind("R6 synthetic post body")
    .bind(seed.status)
    .bind(seed.visibility)
    .bind(CONTACT_METHOD)
    .bind(market)
    .bind(market.then_some(OFFERED_CENTS))
    .bind(market.then_some(OFFERED_CURRENCY))
    .bind(market.then_some("good"))
    .execute(pool)
    .await
    .expect("insert test post");
    SeededPost { id, title }
}

async fn mark_sold(pool: &PgPool, post: Uuid, buyer: Uuid) {
    sqlx::query("UPDATE posts SET sold_at = now(), buyer_id = $2 WHERE id = $1")
        .bind(post)
        .bind(buyer)
        .execute(pool)
        .await
        .expect("mark the test post sold");
}

/// An `accepted` match carries the agreed terms, as an accept through the offers route leaves it.
async fn seed_match(pool: &PgPool, post: Uuid, responder: Uuid, status: &str) -> Uuid {
    let id = Uuid::now_v7();
    let accepted = status == "accepted";
    sqlx::query(
        "INSERT INTO matches (id, post_id, responder_id, status, agreed_price_cents, currency)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(post)
    .bind(responder)
    .bind(status)
    .bind(accepted.then_some(OFFERED_CENTS))
    .bind(accepted.then_some(OFFERED_CURRENCY))
    .execute(pool)
    .await
    .expect("insert test match");
    id
}

async fn seed_offer(pool: &PgPool, match_id: Uuid, actor: Uuid) {
    sqlx::query(
        "INSERT INTO match_offers (id, match_id, actor_id, kind, amount_cents, currency)
         VALUES ($1, $2, $3, 'offer', $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(match_id)
    .bind(actor)
    .bind(OFFERED_CENTS)
    .bind(OFFERED_CURRENCY)
    .execute(pool)
    .await
    .expect("insert test offer");
}

/// Every column of the row: the only way to see that a refusal moved nothing.
async fn post_row(pool: &PgPool, id: Uuid) -> Value {
    sqlx::query_scalar::<_, Value>("SELECT to_jsonb(p) FROM posts p WHERE p.id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("read full post row")
}

async fn match_row(pool: &PgPool, id: Uuid) -> Value {
    sqlx::query_scalar::<_, Value>("SELECT to_jsonb(m) FROM matches m WHERE m.id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("read full match row")
}

#[derive(sqlx::FromRow)]
struct PostState {
    status: String,
    title: String,
    sold: bool,
    buyer_id: Option<Uuid>,
}

async fn post_state(pool: &PgPool, id: Uuid) -> PostState {
    sqlx::query_as::<_, PostState>(
        "SELECT status, title, sold_at IS NOT NULL AS sold, buyer_id FROM posts WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("read post state")
}

async fn match_terms(pool: &PgPool, id: Uuid) -> (String, Option<i64>, Option<String>) {
    sqlx::query_as::<_, (String, Option<i64>, Option<String>)>(
        "SELECT status, agreed_price_cents, currency FROM matches WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("read match terms")
}

async fn count(pool: &PgPool, sql: &str, id: Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count rows")
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

fn assert_status(got: StatusCode, want: StatusCode, body: &Value, context: &str) {
    assert_eq!(got, want, "{context}: error {:?}", error_of(body));
}

fn buyer_of(body: &Value) -> Option<&str> {
    body.get("buyer_id").and_then(Value::as_str)
}

async fn read_post(h: &Harness, bearer: Option<&str>, id: Uuid) -> (StatusCode, Value) {
    send(&h.app, Method::GET, &format!("/posts/{id}"), bearer, None).await
}

async fn patch_post(h: &Harness, user: &TestUser, id: Uuid, body: &Value) -> (StatusCode, Value) {
    let path = format!("/posts/{id}");
    send(&h.app, Method::PATCH, &path, Some(&user.bearer), Some(body)).await
}

/// A synthetic sealed box: the server stores the bytes and never reads them.
fn sealed_opening() -> Value {
    let engine = base64::engine::general_purpose::STANDARD;
    json!({
        "ciphertext": engine.encode([0x42_u8; 48]),
        "nonce": engine.encode([0x24_u8; 24]),
    })
}

async fn respond(h: &Harness, user: &TestUser, post: Uuid) -> (StatusCode, Value) {
    let path = format!("/posts/{post}/respond");
    let body = sealed_opening();
    send(&h.app, Method::POST, &path, Some(&user.bearer), Some(&body)).await
}

// ---------------------------------------------------------------------------------------------
// An accept carries the amount and currency of the offer it accepts.
// ---------------------------------------------------------------------------------------------

struct OpenOffer {
    h: Harness,
    author: TestUser,
    match_id: Uuid,
}

async fn open_offer() -> OpenOffer {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user").await;
    let responder = seed_user(&h.pool, "user").await;
    let post = seed_post(&h.pool, author.id, PostSeed::public("listing", "active")).await;
    let match_id = seed_match(&h.pool, post.id, responder.id, "proposed").await;
    seed_offer(&h.pool, match_id, responder.id).await;
    OpenOffer {
        h,
        author,
        match_id,
    }
}

async fn accept(offer: &OpenOffer, amount: i64, currency: &str) -> (StatusCode, Value) {
    let path = format!("/conversations/{}/offers", offer.match_id);
    let body = json!({ "kind": "accept", "amount_cents": amount, "currency": currency });
    let bearer = Some(offer.author.bearer.as_str());
    send(&offer.h.app, Method::POST, &path, bearer, Some(&body)).await
}

async fn accept_off_the_offered_terms_is_refused(amount: i64, currency: &str) {
    let offer = open_offer().await;
    let before = match_row(&offer.h.pool, offer.match_id).await;

    let (got, body) = accept(&offer, amount, currency).await;

    assert_status(
        got,
        StatusCode::CONFLICT,
        &body,
        "accept off the offered terms",
    );
    assert_eq!(
        match_row(&offer.h.pool, offer.match_id).await,
        before,
        "a refused accept must leave the match row unchanged"
    );
    assert_eq!(
        count(&offer.h.pool, ACCEPT_ROWS, offer.match_id).await,
        0,
        "a refused accept must append no accept row"
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb02_accept_must_carry_the_offered_amount() {
    accept_off_the_offered_terms_is_refused(OFFERED_CENTS - 1000, OFFERED_CURRENCY).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb02_accept_must_carry_the_offered_currency() {
    accept_off_the_offered_terms_is_refused(OFFERED_CENTS, OTHER_CURRENCY).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb02_accept_at_the_offered_terms_is_unchanged() {
    let offer = open_offer().await;

    let (got, body) = accept(&offer, OFFERED_CENTS, OFFERED_CURRENCY).await;

    assert_status(
        got,
        StatusCode::CREATED,
        &body,
        "accept at the offered terms",
    );
    let (status, agreed, currency) = match_terms(&offer.h.pool, offer.match_id).await;
    assert_eq!(status, "accepted");
    assert_eq!(agreed, Some(OFFERED_CENTS));
    assert_eq!(currency.as_deref(), Some(OFFERED_CURRENCY));
    assert_eq!(count(&offer.h.pool, ACCEPT_ROWS, offer.match_id).await, 1);
}

// ---------------------------------------------------------------------------------------------
// An author cannot update a post under moderation or make a sold listing active again.
// ---------------------------------------------------------------------------------------------

async fn author_update_is_refused(seed: PostSeed, update: Value, want: StatusCode) {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user").await;
    let post = seed_post(&h.pool, author.id, seed).await;
    let before = post_row(&h.pool, post.id).await;

    let (got, body) = patch_post(&h, &author, post.id, &update).await;

    let context = format!("author update on a {} post", seed.status);
    assert_status(got, want, &body, &context);
    assert_eq!(
        post_row(&h.pool, post.id).await,
        before,
        "a refused author update must leave the post unchanged"
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb03_author_cannot_change_status_of_a_moderated_post() {
    let update = json!({ "status": "active" });
    author_update_is_refused(
        PostSeed::public("need", "hidden"),
        update,
        StatusCode::FORBIDDEN,
    )
    .await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb03_author_cannot_edit_text_of_a_moderated_post() {
    let update = json!({ "title": "R6 edited title" });
    author_update_is_refused(
        PostSeed::public("need", "flagged"),
        update,
        StatusCode::FORBIDDEN,
    )
    .await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb03_sold_listing_cannot_return_to_active() {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user").await;
    let buyer = seed_user(&h.pool, "user").await;
    let post = seed_post(&h.pool, author.id, PostSeed::public("listing", "fulfilled")).await;
    mark_sold(&h.pool, post.id, buyer.id).await;
    let before = post_row(&h.pool, post.id).await;

    let update = json!({ "status": "active" });
    let (got, body) = patch_post(&h, &author, post.id, &update).await;

    assert_status(
        got,
        StatusCode::CONFLICT,
        &body,
        "a sold listing made active",
    );
    assert_eq!(
        post_row(&h.pool, post.id).await,
        before,
        "a refused reopen must leave the sold listing unchanged"
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb03_author_edits_and_fulfils_an_active_post_as_before() {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user").await;
    let post = seed_post(&h.pool, author.id, PostSeed::public("need", "active")).await;

    let update = json!({ "title": "R6 edited title", "body": "R6 edited body" });
    let (got, body) = patch_post(&h, &author, post.id, &update).await;
    assert_status(got, StatusCode::OK, &body, "text edit on an active post");
    assert_eq!(post_state(&h.pool, post.id).await.title, "R6 edited title");

    let update = json!({ "status": "fulfilled" });
    let (got, body) = patch_post(&h, &author, post.id, &update).await;
    assert_status(got, StatusCode::OK, &body, "fulfilling an active post");
    assert_eq!(post_state(&h.pool, post.id).await.status, "fulfilled");
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb03_author_still_edits_text_on_a_sold_listing() {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user").await;
    let buyer = seed_user(&h.pool, "user").await;
    let post = seed_post(&h.pool, author.id, PostSeed::public("listing", "fulfilled")).await;
    mark_sold(&h.pool, post.id, buyer.id).await;

    let update = json!({ "title": "R6 edited title" });
    let (got, body) = patch_post(&h, &author, post.id, &update).await;

    assert_status(got, StatusCode::OK, &body, "text edit on a sold listing");
    let state = post_state(&h.pool, post.id).await;
    assert_eq!(state.title, "R6 edited title");
    assert_eq!(state.status, "fulfilled");
    assert!(state.sold, "a text edit must leave the listing sold");
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb03_author_moves_an_unsold_post_among_open_statuses_as_before() {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user").await;
    let post = seed_post(&h.pool, author.id, PostSeed::public("listing", "active")).await;

    for status in [
        "matched",
        "expired",
        "active",
        "withdrawn",
        "active",
        "fulfilled",
        "active",
    ] {
        let update = json!({ "status": status });
        let (got, body) = patch_post(&h, &author, post.id, &update).await;
        assert_status(
            got,
            StatusCode::OK,
            &body,
            &format!("author move to {status}"),
        );
        assert_eq!(post_state(&h.pool, post.id).await.status, status);
    }
}

// ---------------------------------------------------------------------------------------------
// A deal completes only while its post is active or matched.
// ---------------------------------------------------------------------------------------------

struct Deal {
    author: TestUser,
    responder: Uuid,
    post: Uuid,
    match_id: Uuid,
}

async fn accepted_deal(pool: &PgPool, seed: PostSeed) -> Deal {
    let author = seed_user(pool, "user").await;
    let responder = seed_user(pool, "user").await;
    let post = seed_post(pool, author.id, seed).await;
    let match_id = seed_match(pool, post.id, responder.id, "accepted").await;
    Deal {
        author,
        responder: responder.id,
        post: post.id,
        match_id,
    }
}

/// Completed by the author, so these tests do not turn on which participant may complete.
async fn complete(h: &Harness, deal: &Deal) -> (StatusCode, Value) {
    let path = format!("/conversations/{}/status", deal.match_id);
    let body = json!({ "status": "completed" });
    let bearer = Some(deal.author.bearer.as_str());
    send(&h.app, Method::PATCH, &path, bearer, Some(&body)).await
}

async fn completion_is_refused_on(status: &'static str) {
    let h = live_harness().await;
    let deal = accepted_deal(&h.pool, PostSeed::public("listing", status)).await;
    let post_before = post_row(&h.pool, deal.post).await;
    let match_before = match_row(&h.pool, deal.match_id).await;

    let (got, body) = complete(&h, &deal).await;

    let context = format!("completion on a {status} post");
    assert_status(got, StatusCode::CONFLICT, &body, &context);
    assert_eq!(
        post_row(&h.pool, deal.post).await,
        post_before,
        "a refused completion must leave the post unchanged"
    );
    assert_eq!(
        match_row(&h.pool, deal.match_id).await,
        match_before,
        "a refused completion must leave the deal accepted"
    );
}

async fn complete_open_deal(kind: &'static str, status: &'static str) -> (Deal, PostState) {
    let h = live_harness().await;
    let deal = accepted_deal(&h.pool, PostSeed::public(kind, status)).await;

    let (got, body) = complete(&h, &deal).await;

    let context = format!("completion on a {status} {kind}");
    assert_status(got, StatusCode::OK, &body, &context);
    let (match_status, _, _) = match_terms(&h.pool, deal.match_id).await;
    assert_eq!(match_status, "completed");
    let post = post_state(&h.pool, deal.post).await;
    assert_eq!(post.status, "fulfilled");
    assert!(post.sold, "a completed {kind} is marked sold");
    (deal, post)
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb04_completion_is_refused_on_an_expired_post() {
    completion_is_refused_on("expired").await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb04_completion_is_refused_on_a_fulfilled_post() {
    completion_is_refused_on("fulfilled").await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb04_completion_is_refused_on_a_withdrawn_post() {
    completion_is_refused_on("withdrawn").await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb04_completion_is_refused_on_a_hidden_post() {
    completion_is_refused_on("hidden").await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb04_completion_is_refused_on_a_flagged_post() {
    completion_is_refused_on("flagged").await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb04_completion_on_an_active_listing_is_unchanged() {
    let (deal, post) = complete_open_deal("listing", "active").await;
    assert_eq!(post.buyer_id, Some(deal.responder));
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb04_completion_on_a_matched_listing_is_unchanged() {
    let (deal, post) = complete_open_deal("listing", "matched").await;
    assert_eq!(post.buyer_id, Some(deal.responder));
}

// ---------------------------------------------------------------------------------------------
// A single-post read shows what the feed shows, plus the author's and an admin's view.
// ---------------------------------------------------------------------------------------------

async fn post_is_not_found(seed: PostSeed, as_other_user: bool) {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user").await;
    let other = seed_user(&h.pool, "user").await;
    let post = seed_post(&h.pool, author.id, seed).await;
    let bearer = as_other_user.then_some(other.bearer.as_str());

    let (got, body) = read_post(&h, bearer, post.id).await;

    let context = format!("read of a {} {} post", seed.visibility, seed.status);
    assert_status(got, StatusCode::NOT_FOUND, &body, &context);
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb07_private_post_is_not_found_anonymously() {
    post_is_not_found(PostSeed::private("need", "active"), false).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb07_withdrawn_post_is_not_found_anonymously() {
    post_is_not_found(PostSeed::public("need", "withdrawn"), false).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb07_flagged_post_is_not_found_anonymously() {
    post_is_not_found(PostSeed::public("need", "flagged"), false).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb07_moderated_post_is_not_found_for_another_user() {
    post_is_not_found(PostSeed::public("need", "hidden"), true).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb07_author_still_reads_own_private_post() {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user").await;
    let post = seed_post(&h.pool, author.id, PostSeed::private("need", "active")).await;

    let (got, body) = read_post(&h, Some(&author.bearer), post.id).await;

    assert_status(got, StatusCode::OK, &body, "author read of a private post");
    assert_eq!(body["id"], json!(post.id));
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb07_admin_still_reads_a_moderated_post() {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user").await;
    let admin = seed_user(&h.pool, "admin").await;
    let post = seed_post(&h.pool, author.id, PostSeed::public("need", "hidden")).await;

    let (got, body) = read_post(&h, Some(&admin.bearer), post.id).await;

    assert_status(got, StatusCode::OK, &body, "admin read of a hidden post");
    assert_eq!(body["id"], json!(post.id));
}

/// `verified_by` and `contact_method` stay in the public projection.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb07_public_active_post_is_unchanged() {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user").await;
    let verifier = seed_user(&h.pool, "user").await;
    let post = seed_post(&h.pool, author.id, PostSeed::public("need", "active")).await;
    sqlx::query("UPDATE posts SET verified_by = $2, verified_at = now() WHERE id = $1")
        .bind(post.id)
        .bind(verifier.id)
        .execute(&h.pool)
        .await
        .expect("mark the test post verified");

    let (got, body) = read_post(&h, None, post.id).await;

    assert_status(
        got,
        StatusCode::OK,
        &body,
        "anonymous read of a public active post",
    );
    assert_eq!(body["id"], json!(post.id));
    assert_eq!(body["title"], json!(post.title));
    assert_eq!(body["status"], json!("active"));
    assert_eq!(body["visibility"], json!("public"));
    assert_eq!(body["contact_method"], json!(CONTACT_METHOD));
    assert_eq!(body["verified_by"], json!(verifier.id));
}

// ---------------------------------------------------------------------------------------------
// A response opens a match only on a public, active, unsold post.
// ---------------------------------------------------------------------------------------------

async fn response_is_refused(seed: PostSeed, sold: bool, want: StatusCode) {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user").await;
    let responder = seed_user(&h.pool, "user").await;
    let post = seed_post(&h.pool, author.id, seed).await;
    if sold {
        let buyer = seed_user(&h.pool, "user").await;
        mark_sold(&h.pool, post.id, buyer.id).await;
    }
    let before = post_row(&h.pool, post.id).await;

    let (got, body) = respond(&h, &responder, post.id).await;

    let context = format!(
        "response to a {} {} {} (sold: {sold})",
        seed.visibility, seed.status, seed.kind
    );
    assert_status(got, want, &body, &context);
    assert_eq!(
        count(&h.pool, MATCHES_ON_POST, post.id).await,
        0,
        "a refused response must open no match"
    );
    assert_eq!(
        count(&h.pool, RESPONSE_NOTIFICATIONS, author.id).await,
        0,
        "a refused response must notify no one"
    );
    assert_eq!(
        post_row(&h.pool, post.id).await,
        before,
        "a refused response must leave the post unchanged"
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb08_response_to_a_private_post_is_not_found() {
    let seed = PostSeed::private("need", "active");
    response_is_refused(seed, false, StatusCode::NOT_FOUND).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb08_response_to_a_withdrawn_post_is_not_found() {
    let seed = PostSeed::public("need", "withdrawn");
    response_is_refused(seed, false, StatusCode::NOT_FOUND).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb08_response_to_a_hidden_post_is_not_found() {
    let seed = PostSeed::public("need", "hidden");
    response_is_refused(seed, false, StatusCode::NOT_FOUND).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb08_response_to_a_flagged_post_is_not_found() {
    let seed = PostSeed::public("need", "flagged");
    response_is_refused(seed, false, StatusCode::NOT_FOUND).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb08_response_to_a_matched_post_is_refused() {
    let seed = PostSeed::public("need", "matched");
    response_is_refused(seed, false, StatusCode::CONFLICT).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb08_response_to_a_fulfilled_post_is_refused() {
    let seed = PostSeed::public("need", "fulfilled");
    response_is_refused(seed, false, StatusCode::CONFLICT).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb08_response_to_an_expired_post_is_refused() {
    let seed = PostSeed::public("need", "expired");
    response_is_refused(seed, false, StatusCode::CONFLICT).await;
}

/// Active and sold at once, so the refusal turns on `sold_at` alone.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb08_response_to_a_sold_listing_is_refused() {
    let seed = PostSeed::public("listing", "active");
    response_is_refused(seed, true, StatusCode::CONFLICT).await;
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb08_response_to_a_missing_post_is_not_found() {
    let h = live_harness().await;
    let responder = seed_user(&h.pool, "user").await;
    let missing = Uuid::now_v7();

    let (got, body) = respond(&h, &responder, missing).await;

    assert_status(
        got,
        StatusCode::NOT_FOUND,
        &body,
        "response to a missing post",
    );
    assert_eq!(count(&h.pool, MATCHES_ON_POST, missing).await, 0);
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb08_response_to_a_public_active_post_is_unchanged() {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user").await;
    let responder = seed_user(&h.pool, "user").await;
    let post = seed_post(&h.pool, author.id, PostSeed::public("need", "active")).await;

    let (got, body) = respond(&h, &responder, post.id).await;

    assert_status(
        got,
        StatusCode::OK,
        &body,
        "response to a public active post",
    );
    assert_eq!(body["status"], json!("proposed"));
    assert_eq!(count(&h.pool, MATCHES_ON_POST, post.id).await, 1);
    assert_eq!(count(&h.pool, RESPONSE_NOTIFICATIONS, author.id).await, 1);
}

// ---------------------------------------------------------------------------------------------
// The buyer of a sold listing is shown to its author and its buyer only.
// ---------------------------------------------------------------------------------------------

struct SoldListing {
    h: Harness,
    author: TestUser,
    buyer: TestUser,
    post: SeededPost,
}

async fn sold_listing() -> SoldListing {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user").await;
    let buyer = seed_user(&h.pool, "user").await;
    let post = seed_post(&h.pool, author.id, PostSeed::public("listing", "fulfilled")).await;
    mark_sold(&h.pool, post.id, buyer.id).await;
    SoldListing {
        h,
        author,
        buyer,
        post,
    }
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb10_buyer_is_omitted_from_the_public_list() {
    let sold = sold_listing().await;
    let path = format!("/posts?q={}", sold.post.title);

    let (got, body) = send(&sold.h.app, Method::GET, &path, None, None).await;

    assert_status(got, StatusCode::OK, &body, "public list");
    let id = json!(sold.post.id);
    let item = body
        .as_array()
        .and_then(|items| items.iter().find(|item| item["id"] == id))
        .expect("the sold listing is in the public list");
    assert_eq!(
        buyer_of(item),
        None,
        "the public list must not name the buyer"
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb10_buyer_is_omitted_from_a_stranger_single_read() {
    let sold = sold_listing().await;
    let stranger = seed_user(&sold.h.pool, "user").await;

    for (who, bearer) in [
        ("an anonymous reader", None),
        ("another user", Some(stranger.bearer.as_str())),
    ] {
        let (got, body) = read_post(&sold.h, bearer, sold.post.id).await;
        assert_status(got, StatusCode::OK, &body, &format!("read by {who}"));
        assert_eq!(buyer_of(&body), None, "{who} must not see the buyer");
    }
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb10_author_and_buyer_still_see_the_buyer() {
    let sold = sold_listing().await;
    let buyer_id = sold.buyer.id.to_string();

    for (who, bearer) in [
        ("the author", sold.author.bearer.as_str()),
        ("the buyer", sold.buyer.bearer.as_str()),
    ] {
        let (got, body) = read_post(&sold.h, Some(bearer), sold.post.id).await;
        assert_status(got, StatusCode::OK, &body, &format!("read by {who}"));
        assert_eq!(
            buyer_of(&body),
            Some(buyer_id.as_str()),
            "{who} must see the buyer"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// On a want the author is the one buying, so a completed want records the author.
// ---------------------------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb12_completed_want_records_its_author_as_buyer() {
    let (deal, post) = complete_open_deal("want", "active").await;
    assert_eq!(
        post.buyer_id,
        Some(deal.author.id),
        "a completed want must record its author as the buyer"
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb12_completed_listing_records_the_responder_as_buyer() {
    let (deal, post) = complete_open_deal("listing", "active").await;
    assert_eq!(
        post.buyer_id,
        Some(deal.responder),
        "a completed listing must record the responder as the buyer"
    );
}
