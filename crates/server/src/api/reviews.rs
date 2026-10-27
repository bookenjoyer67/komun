//! Reviews: writing is a participant's act on a deal and carries `require_auth`; reading a
//! profile's reviews is public and merged into the `/users` nest beside endorsements.

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    middleware,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::{require_auth, AuthUser};
use crate::db::conversations::DealStep;
use crate::db::posts::{DEFAULT_LIMIT, MAX_LIMIT};
use crate::db::reviews::{ReviewRow, ReviewView};
use crate::AppState;

use super::categories::bad_request;
use super::StatusError;

/// A review is a sentence or two, bounded because this text is server-readable and permanent,
/// unlike the encrypted thread it describes.
pub(crate) const MAX_BODY_CHARS: usize = 2000;

/// The star range `chk_deal_reviews_rating` enforces, checked here so a bad rating is a 400 naming
/// the field rather than a 500.
pub(crate) const MIN_RATING: i64 = 1;
pub(crate) const MAX_RATING: i64 = 5;

/// Writing a review: session required, and the thread decides the rest.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/matches/{match_id}/reviews", post(create_review))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .with_state(state)
}

pub fn user_router(state: AppState) -> Router {
    Router::new()
        .route("/{id}/reviews", get(list_reviews))
        .with_state(state)
}

/// `rating` is a `serde_json::Value` rather than an `i16` so `4.5` and `"5"` get a 400 that says
/// what a rating is, instead of serde's 422 naming a Rust type.
#[derive(Deserialize, Default)]
pub(crate) struct ReviewRequest {
    pub(crate) rating: Option<serde_json::Value>,
    pub(crate) body: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ValidReview {
    pub(crate) rating: i16,
    pub(crate) body: Option<String>,
}

async fn create_review(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(match_id): Path<Uuid>,
    Json(input): Json<ReviewRequest>,
) -> Result<(StatusCode, Json<ReviewRow>), StatusError> {
    // The 404/403 pair and the reviewee come from one load; the reviewee is never a client field,
    // or a completed deal would be a licence to rate a stranger.
    let thread = crate::db::conversations::load_thread(&state.pool, match_id)
        .await?
        .ok_or_else(|| StatusError::with_status(StatusCode::NOT_FOUND, "conversation not found"))?;

    let reviewee_id = thread
        .other_participant(auth.user_id)
        .ok_or_else(|| StatusError::with_status(StatusCode::FORBIDDEN, "not a participant"))?;

    let review = validate_review(&input).map_err(bad_request)?;

    // Whether the deal is completed is decided inside the locked transaction; asking here too
    // would be a second, possibly stale, answer.
    match crate::db::reviews::create(
        &state.pool,
        match_id,
        auth.user_id,
        reviewee_id,
        review.rating,
        review.body.as_deref(),
    )
    .await?
    {
        DealStep::Done(row) => Ok((StatusCode::CREATED, Json(row))),
        DealStep::Conflict(why) => Err(StatusError::with_status(StatusCode::CONFLICT, why)),
    }
}

/// Strings, like `PostFilters`, so `?limit=all` is a 400 naming the parameter rather than a 422
/// naming `i64`.
#[derive(Deserialize, Default)]
pub(crate) struct ReviewPage {
    pub(crate) limit: Option<String>,
    pub(crate) offset: Option<String>,
}

async fn list_reviews(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Query(page): Query<ReviewPage>,
) -> Result<Json<Vec<ReviewView>>, StatusError> {
    // An unknown id is a 404 for the profile, so it must be one here too; otherwise an empty list
    // and a missing user look the same.
    if !crate::db::users::exists(&state.pool, user_id).await? {
        return Err(StatusError::with_status(
            StatusCode::NOT_FOUND,
            "user not found",
        ));
    }
    let (limit, offset) = validate_page(&page).map_err(bad_request)?;
    let reviews = crate::db::reviews::list_for_user(&state.pool, user_id, limit, offset).await?;
    Ok(Json(reviews))
}

/// Pure checks on a review body needing neither the thread nor the database; `pub(crate)` so
/// `tests::market` can pin every branch.
pub(crate) fn validate_review(raw: &ReviewRequest) -> Result<ValidReview, String> {
    let rating = match raw.rating.as_ref() {
        None | Some(serde_json::Value::Null) => {
            return Err(format!(
                "rating is required and must be a whole number from {MIN_RATING} to {MAX_RATING}"
            ))
        }
        Some(value) => match value.as_i64() {
            Some(stars) if (MIN_RATING..=MAX_RATING).contains(&stars) => stars as i16,
            // A non-integer in range, or an integer out of it, both get the range back — the fact
            // the client is missing either way.
            Some(stars) => {
                return Err(format!(
                    "rating must be between {MIN_RATING} and {MAX_RATING} (got {stars})"
                ))
            }
            None => {
                return Err(format!(
                    "rating must be a whole number from {MIN_RATING} to {MAX_RATING} (got {value})"
                ))
            }
        },
    };

    let body = match trimmed(raw.body.as_deref()) {
        None => None,
        Some(body) => {
            // Characters, not bytes: a non-Latin review would otherwise be cut to a third of the
            // promised length.
            let length = body.chars().count();
            if length > MAX_BODY_CHARS {
                return Err(format!(
                    "body must be {MAX_BODY_CHARS} characters or fewer (got {length})"
                ));
            }
            Some(body.to_string())
        }
    };

    Ok(ValidReview { rating, body })
}

/// The same limit/offset convention as `GET /api/posts`, rejected rather than clamped so a caller
/// is not quietly handed 200.
pub(crate) fn validate_page(raw: &ReviewPage) -> Result<(i64, i64), String> {
    let limit = bounded("limit", raw.limit.as_deref(), DEFAULT_LIMIT, 1, MAX_LIMIT)?;
    let offset = bounded("offset", raw.offset.as_deref(), 0, 0, i64::MAX)?;
    Ok((limit, offset))
}

fn bounded(name: &str, raw: Option<&str>, default: i64, min: i64, max: i64) -> Result<i64, String> {
    let Some(value) = trimmed(raw) else {
        return Ok(default);
    };

    let parsed: i64 = value
        .parse()
        .map_err(|_| format!("{name} must be a whole number (got {value:?})"))?;
    if parsed < min || parsed > max {
        return Err(format!(
            "{name} must be between {min} and {max} (got {parsed})"
        ));
    }
    Ok(parsed)
}

/// An absent field and a blank one mean the same thing: not supplied.
fn trimmed(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|value| !value.is_empty())
}
