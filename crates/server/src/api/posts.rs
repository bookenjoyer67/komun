//! `/api/posts` — the flat post collection. `require_auth` is the only gate on the mutating half
//! of this router.

use std::path::PathBuf;

use axum::{
    extract::{
        multipart::{Field, MultipartError},
        DefaultBodyLimit, Extension, Multipart, Path, Query, State,
    },
    http::{header, HeaderMap, StatusCode},
    middleware,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::{require_auth, AuthUser};
use crate::config::is_currency_code;
use crate::db::posts::{FeedPost, FeedSort, PostFilter, DEFAULT_LIMIT, MAX_LIMIT, MAX_RADIUS_KM};
use crate::media;
use crate::AppState;
use komun_core::models::{
    coarsen_coordinate, CreatePost, ItemCondition, Post, PostKind, PostStatus, Urgency,
};

use super::categories::{bad_request, validate_slug};
use super::{count_unavailable, hourly_count, Hourly, StatusError, MULTIPART_OVERHEAD};

const MAX_TITLE_CHARS: usize = 200;
const MAX_BODY_CHARS: usize = 10_000;
const MAX_LOCATION_NAME_CHARS: usize = 200;
const MAX_CONTACT_METHOD_CHARS: usize = 200;
const MAX_TAGS: usize = 10;
const MAX_TAG_CHARS: usize = 32;
const MAX_QUANTITY: i32 = 1_000_000;
const MAX_SEARCH_CHARS: usize = 200;

pub(crate) const NUL_IN_SEARCH_TERM: &str = "q must not contain a NUL character";

/// Stored images are scaled down to fit this square.
const IMAGE_FIT_PX: u32 = 1920;

const NO_ROOM_FOR_IMAGES: &str = "the post already has as many images as it may carry";

pub fn router(state: AppState) -> Router {
    let media_config = &state.config.media;
    let upload_limit = (media_config.max_post_images as usize)
        .saturating_mul(media_config.max_post_image_bytes as usize)
        .saturating_add(MULTIPART_OVERHEAD);

    let public = Router::new()
        .route("/", get(list_posts))
        .route("/{id}", get(get_post));

    let protected = Router::new()
        .route("/", axum::routing::post(create_post))
        .route(
            "/{id}",
            axum::routing::patch(update_post).delete(withdraw_post),
        )
        .route(
            "/{id}/images",
            axum::routing::post(upload_images).layer(DefaultBodyLimit::max(upload_limit)),
        )
        .layer(middleware::from_fn_with_state(state.clone(), require_auth));

    public.merge(protected).with_state(state)
}

/// Every field is a `String`: typing them would let axum answer 422 naming a Rust type, and values
/// serde parses but the database rejects would filter to an empty `200 []`. Parsing by hand buys a
/// 400 that names the parameter.
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
    pub(crate) near_lat: Option<String>,
    pub(crate) near_lon: Option<String>,
    pub(crate) radius_km: Option<String>,
    pub(crate) sort: Option<String>,
    pub(crate) limit: Option<String>,
    pub(crate) offset: Option<String>,
}

