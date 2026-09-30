use axum::{
    extract::{Extension, Path, State},
    middleware,
    routing::{get, patch, post},
    Json, Router,
};
use uuid::Uuid;

use super::StatusError;
use crate::auth::{require_auth, AuthUser};
use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/me/notifications", get(list_notifications))
        .route("/me/notifications/count", get(unread_count))
        .route("/me/notifications/{id}/read", patch(mark_read))
        .route("/me/notifications/read-all", post(mark_all_read))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .with_state(state)
}

async fn list_notifications(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<Vec<crate::db::notifications::Notification>>, StatusError> {
    let notifs = crate::db::notifications::list(&state.pool, auth.user_id).await?;
    Ok(Json(notifs))
}

async fn unread_count(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, StatusError> {
    let count = crate::db::notifications::unread_count(&state.pool, auth.user_id).await?;
    Ok(Json(serde_json::json!({"unread": count})))
}

async fn mark_read(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusError> {
    crate::db::notifications::mark_read(&state.pool, id, auth.user_id).await?;
    Ok(Json(serde_json::json!({"status": "read"})))
}

async fn mark_all_read(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, StatusError> {
    crate::db::notifications::mark_all_read(&state.pool, auth.user_id).await?;
    Ok(Json(serde_json::json!({"status": "all read"})))
}
