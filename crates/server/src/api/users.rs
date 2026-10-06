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

/// Below this many reviews the mean is withheld from the profile: a handful of reviews is an
/// anecdote, and at neighbourhood scale a published anecdote is a lasting reputation.
pub(crate) const RATING_PUBLISH_THRESHOLD: i64 = 5;

pub(crate) fn publishable_rating(rating_avg: Option<f64>, rating_count: i64) -> Option<f64> {
    rating_avg.filter(|_| rating_count >= RATING_PUBLISH_THRESHOLD)
}

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
        // `rating_avg` is `null`, never 0, until `rating_count` reaches the publish threshold.
        // `rating_count` stays public: a withheld mean still reads as reviewed, not yet rated.
        "rating_avg": publishable_rating(row.rating_avg, row.rating_count),
        "rating_count": row.rating_count,
        "joined_at": row.created_at,
        "last_seen": row.last_seen,
        "profile_json": row.profile_json,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publishable_rating_is_withheld_below_the_threshold() {
        for count in [0, 1, 4] {
            assert_eq!(
                publishable_rating(Some(4.5), count),
                None,
                "rating_count {count}"
            );
        }
        assert_eq!(publishable_rating(None, 0), None);
    }

    #[test]
    fn publishable_rating_is_published_at_the_threshold() {
        // Checked at compile time: clippy rejects a run-time assert over two constants.
        const _: () = assert!(RATING_PUBLISH_THRESHOLD == 5);
        assert_eq!(publishable_rating(Some(4.5), 5), Some(4.5));
        assert_eq!(publishable_rating(Some(4.5), 6), Some(4.5));
    }
}