async fn list_posts(
    State(state): State<AppState>,
    Query(filters): Query<PostFilters>,
) -> Result<Json<Vec<FeedPost>>, StatusError> {
    let filter = validate_filters(&filters).map_err(bad_request)?;
    let posts = crate::db::posts::list(&state.pool, &filter).await?;
    Ok(Json(
        posts
            .into_iter()
            .map(|mut item| {
                item.post = redact_buyer(item.post, None);
                item
            })
            .collect(),
    ))
}

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
            // An inverted range can only match nothing, so `[]` would be a true but useless reply
            // to a plain mistake.
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

    // Only the slug's shape is checked, not its existence: a well-formed unknown slug legitimately
    // yields nothing.
    let category = match trimmed(raw.category.as_deref()) {
        None => None,
        Some(value) => {
            validate_slug(value).map_err(|why| format!("category is not a valid slug: {why}"))?;
            Some(value.to_string())
        }
    };

    let near = centre_filter(raw.near_lat.as_deref(), raw.near_lon.as_deref())?;
    let radius_km = radius_filter(raw.radius_km.as_deref())?;
    let sort = enum_filter(
        "sort",
        raw.sort.as_deref(),
        FeedSort::parse,
        FeedSort::ALL,
        FeedSort::as_str,
    )?
    .unwrap_or_default();
    // Without a centre a radius would match nothing and a distance order would be the recency
    // order, so either is a mistake worth naming.
    if near.is_none() {
        if radius_km.is_some() {
            return Err("radius_km needs near_lat and near_lon".to_string());
        }
        if sort == FeedSort::Distance {
            return Err("sort=distance needs near_lat and near_lon".to_string());
        }
    }

    let q = trimmed(raw.q.as_deref());
    if let Some(term) = q {
        check_search_term(term)?;
        let length = term.chars().count();
        if length > MAX_SEARCH_CHARS {
            return Err(format!(
                "q must be {MAX_SEARCH_CHARS} characters or fewer (got {length})"
            ));
        }
    }

    Ok(PostFilter {
        kind,
        category,
        status,
        q: q.map(str::to_string),
        min_price_cents,
        max_price_cents,
        currency,
        item_condition,
        near,
        radius_km,
        sort,
        limit: bounded("limit", raw.limit.as_deref(), DEFAULT_LIMIT, 1, MAX_LIMIT)?,
        offset: bounded("offset", raw.offset.as_deref(), 0, 0, i64::MAX)?,
    })
}

/// PostgreSQL refuses U+0000 in a text parameter, so a term carrying one must be refused here as a
/// 400; past this point it is a 500 from the database. `trim` does not remove it.
pub(crate) fn check_search_term(term: &str) -> Result<(), String> {
    if term.contains('\0') {
        return Err(NUL_IN_SEARCH_TERM.to_string());
    }
    Ok(())
}

/// An absent parameter and an empty one both mean no filter; `?kind=` comes from a form field the
/// user left alone.
fn trimmed(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|value| !value.is_empty())
}

/// A filter restricted to a closed set; the refusal lists the set as the type itself renders it,
/// so the message cannot drift from the values the type accepts.
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

/// The range is checked on the value as sent, as `validate_location` does, and the centre is then
/// coarsened here too: a client that sends an exact point gets the same answer as one that
/// coarsened it first.
fn centre_filter(lat: Option<&str>, lon: Option<&str>) -> Result<Option<(f64, f64)>, String> {
    match (trimmed(lat), trimmed(lon)) {
        (None, None) => Ok(None),
        (Some(lat), Some(lon)) => {
            let lat = coordinate_filter("near_lat", lat, 90.0)?;
            let lon = coordinate_filter("near_lon", lon, 180.0)?;
            Ok(Some((coarsen_coordinate(lat), coarsen_coordinate(lon))))
        }
        _ => Err("near_lat and near_lon must be sent together".to_string()),
    }
}

/// `"NaN"` and `"inf"` parse as `f64`, and fall outside the range.
fn coordinate_filter(name: &str, value: &str, limit: f64) -> Result<f64, String> {
    let parsed: f64 = value
        .parse()
        .map_err(|_| format!("{name} must be a number (got {value:?})"))?;
    if !(-limit..=limit).contains(&parsed) {
        return Err(format!(
            "{name} must be a finite number from -{limit} to {limit} (got {value:?})"
        ));
    }
    Ok(parsed)
}

/// Refused rather than clamped, like `bounded`: a clamped radius answers a question the caller did
/// not ask.
fn radius_filter(raw: Option<&str>) -> Result<Option<f64>, String> {
    let Some(value) = trimmed(raw) else {
        return Ok(None);
    };

    let km: f64 = value
        .parse()
        .map_err(|_| format!("radius_km must be a number of kilometres (got {value:?})"))?;
    if !km.is_finite() || km <= 0.0 || km > MAX_RADIUS_KM {
        return Err(format!(
            "radius_km must be greater than 0 and at most {MAX_RADIUS_KM} (got {value:?})"
        ));
    }
    Ok(Some(km))
}

/// Prices are whole cents; a negative bound is rejected as a client mistake rather than matching
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

/// Rejected rather than clamped, so a caller is not silently handed 200.
pub(crate) fn bounded(
    name: &str,
    raw: Option<&str>,
    default: i64,
    min: i64,
    max: i64,
) -> Result<i64, String> {
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
    headers: HeaderMap,
) -> Result<Json<Post>, StatusError> {
    let post = load_post(&state, id).await?;
    let viewer = optional_viewer(&state, &headers).await;
    if !visible_to(&post, viewer.as_ref()) {
        return Err(post_not_found());
    }
    let viewer_id = viewer.map(|v| v.user_id);
    Ok(Json(redact_buyer(post, viewer_id)))
}

