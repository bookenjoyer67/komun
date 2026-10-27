//! Saved searches: a signed-in user saves, lists and deletes their own searches, a saved centre is
//! held on the same coarse grid as a post's, saving the same search twice is a 409, never a
//! second row (`UNIQUE NULLS NOT DISTINCT` over the user, the filters, the centre and the radius),
//! and the digest notifies once per search per window.
//!
//! Every test needs a live Postgres with `005_saved_searches.sql` applied, so every test is
//! `#[ignore]`d; run against a disposable database with `KOMUN_TEST_DATABASE_URL=postgres://...
//! cargo test -p komun-server tests::saved_searches -- --ignored --test-threads=1`. Each test seeds
//! its own users, so they run in any order. All data is synthetic.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::connect_info::MockConnectInfo,
    http::{header, Method, Request, StatusCode},
    Router,
};
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use crate::config::Config;
use crate::db::saved_searches::{Created, NewSavedSearch};
use crate::tasks::saved_search_digest;
use crate::{api, auth, db, rate_limit, sessions, AppState};

const DATABASE_ENV: &str = "KOMUN_TEST_DATABASE_URL";

/// A synthetic verifier in the shape the client sends; it is not real material.
const VERIFIER: &str = "c3ludGhldGljLXRlc3QtdmVyaWZpZXItZm9yLXdhdmU3LXNlYXJjaA";

const SAVED: &str = "/me/saved-searches";

// An exact point; what the server keeps must be its 0.1-degree cell.
const HOME_LAT: f64 = 37.80443;
const HOME_LON: f64 = -122.27121;
const HOME_CELL: (f64, f64) = (37.8, -122.3);

struct LiveHarness {
    pool: PgPool,
    state: AppState,
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

    let config: Config = toml::from_str("").expect("parse test config");
    let state = AppState {
        pool: pool.clone(),
        config: Arc::new(config),
        rate_limiter: Arc::new(rate_limit::RateLimiter::new()),
        mailer: Arc::new(None),
        trusted_proxies: Arc::new(Vec::new()),
        salt_pepper: Arc::new(sessions::generate_pepper()),
    };
    let peer = SocketAddr::from(([203, 0, 113, 17], 40_000));
    let app = api::router(state.clone()).layer(MockConnectInfo(peer));
    LiveHarness { pool, state, app }
}

struct TestUser {
    id: Uuid,
    bearer: String,
}

