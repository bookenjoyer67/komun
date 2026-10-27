//! A NUL byte in `q` must be refused as 400 before any query: PostgreSQL refuses U+0000 in a text
//! parameter, so letting it through turns a client mistake into a 500.
//!
//! No database is needed: the pool is lazy and points at a closed port, so a handler that reached
//! the database answers 500 instead of the refusal under test.

use axum::http::{Method, StatusCode};
use serde_json::json;

use super::support::{app, error_of, send, state_with, unreachable_pool};

/// The refusal message is the API's error code: it has no error-code field.
const NUL_REFUSED: &str = "q must not contain a NUL character";

const GENERIC_500: &str = "internal error";

#[tokio::test]
async fn a_nul_byte_in_q_is_a_400_before_the_database() {
    let app = app(state_with(unreachable_pool(), ""));

    for uri in [
        "/search?q=%00",
        "/search/users?q=%00",
        "/posts?q=%00",
        "/search?q=wool%00",
    ] {
        let (status, body) = send(&app, Method::GET, uri, None, None).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{uri}: {:?}",
            error_of(&body)
        );
        assert_eq!(error_of(&body), NUL_REFUSED, "{uri}");
    }
}

/// The guard must not refuse ordinary input, and a genuine fault must keep the generic body.
#[tokio::test]
async fn an_ordinary_q_reaches_the_database_and_a_fault_stays_generic() {
    let app = app(state_with(unreachable_pool(), ""));

    for uri in ["/search?q=wool", "/search/users?q=wool", "/posts?q=wool"] {
        let (status, body) = send(&app, Method::GET, uri, None, None).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{uri}: {body}");
        assert_eq!(body, json!({ "error": GENERIC_500 }), "{uri}");
    }
}
