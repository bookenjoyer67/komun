mod admin;
mod reports;
// `pub(crate)` on these three only so `crate::tests::market` can unit-test their validators
// directly; nothing outside the crate can reach them.
pub(crate) mod categories;
pub(crate) mod conversations;
pub mod directory;
mod endorsements;
mod error;
pub(crate) mod geocode;
mod health;
mod link_preview;
mod node;
mod notifications;
pub(crate) mod outbound;
pub(crate) mod posts;
pub(crate) mod reviews;
pub(crate) mod saved_searches;
mod search;
mod users;

use std::net::SocketAddr;
use std::time::Duration;

use axum::{
    extract::{DefaultBodyLimit, Request},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use uuid::Uuid;

use crate::auth;
use crate::rate_limit::{self, RouteClass};
use crate::AppState;

pub use error::StatusError;

/// The body cap for every route; an upload route raises its own with a route-level
/// `DefaultBodyLimit`, which wins because it sits closer to the handler.
const BODY_LIMIT: usize = 256 * 1024;

/// What a multipart body carries beyond the files themselves.
pub(crate) const MULTIPART_OVERHEAD: usize = 64 * 1024;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);

pub fn router(state: AppState) -> Router {
    let mut r = Router::new()
        .merge(
            Router::new()
                .route("/geocode", get(geocode::geocode))
                .with_state(state.clone()),
        )
        .merge(health::router())
        .merge(node::router(state.clone()))
        .merge(conversations::router(state.clone()))
        .merge(notifications::router(state.clone()))
        .merge(saved_searches::router(state.clone()))
        .merge(admin::router(state.clone()))
        // Mounted flat, not nested: the two halves sit under different path prefixes and guards.
        .merge(categories::router(state.clone()))
        .merge(reports::router(state.clone()))
        .merge(search::router(state.clone()))
        // A review hangs off the deal it is about, not the thread the negotiation happened in.
        .merge(reviews::router(state.clone()))
        .nest("/auth", auth::router(state.clone()))
        // Reviews read as a fact about a profile, so they join the `/users` nest rather than a
        // second top-level route axum would have to resolve.
        .nest(
            "/users",
            users::router(state.clone())
                .merge(endorsements::router(state.clone()))
                .merge(reviews::user_router(state.clone())),
        )
        .nest("/posts", posts::router(state.clone()));

    r = r.merge(
        Router::new()
            .route("/link-preview", get(link_preview::link_preview))
            .with_state(state.clone()),
    );

    if state.config.discovery.directory_enabled {
        r = r.merge(directory::router(state));
    }

    // Applied here rather than in `main`, so a test driving this router meets the same limits.
    r.layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(middleware::from_fn(bounded_time))
}

/// A request not answered in time gets 408, so a client that stalls its body cannot hold a
/// handler open. Uploads get longer: a large body on a slow link is legitimate.
async fn bounded_time(request: Request, next: Next) -> Response {
    let limit = if is_upload(request.uri().path()) {
        UPLOAD_TIMEOUT
    } else {
        REQUEST_TIMEOUT
    };
    match tokio::time::timeout(limit, next.run(request)).await {
        Ok(response) => response,
        Err(_) => StatusError::with_status(StatusCode::REQUEST_TIMEOUT, "request timed out")
            .into_response(),
    }
}

/// Matched on the path because the two upload routes sit in different routers, one of them
/// nested; a router-level timeout cannot be lengthened by a route inside it.
fn is_upload(path: &str) -> bool {
    let post_images = path
        .strip_prefix("/posts/")
        .and_then(|rest| rest.strip_suffix("/images"))
        .is_some_and(|id| !id.is_empty() && !id.contains('/'));
    post_images || path == "/auth/me/avatar"
}

/// The per-account hourly caps.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Hourly {
    Posts,
    Messages,
    Responses,
    Avatars,
}

impl Hourly {
    fn lock_key(self) -> &'static str {
        match self {
            Hourly::Posts => "hourly-posts",
            Hourly::Messages => "hourly-messages",
            Hourly::Responses => "hourly-responses",
            Hourly::Avatars => "hourly-avatars",
        }
    }

    fn count_sql(self) -> &'static str {
        match self {
            Hourly::Posts => {
                "SELECT COUNT(*) FROM posts WHERE author_id = $1 AND created_at > now() - interval '1 hour'"
            }
            Hourly::Messages => {
                "SELECT COUNT(*) FROM messages WHERE sender_id = $1 AND created_at > now() - interval '1 hour'"
            }
            Hourly::Responses => {
                "SELECT COUNT(*) FROM matches WHERE responder_id = $1 AND created_at > now() - interval '1 hour'"
            }
            Hourly::Avatars => {
                "SELECT COUNT(*) FROM avatar_uploads WHERE user_id = $1 AND uploaded_at > now() - interval '1 hour'"
            }
        }
    }
}

/// The account's count for `cap` in the last hour. It must run inside the transaction that makes
/// the insert it guards: the advisory lock is held until that transaction ends, so a concurrent
/// request reads the count only after this one's insert is visible, and cannot pass the same
/// count twice.
pub(crate) async fn hourly_count(
    conn: &mut sqlx::PgConnection,
    cap: Hourly,
    user_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("{}:{user_id}", cap.lock_key()))
        .execute(&mut *conn)
        .await?;
    sqlx::query_scalar::<_, i64>(cap.count_sql())
        .bind(user_id)
        .fetch_one(&mut *conn)
        .await
}

pub(crate) const COUNT_UNAVAILABLE: &str = "try again shortly";

/// An unreadable count is never read as zero: that would lift the cap exactly when the database
/// is struggling.
pub(crate) fn count_unavailable(cap: Hourly, error: sqlx::Error) -> StatusError {
    tracing::warn!("hourly count {cap:?} failed: {error}");
    StatusError::with_status(StatusCode::SERVICE_UNAVAILABLE, COUNT_UNAVAILABLE)
}

/// A per-IP limit was hit. Kept to the seconds alone rather than a built `Response`, which is
/// large enough to trip `clippy::result_large_err` on every `Result` that carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RateLimited {
    retry_after_seconds: u64,
}

impl IntoResponse for RateLimited {
    fn into_response(self) -> Response {
        let seconds = self.retry_after_seconds;
        (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, seconds.to_string())],
            Json(serde_json::json!({
                "error": "too many requests, try again later",
                "retry_after_seconds": seconds,
            })),
        )
            .into_response()
    }
}

/// The per-IP limit for an anonymous route that makes this server do outbound work, keyed the
/// same way as the auth limits.
pub(crate) fn per_ip_limit(
    state: &AppState,
    class: RouteClass,
    peer: SocketAddr,
    headers: &HeaderMap,
) -> Result<(), RateLimited> {
    let ip = rate_limit::client_key(state, peer, headers);
    state
        .rate_limiter
        .check(class, ip)
        .map_err(|retry| RateLimited {
            retry_after_seconds: retry.as_secs().max(1),
        })
}

#[cfg(test)]
mod tests {
    use super::is_upload;

    #[test]
    fn only_the_two_upload_routes_get_the_upload_timeout() {
        assert!(is_upload(
            "/posts/0192f0c4-0000-7000-8000-000000000000/images"
        ));
        assert!(is_upload("/auth/me/avatar"));
        for path in [
            "/posts",
            "/posts/",
            "/posts//images",
            "/posts/a/b/images",
            "/auth/me",
            "/conversations/x/messages",
        ] {
            assert!(!is_upload(path), "{path}");
        }
    }
}
