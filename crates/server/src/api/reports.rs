use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    middleware,
    routing::{get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use uuid::Uuid;

use super::StatusError;
use crate::auth::{require_auth, require_superadmin, AuthUser};
use crate::AppState;

/// Any signed-in user can file a report, so the stored reason is bounded.
const MAX_REASON_CHARS: usize = 1000;

pub fn router(state: AppState) -> Router {
    // Two routers, not one chain: a layer wraps every route added before it, so a single chain
    // would put the report route behind the superadmin guard too.
    let reporting: Router<AppState> = Router::new()
        .route("/posts/{post_id}/report", post(report_post))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth));
    let moderation: Router<AppState> = Router::new()
        .route("/posts/{post_id}/hide", post(hide_post))
        .route("/admin/reports", get(list_reports))
        .route("/admin/reports/{report_id}", patch(resolve_report))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_superadmin,
        ));
    reporting.merge(moderation).with_state(state)
}

#[derive(Deserialize)]
struct ReportRequest {
    reason: String,
}

async fn report_post(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(post_id): Path<Uuid>,
    Json(input): Json<ReportRequest>,
) -> Result<Json<crate::db::reports::Report>, StatusError> {
    let reason_chars = input.reason.chars().count();
    if !(1..=MAX_REASON_CHARS).contains(&reason_chars) {
        return Err(StatusError::with_status(
            StatusCode::BAD_REQUEST,
            "reason must be 1 to 1000 characters",
        ));
    }
    let report =
        crate::db::reports::create_report(&state.pool, auth.user_id, post_id, &input.reason)
            .await?;
    Ok(Json(report))
}

async fn list_reports(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::reports::Report>>, StatusError> {
    let reports = crate::db::reports::list_reports(&state.pool).await?;
    Ok(Json(reports))
}

#[derive(Deserialize)]
struct ResolveRequest {
    status: String,
    admin_notes: Option<String>,
}

async fn resolve_report(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(report_id): Path<Uuid>,
    Json(input): Json<ResolveRequest>,
) -> Result<Json<serde_json::Value>, StatusError> {
    if input.status != "resolved" && input.status != "dismissed" {
        return Err(StatusError::with_status(
            StatusCode::BAD_REQUEST,
            "status must be 'resolved' or 'dismissed'",
        ));
    }

    crate::db::reports::resolve_report(
        &state.pool,
        report_id,
        &input.status,
        input.admin_notes.as_deref(),
        auth.user_id,
    )
    .await?;

    Ok(Json(serde_json::json!({"status": input.status})))
}

async fn hide_post(
    State(state): State<AppState>,
    Path(post_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusError> {
    crate::db::reports::hide_post(&state.pool, post_id).await?;
    Ok(Json(serde_json::json!({"status": "hidden"})))
}
