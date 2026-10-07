//! The shared HTTP error type.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

/// An `anyhow::Error` carrying the status code it should be reported as. The blanket `From` makes
/// `?` produce a 500; anything the client may see is built with [`StatusError::with_status`].
pub struct StatusError {
    status: StatusCode,
    inner: anyhow::Error,
}

impl<E: Into<anyhow::Error>> From<E> for StatusError {
    fn from(err: E) -> Self {
        StatusError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            inner: err.into(),
        }
    }
}

impl StatusError {
    pub fn with_status(status: StatusCode, message: impl std::fmt::Display) -> Self {
        StatusError {
            status,
            inner: anyhow::anyhow!("{}", message),
        }
    }
}

impl IntoResponse for StatusError {
    fn into_response(self) -> Response {
        // Only the 500s are logged: a 404 or a 400 is the client's problem, not an incident.
        let message = if self.status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!("request error: {:?}", self.inner);
            // A 500 body never carries the error's text; the log does.
            "internal error".to_owned()
        } else {
            self.inner.to_string()
        };
        (self.status, Json(serde_json::json!({"error": message}))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    async fn rendered(err: StatusError) -> (StatusCode, Value) {
        let response = err.into_response();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("read response body");
        let body = serde_json::from_slice(&bytes).expect("a JSON error body");
        (status, body)
    }

    #[tokio::test]
    async fn a_server_error_body_is_generic() {
        let err = StatusError::from(anyhow::anyhow!("relation marker-r14 detail"));

        let (status, body) = rendered(err).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            !body.to_string().contains("marker-r14"),
            "the body must not carry the error's text"
        );
        assert_eq!(body, json!({ "error": "internal error" }));
    }

    #[tokio::test]
    async fn a_client_error_keeps_its_message() {
        let err = StatusError::with_status(StatusCode::BAD_REQUEST, "bad input");

        let (status, body) = rendered(err).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, json!({ "error": "bad input" }));
    }
}
