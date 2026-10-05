//! `/api/categories` — the taxonomy, served as rows rather than compiled in as an enum.
//!
//! `GET /api/categories` is public and unauthenticated: the aid and market forms both read it at
//! load. The admin routes live here rather than in `api/admin.rs`, which layers
//! `require_superadmin` over everything it holds, and are open to admin or superadmin.

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    middleware,
    routing::{get, patch},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use komun_core::models::{Category, CategoryScope, CreateCategory, UpdateCategory};

use crate::auth::{record_audit, require_admin, AuthUser};
use crate::AppState;

use super::StatusError;

/// Audit actions namespaced so one prefix filter finds every administrative act.
pub(crate) const AUDIT_CREATE: &str = "admin.category_create";
pub(crate) const AUDIT_UPDATE: &str = "admin.category_update";

pub fn router(state: AppState) -> Router {
    let public = Router::new().route("/categories", get(list_categories));

    let admin = Router::new()
        .route(
            "/admin/categories",
            get(admin_list_categories).post(create_category),
        )
        .route("/admin/categories/{slug}", patch(update_category))
        .layer(middleware::from_fn_with_state(state.clone(), require_admin));

    public.merge(admin).with_state(state)
}

#[derive(Deserialize)]
struct ScopeQuery {
    scope: Option<String>,
}

/// `scope` is a `String`, not a `CategoryScope`: typing it would let axum's `Json` extractor answer
/// 422 in serde's vocabulary, while `GET /api/categories?scope=nope` answers a 400 naming the
/// accepted values, and one endpoint family cannot hold two opinions about a bad scope.
#[derive(Deserialize)]
pub(crate) struct CreateCategoryBody {
    slug: String,
    label: String,
    scope: String,
    sort_order: Option<i32>,
}

impl CreateCategoryBody {
    fn into_input(self) -> Result<CreateCategory, String> {
        Ok(CreateCategory {
            scope: parse_body_scope(&self.scope)?,
            slug: self.slug,
            label: self.label,
            sort_order: self.sort_order,
        })
    }
}

/// `scope` is a `String` for the same reason [`CreateCategoryBody`]'s is. An absent `scope` and an
/// explicit `"scope": null` both mean "leave the scope alone".
#[derive(Deserialize)]
struct UpdateCategoryBody {
    label: Option<String>,
    scope: Option<String>,
    sort_order: Option<i32>,
    active: Option<bool>,
}

impl UpdateCategoryBody {
    fn into_input(self) -> Result<UpdateCategory, String> {
        Ok(UpdateCategory {
            scope: self.scope.as_deref().map(parse_body_scope).transpose()?,
            label: self.label,
            sort_order: self.sort_order,
            active: self.active,
        })
    }
}

/// What a caller without a session sees: the three fields a form needs. `sort_order` is dropped
/// because rows already arrive in that order, and `active` because an inactive row never reaches
/// this serializer.
#[derive(Serialize)]
struct CategoryView {
    slug: String,
    label: String,
    scope: CategoryScope,
}

impl From<&Category> for CategoryView {
    fn from(c: &Category) -> Self {
        CategoryView {
            slug: c.slug.clone(),
            label: c.label.clone(),
            scope: c.scope,
        }
    }
}

/// Public; active rows only.
async fn list_categories(
    State(state): State<AppState>,
    Query(query): Query<ScopeQuery>,
) -> Result<Json<Vec<CategoryView>>, StatusError> {
    let scope = parse_scope(query.scope.as_deref()).map_err(bad_request)?;
    let rows = crate::db::categories::list(&state.pool, scope, false).await?;
    Ok(Json(rows.iter().map(CategoryView::from).collect()))
}

/// Admin view: retired categories included.
async fn admin_list_categories(
    State(state): State<AppState>,
    Query(query): Query<ScopeQuery>,
) -> Result<Json<Vec<Category>>, StatusError> {
    let scope = parse_scope(query.scope.as_deref()).map_err(bad_request)?;
    let rows = crate::db::categories::list(&state.pool, scope, true).await?;
    Ok(Json(rows))
}

/// `pub(crate)` so `crate::tests::market` can mount the handler and pin the status a bad `scope`
/// produces.
pub(crate) async fn create_category(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Json(body): Json<CreateCategoryBody>,
) -> Result<(StatusCode, Json<Category>), StatusError> {
    // Scope is checked before slug and label so a bad scope is reported first.
    let input = body.into_input().map_err(bad_request)?;

    validate_slug(&input.slug).map_err(bad_request)?;
    validate_label(&input.label).map_err(bad_request)?;

    let created = crate::db::categories::create(&state.pool, &input)
        .await?
        .ok_or_else(|| {
            StatusError::with_status(
                StatusCode::CONFLICT,
                format!("category {:?} already exists", input.slug),
            )
        })?;

    record_audit(
        &state.pool,
        Some(auth.user_id),
        AUDIT_CREATE,
        // `subject_id` is a UUID column but a category is keyed by its TEXT slug, so the subject
        // travels in `detail`.
        None,
        audit_detail_create(&created),
    )
    .await;

    Ok((StatusCode::CREATED, Json(created)))
}