/// `db::sessions::lookup` refuses revoked and expired sessions in SQL and reads the role from
/// `users` on every call. Every failure is anonymous rather than a 401, so this can only narrow
/// what a caller sees, never widen it past the middleware. `last_used_at` is left alone: a public
/// read is not session activity.
async fn optional_viewer(state: &AppState, headers: &HeaderMap) -> Option<AuthUser> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = crate::sessions::bearer_token(value)?;
    let hash = crate::sessions::hash_token(token);
    match crate::db::sessions::lookup(&state.pool, &hash).await {
        Ok(Some(session)) => Some(AuthUser {
            user_id: session.user_id,
            session_id: session.session_id,
            role: session.role,
            email_verified: session.email_verified,
        }),
        Ok(None) => None,
        Err(e) => {
            tracing::warn!("optional session lookup failed: {e}");
            None
        }
    }
}

/// A single read shows what the feed shows, plus a post's own author and admins. Anyone else gets
/// the 404 a missing id gets, so a private or moderated post is not confirmed to exist.
pub(crate) fn visible_to(post: &Post, viewer: Option<&AuthUser>) -> bool {
    crate::db::posts::publicly_visible(post.status, post.visibility)
        || viewer.is_some_and(|v| v.user_id == post.author_id || v.is_admin())
}

/// Who bought is between the two parties, so the buyer is shown to the author and the buyer only.
/// The key stays, as null, so the response shape does not change with the caller.
pub(crate) fn redact_buyer(mut post: Post, viewer: Option<Uuid>) -> Post {
    let party = viewer.is_some_and(|v| v == post.author_id || Some(v) == post.buyer_id);
    if !party {
        post.buyer_id = None;
    }
    post
}

async fn create_post(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Json(mut input): Json<CreatePost>,
) -> Result<Json<Post>, StatusError> {
    validate_location(&input)?;
    validate_market_fields(&input)?;
    validate_text_fields(&input)?;

    // Resolve only after validation, so a caller's bad currency is their 400 rather than silently
    // replaced by the server default.
    if input.kind.is_market() {
        let resolved = state
            .config
            .market
            .resolve_currency(input.currency.as_deref());
        input.currency = resolved;
    }

    let mut tx = state.pool.begin().await?;
    let recent = hourly_count(&mut tx, Hourly::Posts, auth.user_id)
        .await
        .map_err(|e| count_unavailable(Hourly::Posts, e))?;
    if recent >= state.config.security.max_posts_per_hour as i64 {
        return Err(StatusError::with_status(
            StatusCode::TOO_MANY_REQUESTS,
            format!(
                "rate limit: max {} posts per hour",
                state.config.security.max_posts_per_hour
            ),
        ));
    }

    let post = crate::db::posts::create(&mut tx, auth.user_id, input).await?;
    tx.commit().await?;
    Ok(Json(post))
}

/// Checks the coordinates as sent, because `db::posts::create` coarsens them and `90.04` coarsens
/// to a legal `90.0`. Refused, never clamped: a clamped value is a location the author did not
/// give. NaN and the infinities fall outside both ranges.
fn validate_location(input: &CreatePost) -> Result<(), StatusError> {
    match (input.location_lat, input.location_lon) {
        (None, None) => Ok(()),
        (Some(lat), Some(lon)) => {
            if !(-90.0..=90.0).contains(&lat) {
                return Err(bad_request(
                    "location_lat must be a finite number from -90 to 90",
                ));
            }
            if !(-180.0..=180.0).contains(&lon) {
                return Err(bad_request(
                    "location_lon must be a finite number from -180 to 180",
                ));
            }
            Ok(())
        }
        _ => Err(bad_request(
            "location_lat and location_lon must be sent together",
        )),
    }
}

/// `chk_posts_market_fields` and `chk_posts_currency` are the authority; checking here turns a 500
/// into a 400.
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

