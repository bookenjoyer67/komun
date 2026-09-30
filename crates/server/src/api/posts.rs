//! `/api/posts` — the flat post collection.
//!
//! A3.1: posts used to hang off a per-tenant path segment, and every handler began by resolving
//! that segment to a row in a table the squashed schema no longer has. The path segment, the
//! lookup and the membership/role checks that gated posting are all gone. `require_auth` is the
//! only gate on the mutating half of this router.

use axum::{
    extract::{Extension, Multipart, Path, Query, State},
    http::StatusCode,
    middleware,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::{require_auth, AuthUser};
use crate::config::is_currency_code;
use crate::db::posts::{PostFilter, DEFAULT_LIMIT, MAX_LIMIT};
use crate::AppState;
use komun_core::models::{CreatePost, ItemCondition, Post, PostKind, PostStatus, Urgency};

use super::categories::{bad_request, validate_slug};
use super::StatusError;

pub fn router(state: AppState) -> Router {
    let public = Router::new()
        .route("/", get(list_posts))
        .route("/{id}", get(get_post));

    let protected = Router::new()
        .route("/", axum::routing::post(create_post))
        .route(
            "/{id}",
            axum::routing::patch(update_post).delete(withdraw_post),
        )
        .route("/{id}/images", axum::routing::post(upload_images))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth));

    public.merge(protected).with_state(state)
}

/// The raw query string, every field a `String`.
///
/// M1.4: typing these (`Option<i64>`, `Option<ItemCondition>`) would hand the rejection to axum's
/// extractor, which answers **422** with a serde message naming a Rust field. Worse, the values
/// serde *can* parse but the database has never heard of — `?currency=dollars`,
/// `?item_condition=mint` — would filter to nothing and answer `200 []`, which reads as "this
/// marketplace is empty" rather than "you asked wrong". Parsing by hand is what buys a 400 that
/// names the parameter.
#[derive(Deserialize, Default)]
pub(crate) struct PostFilters {
    pub(crate) kind: Option<String>,
    pub(crate) category: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) q: Option<String>,
    pub(crate) min_price_cents: Option<String>,
    pub(crate) max_price_cents: Option<String>,
    pub(crate) currency: Option<String>,
    pub(crate) item_condition: Option<String>,
    pub(crate) limit: Option<String>,
    pub(crate) offset: Option<String>,
}

async fn list_posts(
    State(state): State<AppState>,
    Query(filters): Query<PostFilters>,
) -> Result<Json<Vec<Post>>, StatusError> {
    let filter = validate_filters(&filters).map_err(bad_request)?;
    let posts = crate::db::posts::list(&state.pool, &filter).await?;
    Ok(Json(posts))
}

/// Turn a raw query string into a [`PostFilter`], or into the message of a 400.
///
/// Pure and `pub(crate)` so `tests::market` can pin every branch without a database.
pub(crate) fn validate_filters(raw: &PostFilters) -> Result<PostFilter, String> {
    let kind = enum_filter(
        "kind",
        raw.kind.as_deref(),
        PostKind::parse,
        PostKind::ALL,
        PostKind::as_str,
    )?;
    let status = enum_filter(
        "status",
        raw.status.as_deref(),
        PostStatus::parse,
        PostStatus::ALL,
        PostStatus::as_str,
    )?;
    let item_condition = enum_filter(
        "item_condition",
        raw.item_condition.as_deref(),
        ItemCondition::parse,
        ItemCondition::ALL,
        ItemCondition::as_str,
    )?;

    let min_price_cents = price_filter("min_price_cents", raw.min_price_cents.as_deref())?;
    let max_price_cents = price_filter("max_price_cents", raw.max_price_cents.as_deref())?;
    if let (Some(min), Some(max)) = (min_price_cents, max_price_cents) {
        if min > max {
            // An inverted range can only ever match nothing, so answering `[]` would be a true
            // but useless reply to what is plainly a mistake.
            return Err(format!(
                "min_price_cents ({min}) is greater than max_price_cents ({max})"
            ));
        }
    }

    let currency = match trimmed(raw.currency.as_deref()) {
        None => None,
        Some(value) if is_currency_code(value) => Some(value.to_string()),
        Some(value) => {
            return Err(format!(
                "currency must be a three-letter uppercase ISO-4217 code (got {value:?})"
            ))
        }
    };

    // Only the shape, not the existence: a well-formed slug nobody has created is a legitimately
    // empty result, whereas `?category=Electronics!` can never match anything at all.
    let category = match trimmed(raw.category.as_deref()) {
        None => None,
        Some(value) => {
            validate_slug(value).map_err(|why| format!("category is not a valid slug: {why}"))?;
            Some(value.to_string())
        }
    };

    Ok(PostFilter {
        kind,
        category,
        status,
        q: trimmed(raw.q.as_deref()).map(str::to_string),
        min_price_cents,
        max_price_cents,
        currency,
        item_condition,
        limit: bounded("limit", raw.limit.as_deref(), DEFAULT_LIMIT, 1, MAX_LIMIT)?,
        offset: bounded("offset", raw.offset.as_deref(), 0, 0, i64::MAX)?,
    })
}