/// Inserts a verified user, then opens a session; the email is unique per call.
async fn seed_user(pool: &PgPool) -> TestUser {
    let id = Uuid::now_v7();
    let email = format!("wave7-saved-{id}@test.invalid");
    let password_hash = auth::password::hash_verifier(VERIFIER).expect("hash");
    sqlx::query(
        "INSERT INTO users (id, email, email_verified_at, password_hash, auth_salt, display_name, role)
         VALUES ($1, $2, now(), $3, $4, $5, 'user')",
    )
    .bind(id)
    .bind(email)
    .bind(password_hash)
    .bind(vec![0x5a_u8; 16])
    .bind("Wave 7 test user")
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

fn id_of(body: &Value) -> Uuid {
    body.get("id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(|| panic!("a saved search answers with its id, got {body}"))
}

fn wool_near_home() -> Value {
    json!({ "q": "wool", "near_lat": HOME_LAT, "near_lon": HOME_LON, "radius_km": 25 })
}

async fn save(h: &LiveHarness, user: &TestUser, body: &Value) -> (StatusCode, Value) {
    send(&h.app, Method::POST, SAVED, Some(&user.bearer), Some(body)).await
}

async fn rows_for(pool: &PgPool, user: Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM saved_searches WHERE user_id = $1")
        .bind(user)
        .fetch_one(pool)
        .await
        .expect("count saved searches")
}

async fn listed_ids(h: &LiveHarness, user: &TestUser) -> Vec<Uuid> {
    let (status, body) = send(&h.app, Method::GET, SAVED, Some(&user.bearer), None).await;
    assert_eq!(status, StatusCode::OK, "list: {:?}", error_of(&body));
    body.as_array()
        .unwrap_or_else(|| panic!("the list is a JSON array, got {body}"))
        .iter()
        .map(id_of)
        .collect()
}

/// Opens the digest window as if the search had been saved, or last notified, a day and an hour
/// ago.
async fn open_window(pool: &PgPool, search: Uuid) {
    sqlx::query(
        "UPDATE saved_searches SET last_notified_at = now() - interval '25 hours' WHERE id = $1",
    )
    .bind(search)
    .execute(pool)
    .await
    .expect("open the digest window");
}

async fn window_start(pool: &PgPool, search: Uuid) -> DateTime<Utc> {
    sqlx::query_scalar::<_, DateTime<Utc>>(
        "SELECT last_notified_at FROM saved_searches WHERE id = $1",
    )
    .bind(search)
    .fetch_one(pool)
    .await
    .expect("read the window start")
}

async fn seed_post(pool: &PgPool, author: Uuid, title: &str) {
    sqlx::query(
        "INSERT INTO posts (id, author_id, kind, category, title, status, visibility)
         VALUES ($1, $2, 'offer', 'food', $3, 'active', 'public')",
    )
    .bind(Uuid::now_v7())
    .bind(author)
    .bind(title)
    .execute(pool)
    .await
    .expect("insert test post");
}

async fn digests_for(pool: &PgPool, user: Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM notifications WHERE user_id = $1 AND kind = $2",
    )
    .bind(user)
    .bind(saved_search_digest::NOTIFICATION_KIND)
    .fetch_one(pool)
    .await
    .expect("count digest notifications")
}

/// A word no other test's post carries, so a digest counts only this test's posts.
fn unique_word(user: &TestUser) -> String {
    format!("digest{}", user.id.simple())
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_saved_search_is_created_and_then_listed() {
    let h = live_harness().await;
    let user = seed_user(&h.pool).await;

    let (status, body) = save(&h, &user, &wool_near_home()).await;
    assert_eq!(status, StatusCode::CREATED, "save: {:?}", error_of(&body));
    let id = id_of(&body);
    assert_eq!(body["q"], json!("wool"));
    assert_eq!(body["radius_km"], json!(25.0));

    assert_eq!(listed_ids(&h, &user).await, vec![id]);
    assert_eq!(rows_for(&h.pool, user.id).await, 1);
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_saved_centre_is_stored_and_served_on_the_coarse_grid() {
    let h = live_harness().await;
    let user = seed_user(&h.pool).await;

    let (status, body) = save(&h, &user, &wool_near_home()).await;
    assert_eq!(status, StatusCode::CREATED, "save: {:?}", error_of(&body));
    assert_eq!(body["near_lat"], json!(HOME_CELL.0));
    assert_eq!(body["near_lon"], json!(HOME_CELL.1));

    let stored = sqlx::query_as::<_, (Option<f64>, Option<f64>)>(
        "SELECT near_lat, near_lon FROM saved_searches WHERE id = $1",
    )
    .bind(id_of(&body))
    .fetch_one(&h.pool)
    .await
    .expect("read the saved centre");
    assert_eq!(
        stored,
        (Some(HOME_CELL.0), Some(HOME_CELL.1)),
        "the exact point must never reach the table"
    );
}

// The API coarsens too, so only a direct call shows the store does not rely on it.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn the_store_coarsens_a_centre_it_is_handed_exact() {
    let h = live_harness().await;
    let user = seed_user(&h.pool).await;
    let exact = NewSavedSearch {
        label: None,
        q: Some("wool".to_string()),
        kind: None,
        category: None,
        near: Some((HOME_LAT, HOME_LON)),
        radius_km: Some(25.0),
    };

    let created = db::saved_searches::create(&h.pool, user.id, &exact)
        .await
        .expect("create");
    let Created::Saved(row) = created else {
        panic!("a first save is stored");
    };
    assert_eq!(
        (row.near_lat, row.near_lon),
        (Some(HOME_CELL.0), Some(HOME_CELL.1))
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn saving_the_same_search_twice_is_a_409_and_leaves_one_row() {
    let h = live_harness().await;
    let user = seed_user(&h.pool).await;

    let (status, body) = save(&h, &user, &wool_near_home()).await;
    assert_eq!(status, StatusCode::CREATED, "first: {:?}", error_of(&body));

    let (status, body) = save(&h, &user, &wool_near_home()).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "second: {:?}",
        error_of(&body)
    );
    assert!(
        !error_of(&body).is_empty(),
        "a 409 must say why, not answer with an empty body"
    );
    assert_eq!(rows_for(&h.pool, user.id).await, 1);
}

// Postgres treats NULLs as distinct by default, so without NULLS NOT DISTINCT a search with no
// kind, category or centre is stored twice.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_search_with_no_kind_category_or_centre_still_collides_with_itself() {
    let h = live_harness().await;
    let user = seed_user(&h.pool).await;
    let bare = json!({ "q": "ladder" });

    let (status, body) = save(&h, &user, &bare).await;
    assert_eq!(status, StatusCode::CREATED, "first: {:?}", error_of(&body));

    let (status, body) = save(&h, &user, &bare).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "second: {:?}",
        error_of(&body)
    );
    assert_eq!(rows_for(&h.pool, user.id).await, 1);
}

// The key holds the coarsened centre, so two exact points in one cell are one search.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn two_points_in_the_same_cell_are_the_same_search() {
    let h = live_harness().await;
    let user = seed_user(&h.pool).await;

    let (status, body) = save(&h, &user, &wool_near_home()).await;
    assert_eq!(status, StatusCode::CREATED, "first: {:?}", error_of(&body));

    let same_cell = json!({ "q": "wool", "near_lat": 37.81, "near_lon": -122.29, "radius_km": 25 });
    let (status, body) = save(&h, &user, &same_cell).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "same cell: {:?}",
        error_of(&body)
    );
    assert_eq!(rows_for(&h.pool, user.id).await, 1);
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_different_centre_radius_or_filter_is_a_different_search() {
    let h = live_harness().await;
    let user = seed_user(&h.pool).await;

    let variants = [
        wool_near_home(),
        json!({ "q": "wool", "near_lat": 51.5072, "near_lon": -0.1276, "radius_km": 25 }),
        json!({ "q": "wool", "near_lat": HOME_LAT, "near_lon": HOME_LON, "radius_km": 50 }),
        json!({ "q": "wool", "near_lat": HOME_LAT, "near_lon": HOME_LON }),
        json!({ "q": "wool", "kind": "offer", "near_lat": HOME_LAT, "near_lon": HOME_LON, "radius_km": 25 }),
    ];
    for variant in &variants {
        let (status, body) = save(&h, &user, variant).await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "{variant}: {:?}",
            error_of(&body)
        );
    }
    assert_eq!(rows_for(&h.pool, user.id).await, variants.len() as i64);
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn two_people_may_save_the_same_search_and_each_lists_only_their_own() {
    let h = live_harness().await;
    let alice = seed_user(&h.pool).await;
    let bob = seed_user(&h.pool).await;

    let (status, body) = save(&h, &alice, &wool_near_home()).await;
    assert_eq!(status, StatusCode::CREATED, "alice: {:?}", error_of(&body));
    let alices = id_of(&body);
    let (status, body) = save(&h, &bob, &wool_near_home()).await;
    assert_eq!(status, StatusCode::CREATED, "bob: {:?}", error_of(&body));
    let bobs = id_of(&body);

    assert_eq!(listed_ids(&h, &alice).await, vec![alices]);
    assert_eq!(listed_ids(&h, &bob).await, vec![bobs]);
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn the_owner_deletes_a_saved_search_and_it_is_gone() {
    let h = live_harness().await;
    let user = seed_user(&h.pool).await;
    let (status, body) = save(&h, &user, &wool_near_home()).await;
    assert_eq!(status, StatusCode::CREATED, "save: {:?}", error_of(&body));
    let id = id_of(&body);

    let path = format!("{SAVED}/{id}");
    let (status, body) = send(&h.app, Method::DELETE, &path, Some(&user.bearer), None).await;
    assert!(
        status.is_success(),
        "delete: {status} {:?}",
        error_of(&body)
    );

    assert!(listed_ids(&h, &user).await.is_empty());
    assert_eq!(rows_for(&h.pool, user.id).await, 0);

    let (status, body) = save(&h, &user, &wool_near_home()).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "a deleted search may be saved again: {:?}",
        error_of(&body)
    );
}

