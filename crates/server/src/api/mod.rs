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

use axum::{
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};

use crate::auth;
use crate::rate_limit::{self, RouteClass};
use crate::AppState;

pub use error::StatusError;

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

    r
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