/// An absent parameter and an empty one mean the same thing: no filter. `?kind=` comes from a
/// form field the user left alone, and rejecting it would break every such form.
fn trimmed(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|value| !value.is_empty())
}

/// A filter whose value must be one of a DB enum's values. The error lists what is accepted,
/// rendered from the enum itself so it cannot fall behind the `CHECK` the enum is pinned to.
fn enum_filter<T: Copy>(
    name: &str,
    raw: Option<&str>,
    parse: fn(&str) -> Option<T>,
    all: &[T],
    as_str: fn(&T) -> &'static str,
) -> Result<Option<T>, String> {
    let Some(value) = trimmed(raw) else {
        return Ok(None);
    };

    match parse(value) {
        Some(parsed) => Ok(Some(parsed)),
        None => Err(format!(
            "{name} must be one of {} (got {value:?})",
            all.iter().map(as_str).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// Prices are whole cents. Negative is rejected here as well as by `chk_posts_price_cents`,
/// because a negative bound is a client mistake worth naming rather than a range that matches
/// every priced post.
fn price_filter(name: &str, raw: Option<&str>) -> Result<Option<i64>, String> {
    let Some(value) = trimmed(raw) else {
        return Ok(None);
    };

    let cents: i64 = value
        .parse()
        .map_err(|_| format!("{name} must be a whole number of cents (got {value:?})"))?;
    if cents < 0 {
        return Err(format!("{name} cannot be negative (got {cents})"));
    }
    Ok(Some(cents))
}

/// Rejected rather than clamped: a caller that asks for 5,000 posts and is handed 200 without
/// being told has no way to know its pagination is wrong.
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

async fn get_post(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Post>, StatusError> {
    let post = load_post(&state, id).await?;
    Ok(Json(post))
}

async fn create_post(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Json(mut input): Json<CreatePost>,
) -> Result<Json<Post>, StatusError> {
    let recent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM posts WHERE author_id = $1 AND created_at > now() - interval '1 hour'"
    )
    .bind(auth.user_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    if recent >= state.config.security.max_posts_per_hour as i64 {
        return Err(StatusError::with_status(
            StatusCode::TOO_MANY_REQUESTS,
            format!(
                "rate limit: max {} posts per hour",
                state.config.security.max_posts_per_hour
            ),
        ));
    }

    validate_market_fields(&input)?;

    // M1.1 / SPEC B7. Validation first, so a caller's bad currency is still their 400 and not
    // quietly replaced by the server's default; the configured value needs no check of its own
    // because `Config::validate_market` refused to start if it was malformed.
    if input.kind.is_market() {
        let resolved = state
            .config
            .market
            .resolve_currency(input.currency.as_deref());
        input.currency = resolved;
    }

    let post = crate::db::posts::create(&state.pool, auth.user_id, input).await?;
    Ok(Json(post))
}

/// The `chk_posts_market_fields` and `chk_posts_currency` constraints are the real authority on
/// this; checking here turns a constraint violation (a 500, with the SQL in the log) into a 400
/// that says what is wrong.
fn validate_market_fields(input: &CreatePost) -> Result<(), StatusError> {
    if !input.kind.is_market()
        && (input.market_listed || input.price_cents.is_some() || input.item_condition.is_some())
    {
        return Err(bad_request(
            "price, condition and market_listed belong to 'listing' and 'want' posts only",
        ));
    }
    if input.price_cents.is_some_and(|c| c < 0) {
        return Err(bad_request("price_cents cannot be negative"));
    }
    if let Some(currency) = &input.currency {
        if !is_currency_code(currency) {
            return Err(bad_request(
                "currency must be a three-letter uppercase ISO-4217 code",
            ));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct UpdatePostRequest {
    title: Option<String>,
    body: Option<String>,
    /// Typed, so an unknown value is a 422 from serde rather than a CHECK violation at the
    /// bottom of the stack.
    urgency: Option<Urgency>,
    status: Option<PostStatus>,
}

async fn update_post(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdatePostRequest>,
) -> Result<Json<serde_json::Value>, StatusError> {
    let post = load_post(&state, id).await?;
    if post.author_id != auth.user_id {
        return Err(StatusError::with_status(
            StatusCode::FORBIDDEN,
            "not your post",
        ));
    }

    // `hidden` and `flagged` are moderation states; an author setting either on their own post
    // would either hide it from moderators' queues or fake a report outcome.
    if matches!(
        input.status,
        Some(PostStatus::Hidden) | Some(PostStatus::Flagged)
    ) {
        return Err(StatusError::with_status(
            StatusCode::FORBIDDEN,
            "that status is set by moderators, not by the author",
        ));
    }

    crate::db::posts::update(
        &state.pool,
        id,
        input.title,
        input.body,
        input.urgency,
        input.status,
    )
    .await?;
    Ok(Json(json!({"status": "updated"})))
}

async fn withdraw_post(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusError> {
    let post = load_post(&state, id).await?;
    if post.author_id != auth.user_id {
        return Err(StatusError::with_status(
            StatusCode::FORBIDDEN,
            "not your post",
        ));
    }
    crate::db::posts::withdraw(&state.pool, id).await?;
    Ok(Json(json!({"status": "withdrawn"})))
}

async fn upload_images(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, StatusError> {
    let post = load_post(&state, id).await?;
    if post.author_id != auth.user_id {
        return Err(StatusError::with_status(
            StatusCode::FORBIDDEN,
            "not your post",
        ));
    }

    let current_count: i64 =
        sqlx::query_scalar("SELECT COALESCE(array_length(images, 1), 0) FROM posts WHERE id = $1")
            .bind(id)
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);

    let max = state.config.media.max_post_images as i64;
    let mut filenames: Vec<String> = vec![];

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?
    {
        if current_count + filenames.len() as i64 >= max {
            break;
        }

        let content_type = field.content_type().unwrap_or("").to_string();
        if !matches!(
            content_type.as_str(),
            "image/png" | "image/jpeg" | "image/webp"
        ) {
            continue;
        }

        let data = field.bytes().await.map_err(|e| anyhow::anyhow!("{}", e))?;
        if data.len() > state.config.media.max_post_image_bytes as usize {
            continue;
        }

        let img = image::load_from_memory(&data)
            .map_err(|_| StatusError::with_status(StatusCode::BAD_REQUEST, "invalid image"))?;
        let img = if img.width() > 1920 || img.height() > 1920 {
            img.resize(1920, 1920, image::imageops::FilterType::Lanczos3)
        } else {
            img
        };

        let img_id = Uuid::now_v7();
        let filename = format!("{}.webp", img_id);
        let dir = std::path::Path::new(&state.config.media.post_images_dir);
        std::fs::create_dir_all(dir).ok();
        let path = dir.join(&filename);
        img.save(&path).map_err(|e| anyhow::anyhow!("{}", e))?;
        filenames.push(filename);
    }

    if filenames.is_empty() {
        return Ok(Json(json!({"images": []})));
    }

    sqlx::query(
        "UPDATE posts SET images = array_cat(COALESCE(images, '{}'), $1::text[]) WHERE id = $2",
    )
    .bind(&filenames)
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    let urls: Vec<String> = filenames
        .iter()
        .map(|f| format!("/post-images/{}", f))
        .collect();

    Ok(Json(json!({"images": urls})))
}

/// A missing post is a 404. The old code let `anyhow!("post not found")` fall through the
/// blanket `From` impl and answered 500.
async fn load_post(state: &AppState, id: Uuid) -> Result<Post, StatusError> {
    crate::db::posts::get(&state.pool, id)
        .await?
        .ok_or_else(|| StatusError::with_status(StatusCode::NOT_FOUND, "post not found"))
}
