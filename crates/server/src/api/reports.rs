use axum::{
    extract::{Extension, Path, State},
    middleware,
    routing::{get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use uuid::Uuid;

use super::StatusError;
use crate::auth::{require_auth, require_superadmin, AuthUser};
use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/posts/{post_id}/report", post(report_post))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .route("/posts/{post_id}/hide", post(hide_post))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_superadmin,
        ))
        .route("/admin/reports", get(list_reports))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_superadmin,
        ))
        .route("/admin/reports/{report_id}", patch(resolve_report))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_superadmin,
        ))
        .with_state(state.clone())
        .merge(appeal_routes(state.clone()))
        .merge(appeal_admin_routes(state))
}

/// Separate routers, merged: a `layer` wraps every route added before it on the same router, so an
/// author route appended to the chain above would sit behind `require_superadmin`.
fn appeal_routes(state: AppState) -> Router {
    Router::new()
        .route("/posts/{post_id}/appeal", post(file_appeal))
        .route("/me/moderation", get(my_moderation))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .with_state(state)
}

fn appeal_admin_routes(state: AppState) -> Router {
    Router::new()
        .route("/admin/appeals", get(list_appeals))
        .route("/admin/appeals/{appeal_id}", patch(resolve_appeal))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_superadmin,
        ))
        .with_state(state)
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
        return Err(anyhow::anyhow!("status must be 'resolved' or 'dismissed'").into());
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

/// `Option` so a body without the field is the same 400 as a blank one, not serde's 422.
#[derive(Deserialize)]
struct HideRequest {
    reason: Option<String>,
}

async fn hide_post(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(post_id): Path<Uuid>,
    Json(input): Json<HideRequest>,
) -> Result<Json<serde_json::Value>, StatusError> {
    let reason = required_text("reason", input.reason.as_deref()).map_err(bad_request)?;
    let hidden = crate::db::reports::hide_post(&state.pool, post_id, auth.user_id, reason);
    let action_id = hidden.await?.ok_or_else(|| not_found("post not found"))?;
    let reply = serde_json::json!({"status": "hidden", "action_id": action_id});
    Ok(Json(reply))
}

#[derive(Deserialize)]
struct AppealRequest {
    body: Option<String>,
}

async fn file_appeal(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(post_id): Path<Uuid>,
    Json(input): Json<AppealRequest>,
) -> Result<(axum::http::StatusCode, Json<crate::db::reports::Appeal>), StatusError> {
    use crate::db::reports::AppealStep;

    let body = required_text("body", input.body.as_deref()).map_err(bad_request)?;
    match crate::db::reports::file_appeal(&state.pool, post_id, auth.user_id, body).await? {
        AppealStep::Done(appeal) => Ok((axum::http::StatusCode::CREATED, Json(appeal))),
        AppealStep::NotFound => Err(not_found("post not found")),
        AppealStep::Conflict(why) => Err(conflict(why)),
    }
}

async fn my_moderation(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<Vec<crate::db::reports::ModerationNotice>>, StatusError> {
    let now = chrono::Utc::now();
    let author = auth.user_id;
    let notices = crate::db::reports::moderation_for_author(&state.pool, author, now).await?;
    Ok(Json(notices))
}

async fn list_appeals(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::reports::AppealView>>, StatusError> {
    let appeals = crate::db::reports::list_appeals(&state.pool).await?;
    Ok(Json(appeals))
}

#[derive(Deserialize)]
struct ResolveAppealRequest {
    status: Option<String>,
    admin_notes: Option<String>,
}

async fn resolve_appeal(
    State(state): State<AppState>,
    Path(appeal_id): Path<Uuid>,
    Json(input): Json<ResolveAppealRequest>,
) -> Result<Json<serde_json::Value>, StatusError> {
    use crate::db::reports::AppealStep;

    let status = input.status.as_deref();
    let notes = input.admin_notes.as_deref();
    let (decision, note) = validate_resolution(status, notes).map_err(bad_request)?;
    match crate::db::reports::resolve_appeal(&state.pool, appeal_id, decision, note).await? {
        AppealStep::Done(()) => Ok(Json(serde_json::json!({"status": decision.as_str()}))),
        AppealStep::NotFound => Err(not_found("appeal not found")),
        AppealStep::Conflict(why) => Err(conflict(why)),
    }
}

/// Absent and whitespace-only both say nothing. For a hide that is the whole guard: a removal
/// whose reason is blank is a removal its author is never told the reason for.
pub(crate) fn required_text<'a>(field: &str, raw: Option<&'a str>) -> Result<&'a str, String> {
    raw.map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("{field} is required and cannot be blank"))
}

/// A denial carries a note because a refusal with no reason is the silence the appeal exists to
/// end; a grant needs none, since the post itself comes back.
pub(crate) fn validate_resolution<'a>(
    status: Option<&str>,
    admin_notes: Option<&'a str>,
) -> Result<(crate::db::reports::AppealDecision, Option<&'a str>), String> {
    use crate::db::reports::AppealDecision;

    let decision = status
        .map(str::trim)
        .and_then(AppealDecision::parse)
        .ok_or_else(|| "status must be 'granted' or 'denied'".to_string())?;
    let note = admin_notes.map(str::trim).filter(|note| !note.is_empty());
    match decision {
        AppealDecision::Denied => Ok((decision, Some(required_text("admin_notes", note)?))),
        AppealDecision::Granted => Ok((decision, note)),
    }
}

fn bad_request(message: String) -> StatusError {
    super::categories::bad_request(message)
}

fn not_found(message: &str) -> StatusError {
    StatusError::with_status(axum::http::StatusCode::NOT_FOUND, message)
}

fn conflict(message: String) -> StatusError {
    StatusError::with_status(axum::http::StatusCode::CONFLICT, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::reports::AppealDecision;

    #[test]
    fn a_hide_reason_that_says_nothing_is_refused() {
        for blank in [None, Some(""), Some("   "), Some("\n\t ")] {
            let refused = required_text("reason", blank).expect_err("a blank reason");
            assert!(refused.starts_with("reason "), "refused: {refused}");
        }
    }

    #[test]
    fn a_hide_reason_that_says_something_is_kept_trimmed() {
        assert_eq!(required_text("reason", Some("spam")), Ok("spam"));
        assert_eq!(
            required_text("reason", Some("  posted three times \n")),
            Ok("posted three times")
        );
    }

    #[test]
    fn a_denial_needs_a_note() {
        for blank in [None, Some(""), Some("  ")] {
            let refused = validate_resolution(Some("denied"), blank).expect_err("a bare denial");
            assert!(refused.starts_with("admin_notes "), "refused: {refused}");
        }
        assert_eq!(
            validate_resolution(Some("denied"), Some(" still listed ")),
            Ok((AppealDecision::Denied, Some("still listed")))
        );
    }

    #[test]
    fn a_grant_needs_no_note() {
        assert_eq!(
            validate_resolution(Some("granted"), None),
            Ok((AppealDecision::Granted, None))
        );
        assert_eq!(
            validate_resolution(Some("granted"), Some("welcome back")),
            Ok((AppealDecision::Granted, Some("welcome back")))
        );
    }

    #[test]
    fn a_decision_other_than_grant_or_deny_is_refused() {
        for other in [None, Some(""), Some("pending"), Some("GRANTED")] {
            let refused = validate_resolution(other, Some("a note")).expect_err("not a decision");
            assert!(refused.starts_with("status "), "refused: {refused}");
        }
    }
}
