//! The profile query supplies exactly the counts the badge rules need (D3).
//!
//! Every test needs a live Postgres, so every test is `#[ignore]`d; run against a disposable
//! database with `KOMUN_TEST_DATABASE_URL=postgres://... cargo test -p komun-server
//! tests::badges -- --ignored --test-threads=1`. Each test seeds its own users and posts, so they
//! run in any order. All data is synthetic.

use chrono::{DateTime, Duration, Utc};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

use crate::badges::{self, Badge};
use crate::{auth, db};

const DATABASE_ENV: &str = "KOMUN_TEST_DATABASE_URL";

/// A synthetic verifier in the shape the client sends; it is not real material.
const VERIFIER: &str = "c3ludGhldGljLXRlc3QtdmVyaWZpZXItZm9yLXdhdmU0LWJhZGdlcw";

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
    let email = format!("wave4-badges-{id}@test.invalid");
    let password_hash = auth::password::hash_verifier(VERIFIER).expect("hash");
    sqlx::query(
        "INSERT INTO users (id, email, email_verified_at, password_hash, auth_salt, display_name, role)
         VALUES ($1, $2, now(), $3, $4, $5, 'user')",
    )
    .bind(id)
    .bind(email)
    .bind(password_hash)
    .bind(vec![0x5a_u8; 16])
    .bind("Wave 4 test user")
    .execute(pool)
    .await
    .expect("insert test user");
    id
}

async fn endorse(pool: &PgPool, endorser: Uuid, endorsee: Uuid) {
    sqlx::query("INSERT INTO endorsements (id, endorser_id, endorsee_id) VALUES ($1, $2, $3)")
        .bind(Uuid::now_v7())
        .bind(endorser)
        .bind(endorsee)
        .execute(pool)
        .await
        .expect("insert endorsement");
}

async fn seed_post(pool: &PgPool, author: Uuid, kind: &str, status: &str, at: DateTime<Utc>) {
    seed_post_as(pool, author, kind, status, "public", at).await;
}

async fn seed_post_as(
    pool: &PgPool,
    author: Uuid,
    kind: &str,
    status: &str,
    visibility: &str,
    at: DateTime<Utc>,
) {
    sqlx::query(
        "INSERT INTO posts (id, author_id, kind, category, title, status, visibility, created_at)
         VALUES ($1, $2, $3, 'food', 'wave4-badge-post', $4, $5, $6)",
    )
    .bind(Uuid::now_v7())
    .bind(author)
    .bind(kind)
    .bind(status)
    .bind(visibility)
    .bind(at)
    .execute(pool)
    .await
    .expect("insert post");
}

async fn profile(pool: &PgPool, user: Uuid) -> db::users::UserProfileRow {
    db::users::get_profile(pool, user)
        .await
        .expect("profile query")
        .expect("the seeded user exists")
}

async fn earned_for(pool: &PgPool, user: Uuid) -> Vec<Badge> {
    let row = profile(pool, user).await;
    badges::earned(
        row.endorsement_count,
        row.give_count,
        row.first_give_at,
        row.last_give_at,
    )
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn three_endorsements_earn_reliable_and_two_do_not() {
    let pool = live_pool().await;
    let subject = seed_user(&pool).await;
    for _ in 0..2 {
        let endorser = seed_user(&pool).await;
        endorse(&pool, endorser, subject).await;
    }
    let earned = earned_for(&pool, subject).await;
    assert!(!earned.contains(&Badge::Reliable), "two endorsements");

    let third = seed_user(&pool).await;
    endorse(&pool, third, subject).await;
    let earned = earned_for(&pool, subject).await;
    assert!(earned.contains(&Badge::Reliable), "three endorsements");
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn only_offer_and_resource_posts_the_public_feed_shows_count_toward_regular_giver() {
    let pool = live_pool().await;
    let subject = seed_user(&pool).await;
    let now = Utc::now();
    let ago = |days: i64| now - Duration::days(days);

    // None of these may raise the count or widen the span: a need is not a give, and a post the
    // public feed does not show (moderated, withdrawn or private) never earns a public badge.
    // Any one of them counted would lift the four gives below to five over more than 30 days.
    seed_post(&pool, subject, "need", "active", ago(200)).await;
    seed_post(&pool, subject, "offer", "hidden", ago(300)).await;
    seed_post(&pool, subject, "resource", "flagged", ago(400)).await;
    seed_post(&pool, subject, "offer", "withdrawn", ago(60)).await;
    seed_post_as(&pool, subject, "resource", "active", "private", ago(100)).await;

    for days in [20, 15, 10, 5] {
        let kind = if days % 10 == 0 { "offer" } else { "resource" };
        seed_post(&pool, subject, kind, "active", ago(days)).await;
    }
    let row = profile(&pool, subject).await;
    assert_eq!(row.give_count, 4, "only the four feed-visible gives count");
    let earned = earned_for(&pool, subject).await;
    assert!(!earned.contains(&Badge::RegularGiver), "four gives");

    seed_post(&pool, subject, "offer", "fulfilled", ago(40)).await;
    let row = profile(&pool, subject).await;
    assert_eq!(row.give_count, 5);
    let (Some(first), Some(last)) = (row.first_give_at, row.last_give_at) else {
        panic!("a user with gives must have a dated give history");
    };
    let span = last - first;
    assert!(
        span >= Duration::days(35) && span < Duration::days(36),
        "the span must run from the 40-day give to the 5-day give, got {span}"
    );
    let earned = earned_for(&pool, subject).await;
    assert!(
        earned.contains(&Badge::RegularGiver),
        "five gives over 35 days"
    );
}
