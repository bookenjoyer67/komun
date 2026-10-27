//! `/users/{id}` and its sub-resources agree on what an unknown id is.
//!
//! The profile answers 404 for an id that does not exist; the reviews and endorsements lists must
//! too, or a caller cannot tell an unknown id from an empty one. Every test needs a live Postgres,
//! so every test is `#[ignore]`d; run against a disposable database with
//! `KOMUN_TEST_DATABASE_URL=postgres://... cargo test -p komun-server
//! tests::user_subresources -- --ignored --test-threads=1`. All data is synthetic.

use axum::http::{Method, StatusCode};
use serde_json::json;
use uuid::Uuid;

use crate::tests::support::{error_of, live_harness, seed_user, send};

/// A valid id no user carries: `now_v7` is time-ordered, and the fresh test database has no such
/// row.
fn unknown_user_id() -> Uuid {
    Uuid::now_v7()
}

/// The bug this pins: before the fix the two lists answered 200 for an id with no user row, so an
/// unknown id and an empty one read the same.
#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn an_unknown_user_id_is_not_found_by_every_sub_resource() {
    let h = live_harness().await;
    let id = unknown_user_id();

    for path in [
        format!("/users/{id}/reviews"),
        format!("/users/{id}/endorsements"),
    ] {
        let (status, body) = send(&h.app, Method::GET, &path, None, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path} must 404");
        assert_eq!(
            error_of(&body),
            "user not found",
            "{path} must name the missing user"
        );
    }
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_known_user_still_gets_the_empty_lists() {
    let h = live_harness().await;
    let user = seed_user(&h.pool, "user", "Sub-resource test user").await;

    let (status, body) = send(
        &h.app,
        Method::GET,
        &format!("/users/{}/reviews", user.id),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));

    let (status, body) = send(
        &h.app,
        Method::GET,
        &format!("/users/{}/endorsements", user.id),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"count": 0, "endorsements": []}));
}
