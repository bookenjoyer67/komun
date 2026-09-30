use axum::{
    extract::{Path, State},
    http::StatusCode,
    middleware,
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;

use crate::auth::{require_auth, AuthUser};
use crate::db::endorsements;
use crate::AppState;

#[derive(Deserialize)]
struct EndorseRequest {
    note: Option<String>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/{id}/endorsements", get(list_endorsements))
        .route(
            "/{id}/endorse",
            post(endorse).route_layer(middleware::from_fn_with_state(state.clone(), require_auth)),
        )
        .route(
            "/{id}/endorse",
            delete(unendorse)
                .route_layer(middleware::from_fn_with_state(state.clone(), require_auth)),
        )
        .with_state(state)
}

async fn endorse(
    State(state): State<AppState>,
    Path(endorsee_id): Path<uuid::Uuid>,
    Extension(auth): Extension<AuthUser>,
    Json(input): Json<EndorseRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if auth.user_id == endorsee_id {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "cannot endorse yourself"})),
        ));
    }

    match endorsements::create(&state.pool, auth.user_id, endorsee_id, input.note).await {
        Ok(e) => Ok(Json(json!({"id": e.id, "created_at": e.created_at}))),
        Err(e) if is_unique_violation(&e) => Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "already endorsed"})),
        )),
        Err(e) => {
            tracing::error!("endorse failed: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal error"})),
            ))
        }
    }
}

async fn unendorse(
    State(state): State<AppState>,
    Path(endorsee_id): Path<uuid::Uuid>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let removed = endorsements::remove(&state.pool, auth.user_id, endorsee_id)
        .await
        .map_err(|e| {
            tracing::error!("unendorse failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal error"})),
            )
        })?;
    if removed {
        Ok(Json(json!({"ok": true})))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "endorsement not found"})),
        ))
    }
}

async fn list_endorsements(
    State(state): State<AppState>,
    Path(user_id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let list = endorsements::list_for_user(&state.pool, user_id)
        .await
        .map_err(|e| {
            tracing::error!("list endorsements failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal error"})),
            )
        })?;
    let count = list.len() as i64;
    Ok(Json(json!({"count": count, "endorsements": list})))
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db) = e {
        db.code().is_some_and(|c| c == "23505")
    } else {
        false
    }
}

use axum::extract::Extension;
