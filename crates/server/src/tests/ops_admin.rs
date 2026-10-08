//! Reporting is open to any signed-in user while moderation stays superadmin-only, user search
//! exposes public fields only, the server is built without a websocket stack, a client mistake on a
//! moderation route is a 400, and admin removals and hides each leave exactly one audit row.
//!
//! Tests marked `#[ignore]` need a live Postgres; run them on the host against a disposable
//! database with `KOMUN_TEST_DATABASE_URL=postgres://... cargo test -p komun-server ops_admin --
//! --ignored`.

use std::collections::BTreeSet;

use axum::http::{Method, StatusCode};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use super::support::{
    app, audit_rows, error_of, live_harness, seed_user, send, state_with, unreachable_pool,
};

const DELETE_USER_ACTION: &str = "admin.delete_user";
const DIRECTORY_REMOVE_ACTION: &str = "directory.remove";
const HIDE_POST_ACTION: &str = "admin.hide_post";

/// Seeded straight into `posts`, so a test starts without going through a guarded route.
async fn seed_post(pool: &PgPool, author: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO posts (id, author_id, kind, category, title, status, visibility)
         VALUES ($1, $2, 'need', 'food', $3, 'active', 'public')",
    )
    .bind(id)
    .bind(author)
    .bind(format!("r14-post-{id}"))
    .execute(pool)
    .await
    .expect("insert test post");
    id
}

async fn seed_report(pool: &PgPool, reporter: Uuid, post: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO reports (id, reporter_id, post_id, reason) VALUES ($1, $2, $3, $4)")
        .bind(id)
        .bind(reporter)
        .bind(post)
        .bind("r14")
        .execute(pool)
        .await
        .expect("insert test report");
    id
}