/// Refused, never truncated: a cut field is text the author did not write.
fn validate_text_fields(input: &CreatePost) -> Result<(), StatusError> {
    capped("title", Some(&input.title), MAX_TITLE_CHARS)?;
    capped("body", input.body.as_deref(), MAX_BODY_CHARS)?;
    capped(
        "location_name",
        input.location_name.as_deref(),
        MAX_LOCATION_NAME_CHARS,
    )?;
    capped(
        "contact_method",
        input.contact_method.as_deref(),
        MAX_CONTACT_METHOD_CHARS,
    )?;
    if let Some(tags) = &input.tags {
        if tags.len() > MAX_TAGS {
            return Err(bad_request(format!(
                "tags must number {MAX_TAGS} or fewer (got {})",
                tags.len()
            )));
        }
        for tag in tags {
            capped("each tag", Some(tag), MAX_TAG_CHARS)?;
        }
    }
    if let Some(quantity) = input.quantity {
        if !(0..=MAX_QUANTITY).contains(&quantity) {
            return Err(bad_request(format!(
                "quantity must be from 0 to {MAX_QUANTITY} (got {quantity})"
            )));
        }
    }
    Ok(())
}

/// Characters, not bytes: a non-Latin field would otherwise get a third of the promised room.
fn capped(name: &str, value: Option<&str>, max: usize) -> Result<(), StatusError> {
    let length = value.map_or(0, |v| v.chars().count());
    if length > max {
        return Err(bad_request(format!(
            "{name} must be {max} characters or fewer (got {length})"
        )));
    }
    Ok(())
}

#[derive(Deserialize)]
struct UpdatePostRequest {
    title: Option<String>,
    body: Option<String>,
    /// Typed, so an unknown value is a 422 from serde rather than a CHECK violation.
    urgency: Option<Urgency>,
    status: Option<PostStatus>,
}

pub(crate) const UNDER_MODERATION: &str =
    "this post is under moderation and cannot be changed by its author";

pub(crate) const SET_BY_MODERATORS: &str = "that status is set by moderators, not by the author";

pub(crate) const SOLD_NOT_REOPENED: &str = "a sold listing cannot be made active again";

pub(crate) const CHANGED_DURING_EDIT: &str =
    "this post changed while it was being edited; reload and try again";

/// What an author's update may change. While a post is under moderation every update from its
/// author is refused, and `hidden` and `flagged` are never the author's to set: either would let
/// an author lift or fake a moderation outcome. A sold listing does not go back to `active`,
/// because the sale it records would then sit under a live listing. Every status is named, so a
/// new one does not compile until someone decides whether authors may use it.
pub(crate) fn check_author_update(
    current: PostStatus,
    requested: Option<PostStatus>,
    sold: bool,
) -> Result<(), (StatusCode, &'static str)> {
    match current {
        PostStatus::Hidden | PostStatus::Flagged => {
            return Err((StatusCode::FORBIDDEN, UNDER_MODERATION))
        }
        PostStatus::Active
        | PostStatus::Matched
        | PostStatus::Fulfilled
        | PostStatus::Expired
        | PostStatus::Withdrawn => {}
    }

    match requested {
        None => Ok(()),
        Some(PostStatus::Hidden | PostStatus::Flagged) => {
            Err((StatusCode::FORBIDDEN, SET_BY_MODERATORS))
        }
        Some(PostStatus::Active) if sold => Err((StatusCode::CONFLICT, SOLD_NOT_REOPENED)),
        Some(
            PostStatus::Active
            | PostStatus::Matched
            | PostStatus::Fulfilled
            | PostStatus::Expired
            | PostStatus::Withdrawn,
        ) => Ok(()),
    }
}

