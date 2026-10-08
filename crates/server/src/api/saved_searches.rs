//! `/api/me/saved-searches`: a signed-in user's own saved searches. Every route sits behind
//! `require_auth`, and another user's search answers 404, exactly as a missing one does.

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    middleware,
    routing::{delete, get},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::categories::bad_request;
use super::posts::{validate_filters, PostFilters};
use super::StatusError;
use crate::auth::{require_auth, AuthUser};
use crate::db::saved_searches::{Created, NewSavedSearch, SavedSearch};
use crate::AppState;

pub(crate) const MAX_QUERY_CHARS: usize = 200;
pub(crate) const MAX_LABEL_CHARS: usize = 100;

pub(crate) const ALREADY_SAVED: &str = "you have already saved this search";

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/me/saved-searches", get(list_saved).post(save))
        .route("/me/saved-searches/{id}", delete(remove))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .with_state(state)
}

#[derive(Deserialize, Default)]
pub(crate) struct SaveSearchRequest {
    pub(crate) label: Option<String>,
    pub(crate) q: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) category: Option<String>,
    pub(crate) near_lat: Option<f64>,
    pub(crate) near_lon: Option<f64>,
    pub(crate) radius_km: Option<f64>,
}

/// A saved search in the feed's own query shape, so saving it and the digest that later runs it
/// are judged by `validate_filters`, the same rules the live search obeys.
pub(crate) fn feed_filters(
    q: Option<&str>,
    kind: Option<&str>,
    category: Option<&str>,
    near_lat: Option<f64>,
    near_lon: Option<f64>,
    radius_km: Option<f64>,
) -> PostFilters {
    PostFilters {
        q: q.map(str::to_string),
        kind: kind.map(str::to_string),
        category: category.map(str::to_string),
        near_lat: near_lat.map(|v| v.to_string()),
        near_lon: near_lon.map(|v| v.to_string()),
        radius_km: radius_km.map(|v| v.to_string()),
        ..PostFilters::default()
    }
}

/// A search with no words would match every post, so `q` is required. What is stored is the
/// validator's output, not the request, so a saved search is already in the form the feed runs.
pub(crate) fn validate_save(input: &SaveSearchRequest) -> Result<NewSavedSearch, String> {
    let q = input
        .q
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .ok_or("q is required: a saved search needs the words it searches for")?;
    if q.chars().count() > MAX_QUERY_CHARS {
        return Err(format!("q must be at most {MAX_QUERY_CHARS} characters"));
    }

    let label = match input.label.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(label) if label.chars().count() > MAX_LABEL_CHARS => {
            return Err(format!(
                "label must be at most {MAX_LABEL_CHARS} characters"
            ))
        }
        Some(label) => Some(label.to_string()),
    };

    let filter = validate_filters(&feed_filters(
        Some(q),
        input.kind.as_deref(),
        input.category.as_deref(),
        input.near_lat,
        input.near_lon,
        input.radius_km,
    ))?;

    Ok(NewSavedSearch {
        label,
        q: filter.q,
        kind: filter.kind.map(|k| k.as_str().to_string()),
        category: filter.category,
        near: filter.near,
        radius_km: filter.radius_km,
    })
}

async fn list_saved(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<Vec<SavedSearch>>, StatusError> {
    let rows = crate::db::saved_searches::list(&state.pool, auth.user_id).await?;
    Ok(Json(rows))
}

async fn save(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Json(input): Json<SaveSearchRequest>,
) -> Result<(StatusCode, Json<SavedSearch>), StatusError> {
    let search = validate_save(&input).map_err(bad_request)?;
    match crate::db::saved_searches::create(&state.pool, auth.user_id, &search).await? {
        Created::Saved(row) => Ok((StatusCode::CREATED, Json(row))),
        Created::Duplicate => Err(StatusError::with_status(
            StatusCode::CONFLICT,
            ALREADY_SAVED,
        )),
    }
}

async fn remove(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusError> {
    if crate::db::saved_searches::delete(&state.pool, id, auth.user_id).await? {
        Ok(Json(json!({"status": "deleted"})))
    } else {
        Err(StatusError::with_status(
            StatusCode::NOT_FOUND,
            "saved search not found",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::posts::MAX_RADIUS_KM;

    fn request(q: Option<&str>) -> SaveSearchRequest {
        SaveSearchRequest {
            q: q.map(str::to_string),
            ..SaveSearchRequest::default()
        }
    }

    #[test]
    fn a_saved_search_needs_words() {
        for q in [None, Some(""), Some("   ")] {
            let err = validate_save(&request(q)).expect_err("a search with no words is refused");
            assert!(err.starts_with("q "), "the refusal names q: {err}");
        }
    }

    #[test]
    fn an_overlong_query_or_label_is_refused() {
        let longest = "a".repeat(MAX_QUERY_CHARS);
        assert!(validate_save(&request(Some(&longest))).is_ok());
        let too_long = "a".repeat(MAX_QUERY_CHARS + 1);
        assert!(validate_save(&request(Some(&too_long))).is_err());

        let mut labelled = request(Some("wool"));
        labelled.label = Some("l".repeat(MAX_LABEL_CHARS + 1));
        let err = validate_save(&labelled).expect_err("an overlong label is refused");
        assert!(err.starts_with("label "), "{err}");
    }

    #[test]
    fn the_feeds_own_validator_judges_the_filters() {
        let mut half = request(Some("wool"));
        half.near_lat = Some(37.8);
        let err = validate_save(&half).expect_err("half a centre is refused");
        assert!(err.contains("together"), "{err}");

        let mut radius_only = request(Some("wool"));
        radius_only.radius_km = Some(25.0);
        let err = validate_save(&radius_only).expect_err("a radius needs a centre");
        assert!(err.starts_with("radius_km"), "{err}");

        let mut too_wide = request(Some("wool"));
        too_wide.near_lat = Some(0.0);
        too_wide.near_lon = Some(0.0);
        too_wide.radius_km = Some(MAX_RADIUS_KM + 1.0);
        assert!(validate_save(&too_wide).is_err());

        let mut unknown_kind = request(Some("wool"));
        unknown_kind.kind = Some("popular".to_string());
        let err = validate_save(&unknown_kind).expect_err("an unknown kind is refused");
        assert!(err.starts_with("kind"), "{err}");
    }

    #[test]
    fn what_is_saved_is_the_validated_search() {
        let input = SaveSearchRequest {
            label: Some("  Wool near home ".to_string()),
            q: Some("  wool ".to_string()),
            kind: Some("offer".to_string()),
            category: Some("art-craft".to_string()),
            near_lat: Some(37.80443),
            near_lon: Some(-122.27121),
            radius_km: Some(25.0),
        };

        assert_eq!(
            validate_save(&input),
            Ok(NewSavedSearch {
                label: Some("Wool near home".to_string()),
                q: Some("wool".to_string()),
                kind: Some("offer".to_string()),
                category: Some("art-craft".to_string()),
                near: Some((37.8, -122.3)),
                radius_km: Some(25.0),
            })
        );
    }
}