async fn post_status(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar::<_, String>("SELECT status FROM posts WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("read post status")
}

async fn reports_by(pool: &PgPool, reporter: Uuid, post: Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM reports WHERE reporter_id = $1 AND post_id = $2",
    )
    .bind(reporter)
    .bind(post)
    .fetch_one(pool)
    .await
    .expect("count reports")
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

// ---------------------------------------------------------------------------------------------
// Any signed-in user can report a post; moderation stays superadmin-only.
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn an_anonymous_report_is_unauthorized() {
    let app = app(state_with(unreachable_pool(), ""));
    let path = format!("/posts/{}/report", Uuid::now_v7());
    let body = json!({ "reason": "r14" });

    let (status, response) = send(&app, Method::POST, &path, None, Some(&body)).await;

    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "error: {:?}",
        error_of(&response)
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_signed_in_user_can_report_a_post() {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user", "R14 author").await;
    let reporter = seed_user(&h.pool, "user", "R14 reporter").await;
    let post = seed_post(&h.pool, author.id).await;
    let path = format!("/posts/{post}/report");
    let body = json!({ "reason": "r14" });

    let (status, response) = send(
        &h.app,
        Method::POST,
        &path,
        Some(&reporter.bearer),
        Some(&body),
    )
    .await;

    assert!(
        status.is_success(),
        "got {status}: {:?}",
        error_of(&response)
    );
    assert_eq!(response["reporter_id"], json!(reporter.id));
    assert_eq!(reports_by(&h.pool, reporter.id, post).await, 1);
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_signed_in_user_cannot_moderate() {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user", "R14 author").await;
    let user = seed_user(&h.pool, "user", "R14 user").await;
    let post = seed_post(&h.pool, author.id).await;
    let report = seed_report(&h.pool, author.id, post).await;

    let hide = format!("/posts/{post}/hide");
    let (status, body) = send(&h.app, Method::POST, &hide, Some(&user.bearer), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "hide: {:?}", error_of(&body));
    assert_eq!(
        post_status(&h.pool, post).await,
        "active",
        "a refused hide must leave the post as it was"
    );

    let (status, body) = send(
        &h.app,
        Method::GET,
        "/admin/reports",
        Some(&user.bearer),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "list: {:?}", error_of(&body));

    let resolve = format!("/admin/reports/{report}");
    let decision = json!({ "status": "resolved" });
    let (status, body) = send(
        &h.app,
        Method::PATCH,
        &resolve,
        Some(&user.bearer),
        Some(&decision),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "resolve: {:?}",
        error_of(&body)
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_superadmin_sees_a_user_report() {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user", "R14 author").await;
    let reporter = seed_user(&h.pool, "user", "R14 reporter").await;
    let admin = seed_user(&h.pool, "superadmin", "R14 admin").await;
    let post = seed_post(&h.pool, author.id).await;
    let path = format!("/posts/{post}/report");
    let body = json!({ "reason": "r14" });

    let (status, response) = send(
        &h.app,
        Method::POST,
        &path,
        Some(&reporter.bearer),
        Some(&body),
    )
    .await;
    assert!(
        status.is_success(),
        "report: got {status}: {:?}",
        error_of(&response)
    );
    let report_id = response["id"].clone();

    let (status, listed) = send(
        &h.app,
        Method::GET,
        "/admin/reports",
        Some(&admin.bearer),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "list: {:?}", error_of(&listed));
    let found = listed
        .as_array()
        .is_some_and(|reports| reports.iter().any(|r| r["id"] == report_id));
    assert!(
        found,
        "the user's report must appear in the superadmin list"
    );
}

// ---------------------------------------------------------------------------------------------
// User search returns public fields only.
// ---------------------------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn user_search_returns_public_fields_only() {
    let h = live_harness().await;
    let name = format!("r14search{}", Uuid::now_v7().simple());
    let admin = seed_user(&h.pool, "superadmin", &name).await;
    let path = format!("/search/users?q={name}");

    let (status, body) = send(&h.app, Method::GET, &path, None, None).await;

    assert_eq!(status, StatusCode::OK, "error: {:?}", error_of(&body));
    let item = body
        .as_array()
        .and_then(|items| items.iter().find(|item| item["id"] == json!(admin.id)))
        .expect("the seeded user is in the results");
    assert_eq!(item["display_name"], json!(name));

    let keys: BTreeSet<&str> = item
        .as_object()
        .expect("a result is an object")
        .keys()
        .map(String::as_str)
        .collect();
    let public: BTreeSet<&str> = ["id", "display_name", "endorsement_count"].into();
    assert_eq!(keys, public);
}

// ---------------------------------------------------------------------------------------------
// The server is built without a websocket stack. A compile feature has no runtime behaviour to
// call, so the resolved lock graph is the nearest observable of what gets built.
// ---------------------------------------------------------------------------------------------

const WORKSPACE_LOCK: &str = include_str!("../../../../Cargo.lock");
const SERVER_MANIFEST: &str = include_str!("../../Cargo.toml");

#[test]
fn the_lock_resolves_no_websocket_stack() {
    let lock: toml::Table = toml::from_str(WORKSPACE_LOCK).expect("parse Cargo.lock");
    let packages = lock["package"].as_array().expect("[[package]] entries");

    let names: Vec<&str> = packages
        .iter()
        .filter_map(|p| p.get("name").and_then(toml::Value::as_str))
        .collect();
    assert!(names.contains(&"axum"), "the lock must resolve axum");
    assert!(
        !names.contains(&"tokio-tungstenite"),
        "no websocket implementation may be resolved"
    );
}

#[test]
fn axum_is_built_with_multipart_and_without_ws() {
    let manifest: toml::Table = toml::from_str(SERVER_MANIFEST).expect("parse Cargo.toml");
    let features: Vec<&str> = manifest["dependencies"]["axum"]["features"]
        .as_array()
        .expect("axum declares features")
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();

    assert!(features.contains(&"multipart"), "features: {features:?}");
    assert!(!features.contains(&"ws"), "features: {features:?}");
}

// ---------------------------------------------------------------------------------------------
// A client mistake on a moderation route is a client error.
// ---------------------------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn an_unknown_report_status_is_a_bad_request() {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user", "R14 author").await;
    let admin = seed_user(&h.pool, "superadmin", "R14 admin").await;
    let post = seed_post(&h.pool, author.id).await;
    let report = seed_report(&h.pool, author.id, post).await;
    let path = format!("/admin/reports/{report}");
    let body = json!({ "status": "bogus" });

    let (status, response) = send(
        &h.app,
        Method::PATCH,
        &path,
        Some(&admin.bearer),
        Some(&body),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "error: {:?}",
        error_of(&response)
    );
}

// ---------------------------------------------------------------------------------------------
// Admin removals and hides each leave exactly one audit row.
// ---------------------------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn an_admin_user_delete_is_audited_once() {
    let h = live_harness().await;
    let admin = seed_user(&h.pool, "superadmin", "R14 admin").await;
    let target = seed_user(&h.pool, "user", "R14 target").await;
    let path = format!("/admin/users/{}", target.id);

    let (status, body) = send(&h.app, Method::DELETE, &path, Some(&admin.bearer), None).await;

    assert!(status.is_success(), "got {status}: {:?}", error_of(&body));
    let remaining =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM users WHERE id = $1)")
            .bind(target.id)
            .fetch_one(&h.pool)
            .await
            .expect("read user");
    assert!(!remaining, "the deleted user must be gone");

    let rows = audit_rows(&h.pool, DELETE_USER_ACTION, target.id).await;
    assert_eq!(rows.len(), 1, "want exactly one {DELETE_USER_ACTION} row");
    assert_eq!(rows[0].actor_id, Some(admin.id));
    assert_eq!(rows[0].subject_id, Some(target.id));
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn deleting_an_unknown_user_is_not_found_and_unaudited() {
    let h = live_harness().await;
    let admin = seed_user(&h.pool, "superadmin", "R14 admin").await;
    let unknown = Uuid::now_v7();
    let path = format!("/admin/users/{unknown}");

    let (status, body) = send(&h.app, Method::DELETE, &path, Some(&admin.bearer), None).await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "error: {:?}",
        error_of(&body)
    );
    assert!(audit_rows(&h.pool, DELETE_USER_ACTION, unknown)
        .await
        .is_empty());
    assert!(audit_rows(&h.pool, DELETE_USER_ACTION, admin.id)
        .await
        .is_empty());
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn an_admin_directory_removal_is_audited_once() {
    let h = live_harness().await;
    let admin = seed_user(&h.pool, "superadmin", "R14 admin").await;
    let entry = format!("https://r14-{}.test.invalid", Uuid::now_v7());
    sqlx::query("INSERT INTO directory_entries (url, name) VALUES ($1, $2)")
        .bind(&entry)
        .bind("R14 test peer")
        .execute(&h.pool)
        .await
        .expect("insert directory entry");
    let path = format!("/admin/directory/{}", escape(&entry));

    let (status, body) = send(&h.app, Method::DELETE, &path, Some(&admin.bearer), None).await;

    assert!(status.is_success(), "got {status}: {:?}", error_of(&body));
    let remaining = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM directory_entries WHERE url = $1)",
    )
    .bind(&entry)
    .fetch_one(&h.pool)
    .await
    .expect("read directory entry");
    assert!(!remaining, "the removed entry must be gone");

    let rows = audit_rows(&h.pool, DIRECTORY_REMOVE_ACTION, admin.id).await;
    assert_eq!(
        rows.len(),
        1,
        "want exactly one {DIRECTORY_REMOVE_ACTION} row"
    );
}

#[tokio::test]
#[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
async fn a_superadmin_hide_is_audited_once() {
    let h = live_harness().await;
    let author = seed_user(&h.pool, "user", "R14 author").await;
    let admin = seed_user(&h.pool, "superadmin", "R14 admin").await;
    let post = seed_post(&h.pool, author.id).await;
    let path = format!("/posts/{post}/hide");
    let body = json!({ "reason": "r14" });

    let (status, response) = send(
        &h.app,
        Method::POST,
        &path,
        Some(&admin.bearer),
        Some(&body),
    )
    .await;

    assert!(
        status.is_success(),
        "got {status}: {:?}",
        error_of(&response)
    );
    assert_eq!(post_status(&h.pool, post).await, "hidden");
    let rows = audit_rows(&h.pool, HIDE_POST_ACTION, admin.id).await;
    assert_eq!(rows.len(), 1, "want exactly one {HIDE_POST_ACTION} row");
}