async fn update_post(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdatePostRequest>,
) -> Result<Json<serde_json::Value>, StatusError> {
    capped("title", input.title.as_deref(), MAX_TITLE_CHARS)?;
    capped("body", input.body.as_deref(), MAX_BODY_CHARS)?;

    let post = load_post(&state, id).await?;
    if post.author_id != auth.user_id {
        return Err(StatusError::with_status(
            StatusCode::FORBIDDEN,
            "not your post",
        ));
    }

    check_author_update(post.status, input.status, post.sold_at.is_some())
        .map_err(|(status, why)| StatusError::with_status(status, why))?;

    let written = crate::db::posts::update(
        &state.pool,
        id,
        input.title,
        input.body,
        input.urgency,
        input.status,
    )
    .await?;
    if written == 0 {
        return Err(StatusError::with_status(
            StatusCode::CONFLICT,
            CHANGED_DURING_EDIT,
        ));
    }
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

/// A part that is not a supported image, is over the size cap, or does not fit is skipped and
/// reported; a part that claims a supported type and does not decode as it fails the request.
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

    let max_images = state.config.media.max_post_images as usize;
    let max_bytes = state.config.media.max_post_image_bytes as usize;
    let room = max_images.saturating_sub(post.images.len());
    let dir = PathBuf::from(&state.config.media.post_images_dir);
    let mut written = WrittenFiles::new(dir.clone());
    let mut skipped: Vec<serde_json::Value> = Vec::new();

    while let Some(mut field) = multipart.next_field().await.map_err(multipart_error)? {
        let name = field.name().unwrap_or("").to_string();
        let content_type = field.content_type().unwrap_or("").to_string();
        if media::claimed_format(&content_type).is_none() {
            skipped.push(skip(&name, media::Rejected::UnsupportedType.reason()));
            continue;
        }
        if written.len() >= room {
            skipped.push(skip(&name, NO_ROOM_FOR_IMAGES));
            continue;
        }
        let Some(data) = read_capped(&mut field, max_bytes).await? else {
            skipped.push(skip(&name, &format!("the image is over {max_bytes} bytes")));
            continue;
        };

        let img = media::decode_bounded(data, &content_type, IMAGE_FIT_PX)
            .await
            .map_err(|why| StatusError::with_status(StatusCode::BAD_REQUEST, why.reason()))?;

        let filename = format!("{}.webp", Uuid::now_v7());
        std::fs::create_dir_all(&dir).ok();
        written.push(filename.clone());
        img.save(dir.join(&filename))
            .map_err(|e| anyhow::anyhow!("{}", e))?;
    }

    if written.is_empty() {
        return Ok(Json(json!({"images": [], "skipped": skipped})));
    }

    // The count is checked in the same statement that appends, so concurrent uploads cannot
    // together pass `max_post_images`.
    let attached = sqlx::query_scalar::<_, Uuid>(
        r#"UPDATE posts SET images = array_cat(COALESCE(images, '{}'), $1::text[])
           WHERE id = $2 AND COALESCE(array_length(images, 1), 0) + $3 <= $4
           RETURNING id"#,
    )
    .bind(written.names())
    .bind(id)
    .bind(written.len() as i32)
    .bind(max_images as i32)
    .fetch_optional(&state.pool)
    .await?;
    if attached.is_none() {
        return Err(StatusError::with_status(
            StatusCode::CONFLICT,
            NO_ROOM_FOR_IMAGES,
        ));
    }

    let urls: Vec<String> = written
        .keep()
        .iter()
        .map(|f| format!("/post-images/{}", f))
        .collect();

    Ok(Json(json!({"images": urls, "skipped": skipped})))
}

fn skip(name: &str, reason: &str) -> serde_json::Value {
    json!({"name": name, "reason": reason})
}

fn multipart_error(error: MultipartError) -> StatusError {
    StatusError::with_status(error.status(), error.body_text())
}

/// `None` as soon as the part passes `max`; the rest of it is never buffered.
async fn read_capped(field: &mut Field<'_>, max: usize) -> Result<Option<Vec<u8>>, StatusError> {
    let mut data = Vec::new();
    while let Some(chunk) = field.chunk().await.map_err(multipart_error)? {
        if data.len() + chunk.len() > max {
            return Ok(None);
        }
        data.extend_from_slice(&chunk);
    }
    Ok(Some(data))
}

/// The files this request has written. Dropped without [`WrittenFiles::keep`] it removes them, so
/// an error, a refusal or a dropped connection after the first write leaves no file behind.
struct WrittenFiles {
    dir: PathBuf,
    names: Vec<String>,
}

impl WrittenFiles {
    fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            names: Vec::new(),
        }
    }

    fn push(&mut self, name: String) {
        self.names.push(name);
    }

    fn len(&self) -> usize {
        self.names.len()
    }

    fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    fn names(&self) -> &[String] {
        &self.names
    }

    fn keep(mut self) -> Vec<String> {
        std::mem::take(&mut self.names)
    }
}