async fn update_category(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(slug): Path<String>,
    Json(body): Json<UpdateCategoryBody>,
) -> Result<Json<Category>, StatusError> {
    let input = body.into_input().map_err(bad_request)?;

    if let Some(label) = &input.label {
        validate_label(label).map_err(bad_request)?;
    }
    // An all-empty body would otherwise be a successful no-op whose audit row is indistinguishable
    // from a client bug that dropped its payload.
    if input.label.is_none()
        && input.scope.is_none()
        && input.sort_order.is_none()
        && input.active.is_none()
    {
        return Err(bad_request(
            "nothing to update: send at least one of label, scope, sort_order, active",
        ));
    }

    let before = crate::db::categories::get(&state.pool, &slug)
        .await?
        .ok_or_else(|| not_found(&slug))?;
    let after = crate::db::categories::update(&state.pool, &slug, &input)
        .await?
        .ok_or_else(|| not_found(&slug))?;

    // The FTS trigger indexes the label, so a rename must also refresh the search vectors or posts
    // stay searchable only under the old label.
    let reindexed = if after.label != before.label {
        crate::db::categories::refresh_search_vectors(&state.pool, &slug).await?
    } else {
        0
    };

    record_audit(
        &state.pool,
        Some(auth.user_id),
        AUDIT_UPDATE,
        None,
        audit_detail_update(&slug, &before, &after, reindexed),
    )
    .await;

    Ok(Json(after))
}

/// The `detail` payload for a creation. A function guarantees the slug is on every category audit
/// row, since the `UUID` `subject_id` column cannot carry it.
pub(crate) fn audit_detail_create(created: &Category) -> serde_json::Value {
    serde_json::json!({
        "slug": created.slug,
        "label": created.label,
        "scope": created.scope.as_str(),
        "sort_order": created.sort_order,
        "active": created.active,
    })
}

/// The `detail` payload for an edit: before and after, plus how many posts the relabel reindexed.
pub(crate) fn audit_detail_update(
    slug: &str,
    before: &Category,
    after: &Category,
    posts_reindexed: u64,
) -> serde_json::Value {
    serde_json::json!({
        "slug": slug,
        "from": {
            "label": before.label,
            "scope": before.scope.as_str(),
            "sort_order": before.sort_order,
            "active": before.active,
        },
        "to": {
            "label": after.label,
            "scope": after.scope.as_str(),
            "sort_order": after.sort_order,
            "active": after.active,
        },
        "posts_reindexed": posts_reindexed,
    })
}

/// Parse `?scope=`; `None` means "every active category". An unrecognised value is an error rather
/// than an empty result, which the caller could not tell from a server with no categories.
pub(crate) fn parse_scope(raw: Option<&str>) -> Result<Option<CategoryScope>, String> {
    let Some(value) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };

    parse_body_scope(value).map(Some)
}

/// Parse a `scope` that arrived in a body: unlike `?scope=`, where blank means "no filter", a body
/// that mentions `scope` is naming one, so `""` fails exactly as `"nope"` does.
pub(crate) fn parse_body_scope(raw: &str) -> Result<CategoryScope, String> {
    let value = raw.trim();
    CategoryScope::parse(value)
        .ok_or_else(|| format!("scope must be one of {} (got {value:?})", accepted_scopes()))
}

/// The accepted `?scope=` values, rendered from the enum so the message cannot fall behind
/// `chk_categories_scope`.
pub(crate) fn accepted_scopes() -> String {
    CategoryScope::ALL
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Slugs are lowercase-kebab and immutable after creation: `posts.category` is a foreign key onto
/// this column, and the slug appears in `?category=` filters where spaces or uppercase cause silent
/// mismatches.
pub(crate) fn validate_slug(slug: &str) -> Result<(), String> {
    if slug.is_empty() {
        return Err("slug must not be empty".to_string());
    }
    if slug.len() > 64 {
        return Err(format!("slug {slug:?} is longer than 64 characters"));
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        return Err(format!("slug {slug:?} must not start or end with a dash"));
    }
    if slug.contains("--") {
        return Err(format!("slug {slug:?} must not contain a double dash"));
    }
    if !slug
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(format!(
            "slug {slug:?} must be lowercase-kebab: a-z, 0-9 and single dashes only"
        ));
    }
    Ok(())
}

/// The label is displayed and full-text indexed, so a blank one makes the category unusable.
pub(crate) fn validate_label(label: &str) -> Result<(), String> {
    if label.trim().is_empty() {
        return Err("label must not be empty".to_string());
    }
    if label.chars().count() > 80 {
        return Err("label must be 80 characters or fewer".to_string());
    }
    Ok(())
}

pub(crate) fn bad_request(message: impl std::fmt::Display) -> StatusError {
    StatusError::with_status(StatusCode::BAD_REQUEST, message)
}

fn not_found(slug: &str) -> StatusError {
    StatusError::with_status(StatusCode::NOT_FOUND, format!("no category {slug:?}"))
}