// 404 rather than 403, so a stranger cannot tell another user's search id from a made-up one.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_stranger_cannot_delete_someone_elses_saved_search() {
    let h = live_harness().await;
    let owner = seed_user(&h.pool).await;
    let stranger = seed_user(&h.pool).await;
    let (status, body) = save(&h, &owner, &wool_near_home()).await;
    assert_eq!(status, StatusCode::CREATED, "save: {:?}", error_of(&body));
    let id = id_of(&body);

    let path = format!("{SAVED}/{id}");
    let (status, body) = send(&h.app, Method::DELETE, &path, Some(&stranger.bearer), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{:?}", error_of(&body));
    assert_eq!(rows_for(&h.pool, owner.id).await, 1);
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn every_saved_search_route_needs_a_session() {
    let h = live_harness().await;
    let owner = seed_user(&h.pool).await;
    let (status, body) = save(&h, &owner, &wool_near_home()).await;
    assert_eq!(status, StatusCode::CREATED, "save: {:?}", error_of(&body));
    let path = format!("{SAVED}/{}", id_of(&body));

    let (status, _) = send(&h.app, Method::GET, SAVED, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "list");
    let (status, _) = send(&h.app, Method::POST, SAVED, None, Some(&wool_near_home())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "save");
    let (status, _) = send(&h.app, Method::DELETE, &path, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "delete");
    assert_eq!(rows_for(&h.pool, owner.id).await, 1);
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_due_search_gets_one_digest_and_a_second_run_in_the_window_gets_none() {
    let h = live_harness().await;
    let owner = seed_user(&h.pool).await;
    let neighbour = seed_user(&h.pool).await;
    let word = unique_word(&owner);
    let (status, body) = save(&h, &owner, &json!({ "q": word })).await;
    assert_eq!(status, StatusCode::CREATED, "save: {:?}", error_of(&body));
    open_window(&h.pool, id_of(&body)).await;
    seed_post(&h.pool, neighbour.id, &format!("Spare {word}")).await;
    seed_post(&h.pool, neighbour.id, &format!("More {word}")).await;

    saved_search_digest::run_once(&h.state)
        .await
        .expect("first digest run");
    assert_eq!(
        digests_for(&h.pool, owner.id).await,
        1,
        "two matching posts make one digest"
    );

    saved_search_digest::run_once(&h.state)
        .await
        .expect("second digest run");
    assert_eq!(
        digests_for(&h.pool, owner.id).await,
        1,
        "the window is spent until it closes again"
    );
}

// Two runs that read the same window before either claims it; the run-level test above cannot
// show this, because its second run reads the moved window and finds nothing due.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn of_two_claims_on_the_same_window_the_first_wins_and_the_second_is_refused() {
    let h = live_harness().await;
    let owner = seed_user(&h.pool).await;
    let (status, body) = save(&h, &owner, &json!({ "q": unique_word(&owner) })).await;
    assert_eq!(status, StatusCode::CREATED, "save: {:?}", error_of(&body));
    let search = id_of(&body);
    open_window(&h.pool, search).await;

    let seen = window_start(&h.pool, search).await;
    let first_now = seen + Duration::hours(25);
    let second_now = seen + Duration::hours(26);

    let mut first = h.pool.begin().await.expect("begin the first claim");
    let won = db::saved_searches::claim_window(&mut first, search, seen, first_now)
        .await
        .expect("first claim");
    first.commit().await.expect("commit the first claim");
    assert!(won, "the first claim on an open window wins");

    let mut second = h.pool.begin().await.expect("begin the second claim");
    let won = db::saved_searches::claim_window(&mut second, search, seen, second_now)
        .await
        .expect("second claim");
    second.commit().await.expect("commit the second claim");
    assert!(
        !won,
        "a claim made with a window start that has moved is refused"
    );

    assert_eq!(
        window_start(&h.pool, search).await,
        first_now,
        "the refused claim must leave the first claim's window in place"
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn the_owners_own_post_sends_no_digest_and_leaves_the_window_open() {
    let h = live_harness().await;
    let owner = seed_user(&h.pool).await;
    let word = unique_word(&owner);
    let (status, body) = save(&h, &owner, &json!({ "q": word })).await;
    assert_eq!(status, StatusCode::CREATED, "save: {:?}", error_of(&body));
    let search = id_of(&body);
    open_window(&h.pool, search).await;
    seed_post(&h.pool, owner.id, &format!("My own {word}")).await;

    saved_search_digest::run_once(&h.state)
        .await
        .expect("digest run");

    assert_eq!(digests_for(&h.pool, owner.id).await, 0);
    let still_open: bool = sqlx::query_scalar(
        "SELECT last_notified_at < now() - interval '24 hours' FROM saved_searches WHERE id = $1",
    )
    .bind(search)
    .fetch_one(&h.pool)
    .await
    .expect("read the window");
    assert!(
        still_open,
        "a run that sends nothing must not spend the window"
    );
}