impl Drop for WrittenFiles {
    fn drop(&mut self) {
        for name in &self.names {
            if let Err(e) = std::fs::remove_file(self.dir.join(name)) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!("could not remove an unattached post image: {e}");
                }
            }
        }
    }
}

fn post_not_found() -> StatusError {
    StatusError::with_status(StatusCode::NOT_FOUND, "post not found")
}

/// A missing post is a 404, not a 500 from the blanket `From` impl. Unfiltered: the callers that
/// act on the post check authorship themselves, and `get_post` applies `visible_to`.
async fn load_post(state: &AppState, id: Uuid) -> Result<Post, StatusError> {
    crate::db::posts::get(&state.pool, id)
        .await?
        .ok_or_else(post_not_found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use chrono::Utc;
    use komun_core::models::Visibility;

    fn post(status: PostStatus, visibility: Visibility, author_id: Uuid) -> Post {
        let now = Utc::now();
        Post {
            id: Uuid::now_v7(),
            author_id,
            kind: PostKind::Listing,
            category: "electronics".to_string(),
            category_label: None,
            title: "t".to_string(),
            body: None,
            location_name: None,
            location_lat: None,
            location_lon: None,
            urgency: None,
            quantity: None,
            status,
            visibility,
            expires_at: None,
            tags: Vec::new(),
            contact_method: Some("reply in the thread".to_string()),
            images: Vec::new(),
            verified_by: None,
            verified_at: None,
            market_listed: true,
            price_cents: Some(2500),
            currency: Some("EUR".to_string()),
            price_negotiable: false,
            item_condition: None,
            sold_at: None,
            buyer_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn viewer(user_id: Uuid, role: &str) -> AuthUser {
        AuthUser {
            user_id,
            session_id: Uuid::now_v7(),
            role: role.to_string(),
            email_verified: true,
        }
    }

    fn moderated(status: PostStatus) -> bool {
        matches!(status, PostStatus::Hidden | PostStatus::Flagged)
    }

    #[test]
    fn author_update_is_refused_under_moderation_and_on_reopening_a_sale() {
        let requests = std::iter::once(None).chain(PostStatus::ALL.iter().copied().map(Some));
        for current in PostStatus::ALL.iter().copied() {
            for requested in requests.clone() {
                for sold in [false, true] {
                    let want = if moderated(current) {
                        Err((StatusCode::FORBIDDEN, UNDER_MODERATION))
                    } else if requested.is_some_and(moderated) {
                        Err((StatusCode::FORBIDDEN, SET_BY_MODERATORS))
                    } else if sold && requested == Some(PostStatus::Active) {
                        Err((StatusCode::CONFLICT, SOLD_NOT_REOPENED))
                    } else {
                        Ok(())
                    };
                    assert_eq!(
                        check_author_update(current, requested, sold),
                        want,
                        "{current} to {requested:?} (sold: {sold})"
                    );
                }
            }
        }
    }

    #[test]
    fn a_single_read_shows_what_the_feed_shows_plus_author_and_admin() {
        let author = Uuid::now_v7();
        let stranger = viewer(Uuid::now_v7(), "user");
        let own = viewer(author, "user");
        let admin = viewer(Uuid::now_v7(), "admin");
        let superadmin = viewer(Uuid::now_v7(), "superadmin");

        for status in PostStatus::ALL.iter().copied() {
            for visibility in Visibility::ALL.iter().copied() {
                let p = post(status, visibility, author);
                let feed = visibility == Visibility::Public
                    && matches!(
                        status,
                        PostStatus::Active
                            | PostStatus::Matched
                            | PostStatus::Fulfilled
                            | PostStatus::Expired
                    );
                let context = format!("a {visibility} {status} post");
                assert_eq!(visible_to(&p, None), feed, "anonymous, {context}");
                assert_eq!(visible_to(&p, Some(&stranger)), feed, "stranger, {context}");
                for (who, user) in [
                    ("author", &own),
                    ("admin", &admin),
                    ("superadmin", &superadmin),
                ] {
                    assert!(visible_to(&p, Some(user)), "{who}, {context}");
                }
            }
        }
    }

    #[test]
    fn buyer_is_shown_to_the_author_and_the_buyer_only() {
        let author = Uuid::now_v7();
        let buyer = Uuid::now_v7();
        let verifier = Uuid::now_v7();
        let mut sold = post(PostStatus::Fulfilled, Visibility::Public, author);
        sold.buyer_id = Some(buyer);
        sold.verified_by = Some(verifier);

        for party in [author, buyer] {
            assert_eq!(
                redact_buyer(sold.clone(), Some(party)).buyer_id,
                Some(buyer)
            );
        }
        for outsider in [None, Some(Uuid::now_v7())] {
            let shown = redact_buyer(sold.clone(), outsider);
            assert_eq!(shown.buyer_id, None, "viewer {outsider:?}");
            assert_eq!(shown.verified_by, Some(verifier));
            assert_eq!(shown.contact_method, sold.contact_method);
        }

        let unsold = post(PostStatus::Active, Visibility::Public, author);
        assert_eq!(redact_buyer(unsold, Some(author)).buyer_id, None);
    }

    fn located(lat: Option<f64>, lon: Option<f64>) -> CreatePost {
        CreatePost {
            kind: PostKind::Need,
            category: "food".to_string(),
            title: "t".to_string(),
            body: None,
            location_name: None,
            location_lat: lat,
            location_lon: lon,
            urgency: None,
            quantity: None,
            visibility: None,
            expires_at: None,
            tags: None,
            contact_method: None,
            market_listed: false,
            price_cents: None,
            currency: None,
            price_negotiable: false,
            item_condition: None,
        }
    }

    fn refused_with_400(lat: Option<f64>, lon: Option<f64>) {
        let Err(refused) = validate_location(&located(lat, lon)) else {
            panic!("{lat:?},{lon:?} must be refused");
        };
        let status = refused.into_response().status();
        assert_eq!(status, StatusCode::BAD_REQUEST, "{lat:?},{lon:?}");
    }

    #[test]
    fn a_location_on_the_boundaries_is_accepted() {
        for lat in [90.0, -90.0] {
            for lon in [180.0, -180.0] {
                let input = located(Some(lat), Some(lon));
                assert!(validate_location(&input).is_ok(), "{lat},{lon}");
            }
        }
        let inside = located(Some(37.80443), Some(-122.27121));
        assert!(validate_location(&inside).is_ok());
    }

    #[test]
    fn a_post_without_a_location_is_accepted() {
        assert!(validate_location(&located(None, None)).is_ok());
    }

    /// Each of these coarsens to a legal grid value, so a check run after coarsening would pass
    /// them; it must run on the value as sent, and refuse rather than clamp.
    #[test]
    fn an_out_of_range_location_is_refused_even_when_coarsening_would_hide_it() {
        refused_with_400(Some(90.04), Some(0.0));
        refused_with_400(Some(-90.04), Some(0.0));
        refused_with_400(Some(0.0), Some(180.04));
        refused_with_400(Some(0.0), Some(-180.04));
    }

    #[test]
    fn a_non_finite_location_is_refused() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            refused_with_400(Some(bad), Some(0.0));
            refused_with_400(Some(0.0), Some(bad));
        }
    }

    #[test]
    fn half_a_location_is_refused() {
        refused_with_400(Some(37.8), None);
        refused_with_400(None, Some(-122.3));
    }

    fn searching(q: String) -> PostFilters {
        PostFilters {
            q: Some(q),
            ..Default::default()
        }
    }

    #[test]
    fn a_search_term_over_200_characters_is_refused_naming_q() {
        let raw = searching("a".repeat(201));
        let Err(why) = validate_filters(&raw) else {
            panic!("a 201-character q must be refused");
        };
        assert!(why.starts_with("q "), "the error must name q: {why}");
    }

    /// Two-byte characters, so a cap counted in bytes would refuse this term.
    #[test]
    fn a_search_term_of_200_characters_is_accepted() {
        let raw = searching("é".repeat(200));
        let filter = validate_filters(&raw).expect("a 200-character q");
        assert_eq!(filter.q.map(|q| q.chars().count()), Some(200));
    }

    #[test]
    fn a_search_term_with_a_nul_byte_is_refused_naming_q() {
        let raw = searching("a\0b".to_string());
        let Err(why) = validate_filters(&raw) else {
            panic!("a q containing U+0000 must be refused");
        };
        assert!(why.starts_with("q "), "the error must name q: {why:?}");
    }
}
