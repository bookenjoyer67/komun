//! Post coordinates are coarsened on write and on read, and the read path never rewrites a stored
//! row.
//!
//! Every test needs a live Postgres, so every test is `#[ignore]`d; run against a disposable
//! database with `KOMUN_TEST_DATABASE_URL=postgres://... cargo test -p komun-server
//! location_privacy -- --ignored --test-threads=1`. Each test seeds its own user and post, so they
//! run in any order. All data is synthetic.

use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

use komun_core::models::{CreatePost, PostKind};

use crate::{auth, db};

const DATABASE_ENV: &str = "KOMUN_TEST_DATABASE_URL";

/// A synthetic verifier in the shape the client sends; it is not real material.
const VERIFIER: &str = "c3ludGhldGljLXRlc3QtdmVyaWZpZXItZm9yLXIxMy1sb2NhdGlvbg";

const EXACT_LAT: f64 = 37.80443;
const EXACT_LON: f64 = -122.27121;
const COARSE_LAT: f64 = 37.8;
const COARSE_LON: f64 = -122.3;

/// Connects via `KOMUN_TEST_DATABASE_URL`; without it the test panics rather than passing
/// unearned, and the URL is never printed.
async fn live_pool() -> PgPool {
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
    pool
}

async fn seed_user(pool: &PgPool) -> Uuid {
    let id = Uuid::now_v7();
    let email = format!("r13-location-{id}@test.invalid");
    let password_hash = auth::password::hash_verifier(VERIFIER).expect("hash");
    sqlx::query(
        "INSERT INTO users (id, email, email_verified_at, password_hash, auth_salt, display_name, role)
         VALUES ($1, $2, now(), $3, $4, $5, 'user')",
    )
    .bind(id)
    .bind(email)
    .bind(password_hash)
    .bind(vec![0x5a_u8; 16])
    .bind("R13 test user")
    .execute(pool)
    .await
    .expect("insert test user");
    id
}

/// Written past `db::posts::create`, as a row from before coarsening existed would be.
async fn seed_exact_post(pool: &PgPool, author: Uuid) -> (Uuid, String) {
    let id = Uuid::now_v7();
    let title = format!("r13-exact-{id}");
    sqlx::query(
        "INSERT INTO posts (id, author_id, kind, category, title, status, visibility,
                            location_lat, location_lon)
         VALUES ($1, $2, 'need', 'food', $3, 'active', 'public', $4, $5)",
    )
    .bind(id)
    .bind(author)
    .bind(&title)
    .bind(EXACT_LAT)
    .bind(EXACT_LON)
    .execute(pool)
    .await
    .expect("insert exact-location post");
    (id, title)
}

async fn stored_location(pool: &PgPool, id: Uuid) -> (Option<f64>, Option<f64>) {
    sqlx::query_as::<_, (Option<f64>, Option<f64>)>(
        "SELECT location_lat, location_lon FROM posts WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("read stored location")
}

/// Every column of the row: the only way to see that a read moved nothing.
async fn post_row(pool: &PgPool, id: Uuid) -> Value {
    sqlx::query_scalar::<_, Value>("SELECT to_jsonb(p) FROM posts p WHERE p.id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("read full post row")
}

fn bits(pair: (Option<f64>, Option<f64>)) -> (Option<u64>, Option<u64>) {
    (pair.0.map(f64::to_bits), pair.1.map(f64::to_bits))
}

fn new_post(title: &str) -> CreatePost {
    CreatePost {
        kind: PostKind::Need,
        category: "food".to_string(),
        title: title.to_string(),
        body: None,
        location_name: None,
        location_lat: Some(EXACT_LAT),
        location_lon: Some(EXACT_LON),
        urgency: None,
        quantity: None,
        visibility: None,
        expires_at: None,
        tags: None,
        contact_method: None,
        market_listed: false,
        price_cents: None,
        currency: None,
        price_negotiable: false,
        item_condition: None,
    }
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb06_a_new_post_is_stored_coarse() {
    let pool = live_pool().await;
    let author = seed_user(&pool).await;

    let post = db::posts::create(&pool, author, new_post("r13-create"))
        .await
        .expect("create post");

    assert_eq!(
        bits(stored_location(&pool, post.id).await),
        bits((Some(COARSE_LAT), Some(COARSE_LON))),
        "the stored columns must hold the coarse value"
    );
    assert_eq!(post.location_lat, Some(COARSE_LAT));
    assert_eq!(post.location_lon, Some(COARSE_LON));
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn vb06_an_exact_stored_post_is_served_coarse_and_left_unchanged() {
    let pool = live_pool().await;
    let author = seed_user(&pool).await;
    let (id, title) = seed_exact_post(&pool, author).await;
    let before = post_row(&pool, id).await;

    let single = db::posts::get(&pool, id)
        .await
        .expect("get post")
        .expect("the seeded post exists");
    assert_eq!(single.location_lat, Some(COARSE_LAT), "get() lat");
    assert_eq!(single.location_lon, Some(COARSE_LON), "get() lon");

    let filter = db::posts::PostFilter {
        q: Some(title),
        ..Default::default()
    };
    let listed = db::posts::list(&pool, &filter).await.expect("list posts");
    let item = listed
        .iter()
        .find(|post| post.id == id)
        .expect("the seeded post is in the public list");
    assert_eq!(item.location_lat, Some(COARSE_LAT), "list() lat");
    assert_eq!(item.location_lon, Some(COARSE_LON), "list() lon");

    assert_eq!(
        bits(stored_location(&pool, id).await),
        bits((Some(EXACT_LAT), Some(EXACT_LON))),
        "serving a post must not rewrite its stored coordinates"
    );
    assert_eq!(
        post_row(&pool, id).await,
        before,
        "serving a post must leave its row unchanged"
    );
}
