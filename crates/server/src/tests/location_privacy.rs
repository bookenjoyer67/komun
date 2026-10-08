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

use komun_core::models::{coarsen_coordinate, CreatePost, PostKind};

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
async fn seed_legacy_post(pool: &PgPool, author: Uuid, lat: f64, lon: f64) -> (Uuid, String) {
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
    .bind(lat)
    .bind(lon)
    .execute(pool)
    .await
    .expect("insert exact-location post");
    (id, title)
}

async fn seed_exact_post(pool: &PgPool, author: Uuid) -> (Uuid, String) {
    seed_legacy_post(pool, author, EXACT_LAT, EXACT_LON).await
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

/// The distance the public feed reports for one post, measured from `near`.
async fn listed_distance(pool: &PgPool, id: Uuid, title: &str, near: (f64, f64)) -> Option<f64> {
    let filter = db::posts::PostFilter {
        q: Some(title.to_string()),
        near: Some(near),
        ..Default::default()
    };
    db::posts::list(pool, &filter)
        .await
        .expect("list posts")
        .into_iter()
        .find(|item| item.post.id == id)
        .expect("the seeded post is in the public list")
        .distance_km
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

    let mut conn = pool.acquire().await.expect("a test connection");
    let post = db::posts::create(&mut conn, author, new_post("r13-create"))
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
        .find(|item| item.post.id == id)
        .expect("the seeded post is in the public list");
    assert_eq!(item.post.location_lat, Some(COARSE_LAT), "list() lat");
    assert_eq!(item.post.location_lon, Some(COARSE_LON), "list() lon");

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

/// A legacy row is measured from its cell, so it reports exactly the distance a post written
/// coarse into the same cell reports.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_legacy_exact_row_is_as_far_away_as_its_coarse_cell() {
    let pool = live_pool().await;
    let author = seed_user(&pool).await;
    let (legacy_id, legacy_title) = seed_exact_post(&pool, author).await;
    let mut conn = pool.acquire().await.expect("a test connection");
    let twin = db::posts::create(
        &mut conn,
        author,
        new_post(&format!("r13-twin-{legacy_id}")),
    )
    .await
    .expect("create coarse twin");
    let near = (coarsen_coordinate(38.3), coarsen_coordinate(-122.0));

    let legacy = listed_distance(&pool, legacy_id, &legacy_title, near).await;
    let coarse = listed_distance(&pool, twin.id, &twin.title, near).await;
    assert!(legacy.is_some(), "a located post has a distance");
    assert_eq!(
        legacy, coarse,
        "the legacy row must be measured from its cell"
    );
}

/// A centre on a tie's Rust-coarsened cell is 0 km from it only if SQL rounds the tie the same
/// way; a different rule lands one cell over, about 8 to 11 km away.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn sql_coarsening_rounds_ties_as_coarsen_coordinate_does() {
    let pool = live_pool().await;
    let author = seed_user(&pool).await;

    for (lat, lon) in [(37.85, -122.25), (-37.85, 122.25), (0.05, -0.05)] {
        let (id, title) = seed_legacy_post(&pool, author, lat, lon).await;
        let near = (coarsen_coordinate(lat), coarsen_coordinate(lon));
        assert_eq!(
            listed_distance(&pool, id, &title, near).await,
            Some(0.0),
            "tie {lat},{lon}"
        );
    }
}

/// Postgres `LEAST` skips a NULL argument, so an unguarded haversine measures a missing centre as
/// `asin(1)`: every located post half the planet away.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_feed_without_a_centre_reports_no_distance() {
    let pool = live_pool().await;
    let author = seed_user(&pool).await;
    let (id, title) = seed_legacy_post(&pool, author, 38.6, -90.2).await;

    let filter = db::posts::PostFilter {
        q: Some(title.clone()),
        ..Default::default()
    };
    let item = db::posts::list(&pool, &filter)
        .await
        .expect("list posts")
        .into_iter()
        .find(|item| item.post.id == id)
        .expect("the seeded post is in the public list");
    assert_eq!(item.distance_km, None, "no centre, no distance");

    assert_eq!(
        listed_distance(&pool, id, &title, (38.6, -90.2)).await,
        Some(0.0),
        "a centre still yields real km"
    );
}

/// An unlocated post's cell is NULL, and `LEAST` skips that NULL as it skips a missing centre: an
/// unguarded haversine puts the post half the planet from any centre.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn an_unlocated_post_has_no_distance_from_a_centre() {
    let pool = live_pool().await;
    let author = seed_user(&pool).await;
    let tag = format!("r13-unlocated-{}", Uuid::now_v7());
    let mut conn = pool.acquire().await.expect("a test connection");
    // Created first, so a recency order alone would list it second.
    let located = db::posts::create(&mut conn, author, new_post(&format!("{tag}-located")))
        .await
        .expect("create located post");
    let unlocated = db::posts::create(
        &mut conn,
        author,
        CreatePost {
            location_lat: None,
            location_lon: None,
            ..new_post(&tag)
        },
    )
    .await
    .expect("create unlocated post");
    let near = (COARSE_LAT, COARSE_LON);

    assert_eq!(
        listed_distance(&pool, unlocated.id, &unlocated.title, near).await,
        None,
        "no location, no distance"
    );

    let ids = |items: Vec<db::posts::FeedPost>| -> Vec<Uuid> {
        items.into_iter().map(|item| item.post.id).collect()
    };

    let within = db::posts::PostFilter {
        q: Some(tag.clone()),
        near: Some(near),
        radius_km: Some(db::posts::MAX_RADIUS_KM),
        ..Default::default()
    };
    assert_eq!(
        ids(db::posts::list(&pool, &within).await.expect("list posts")),
        vec![located.id],
        "a radius excludes an unlocated post"
    );

    let nearest_first = db::posts::PostFilter {
        q: Some(tag),
        near: Some(near),
        sort: db::posts::FeedSort::Distance,
        ..Default::default()
    };
    assert_eq!(
        ids(db::posts::list(&pool, &nearest_first)
            .await
            .expect("list posts")),
        vec![located.id, unlocated.id],
        "the distance order puts an unlocated post last"
    );
}
