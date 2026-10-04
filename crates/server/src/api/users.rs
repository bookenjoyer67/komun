use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde_json::json;

use crate::auth;
use crate::db::users;
use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new().route("/{id}", get(profile)).with_state(state)
}

async fn profile(
    State(state): State<AppState>,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let row = users::get_profile(&state.pool, id)
        .await
        .map_err(|e| {
            tracing::error!("profile lookup failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal error"})),
            )
        })?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "user not found"})),
        ))?;

    Ok(Json(json!({
        "id": row.id,
        "display_name": row.display_name,
        "bio": row.bio,
        "avatar_url": row.avatar_path.map(|p| format!("/avatars/{}", p)),
        "encryption_public_key": row.encryption_public_key.map(|k| auth::encode_b64(&k)),
        "role": row.role,
        "post_count": row.post_count,
        "verified_post_count": row.verified_post_count,
        "endorsement_count": row.endorsement_count,
        // `rating_avg` is `null`, never 0, for a user nobody has reviewed; `rating_count`
        // distinguishes a reputation from a single review.
        "rating_avg": row.rating_avg,
        "rating_count": row.rating_count,
        "joined_at": row.created_at,
        "last_seen": row.last_seen,
        "profile_json": row.profile_json,
    })))
}
