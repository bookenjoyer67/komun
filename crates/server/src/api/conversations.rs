use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    middleware,
    routing::{get, patch, post},
    Json, Router,
};
use base64::Engine;
use serde::Deserialize;
use uuid::Uuid;

use komun_core::models::{MatchStatus, OfferKind, PostKind};

use super::categories::bad_request;
use super::StatusError;
use crate::auth::{require_auth, AuthUser};
use crate::config::is_currency_code;
use crate::db::conversations::{DealStep, OfferRow, RespondRefusal, RespondStep, Thread};
use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/posts/{post_id}/respond", post(respond_to_post))
        .route("/me/conversations", get(list_conversations))
        .route("/conversations/{match_id}", get(get_conversation))
        .route("/conversations/{match_id}/messages", post(send_message))
        .route("/conversations/{match_id}/status", patch(update_status))
        // The negotiation lives on its thread: an offer has no meaning away from its conversation.
        .route(
            "/conversations/{match_id}/offers",
            post(create_offer).get(list_offers),
        )
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .with_state(state)
}

/// What arrives is a sealed box: the server stores the bytes and never learns what they say, so
/// the only checks are that they decode and are non-empty.
#[derive(Deserialize)]
struct SealedMessage {
    ciphertext: String,
    nonce: Option<String>,
}

impl SealedMessage {
    fn decode(&self) -> Result<(Vec<u8>, Option<Vec<u8>>), StatusError> {
        let bad = |m: &str| StatusError::with_status(StatusCode::BAD_REQUEST, m.to_string());
        let engine = base64::engine::general_purpose::STANDARD;

        let ciphertext = engine
            .decode(&self.ciphertext)
            .map_err(|_| bad("ciphertext is not valid base64"))?;
        if ciphertext.is_empty() {
            return Err(bad("ciphertext is empty"));
        }

        let nonce = match self.nonce.as_deref() {
            None | Some("") => None,
            Some(n) => Some(
                engine
                    .decode(n)
                    .map_err(|_| bad("nonce is not valid base64"))?,
            ),
        };

        Ok((ciphertext, nonce))
    }
}

async fn respond_to_post(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(post_id): Path<Uuid>,
    Json(input): Json<SealedMessage>,
) -> Result<Json<serde_json::Value>, StatusError> {
    let recent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM matches WHERE responder_id = $1 AND created_at > now() - interval '1 hour'"
    )
    .bind(auth.user_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    if recent >= state.config.security.max_matches_per_hour as i64 {
        return Err(StatusError::with_status(
            StatusCode::TOO_MANY_REQUESTS,
            format!(
                "rate limit: max {} responses per hour",
                state.config.security.max_matches_per_hour
            ),
        ));
    }

    let (ciphertext, nonce) = input.decode()?;

    let step = crate::db::conversations::create_match(
        &state.pool,
        post_id,
        auth.user_id,
        &ciphertext,
        nonce.as_deref(),
    )
    .await?;

    let match_id = match step {
        RespondStep::Opened(match_id) => match_id,
        // The same answer as a missing id, so a post the feed hides is not confirmed to exist.
        RespondStep::Refused(RespondRefusal::NotFound) => {
            return Err(StatusError::with_status(
                StatusCode::NOT_FOUND,
                "post not found",
            ))
        }
        RespondStep::Refused(RespondRefusal::Closed(why)) => return Err(conflict(why)),
    };

    Ok(Json(serde_json::json!({
        "match_id": match_id,
        "status": "proposed"
    })))
}

async fn list_conversations(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<Vec<crate::db::conversations::ConversationPreview>>, StatusError> {
    let convos = crate::db::conversations::list_conversations(&state.pool, auth.user_id).await?;
    Ok(Json(convos))
}

async fn get_conversation(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(match_id): Path<Uuid>,
) -> Result<Json<crate::db::conversations::Conversation>, StatusError> {
    // The 404/403 pair, decided the same way on every route on a thread so "may I see this?" does
    // not depend on the endpoint.
    participant_thread(&state, match_id, auth.user_id).await?;

    let convo =
        crate::db::conversations::get_conversation(&state.pool, match_id, auth.user_id).await?;
    Ok(Json(convo))
}

async fn send_message(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(match_id): Path<Uuid>,
    Json(input): Json<SealedMessage>,
) -> Result<Json<crate::db::conversations::MessageRow>, StatusError> {
    let recent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM messages WHERE sender_id = $1 AND created_at > now() - interval '1 hour'"
    )
    .bind(auth.user_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    if recent >= state.config.security.max_messages_per_hour as i64 {
        return Err(StatusError::with_status(
            StatusCode::TOO_MANY_REQUESTS,
            format!(
                "rate limit: max {} messages per hour",
                state.config.security.max_messages_per_hour
            ),
        ));
    }

    let (ciphertext, nonce) = input.decode()?;

    let msg = crate::db::conversations::send_message(
        &state.pool,
        match_id,
        auth.user_id,
        &ciphertext,
        nonce.as_deref(),
    )
    .await?;
    Ok(Json(msg))
}

#[derive(Deserialize)]
struct UpdateStatusRequest {
    status: String,
}

async fn update_status(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(match_id): Path<Uuid>,
    Json(input): Json<UpdateStatusRequest>,
) -> Result<Json<serde_json::Value>, StatusError> {
    // Only the 404/403 pair here; whether the transition is allowed is decided inside the locked
    // transaction, where a second answer could not be stale.
    participant_thread(&state, match_id, auth.user_id).await?;

    let to = parse_status(&input.status).map_err(bad_request)?;

    match crate::db::conversations::update_status(&state.pool, match_id, to).await? {
        DealStep::Done(()) => Ok(Json(serde_json::json!({"status": to.as_str()}))),
        DealStep::Conflict(why) => Err(conflict(why)),
    }
}

/// A note is a server-readable sentence attached to a number, unlike the encrypted messages, so it
/// is length-bounded to stop it becoming a side channel around them.
pub(crate) const MAX_NOTE_CHARS: usize = 500;

/// `kind` is a `String` for the same reason post filters are: a typed field lets serde answer 422
/// with a Rust type name instead of a 400 listing the four steps.
#[derive(Deserialize, Default)]
pub(crate) struct OfferRequest {
    pub(crate) kind: Option<String>,
    pub(crate) amount_cents: Option<i64>,
    pub(crate) currency: Option<String>,
    pub(crate) note: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ValidOffer {
    pub(crate) kind: OfferKind,
    pub(crate) amount_cents: Option<i64>,
    pub(crate) currency: Option<String>,
    pub(crate) note: Option<String>,
}

async fn create_offer(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(match_id): Path<Uuid>,
    Json(input): Json<OfferRequest>,
) -> Result<(StatusCode, Json<OfferRow>), StatusError> {
    let thread = participant_thread(&state, match_id, auth.user_id).await?;
    offers_allowed_on(thread.post_kind).map_err(bad_request)?;

    let offer = validate_offer(&input).map_err(bad_request)?;

    // Only a step that carries a number needs a currency; a decline keeps whatever the client sent.
    let currency = match offer.amount_cents {
        None => offer.currency.clone(),
        Some(_) => Some(
            resolve_offer_currency(
                offer.currency.as_deref(),
                thread.post_currency.as_deref(),
                state.config.market.default_currency.as_deref(),
            )
            .map_err(bad_request)?,
        ),
    };

    let step = match offer.kind {
        // An accept is the agreement, not just a row: it writes the row, the agreed price and the
        // status in one transaction.
        OfferKind::Accept => {
            let amount = offer
                .amount_cents
                .ok_or_else(|| bad_request("amount_cents is required for an 'accept'"))?;
            crate::db::conversations::accept_offer(
                &state.pool,
                match_id,
                auth.user_id,
                amount,
                currency.as_deref(),
                offer.note.as_deref(),
            )
            .await?
        }
        // A decline is recorded, then closes the thread as `withdrawn`.
        OfferKind::Decline => {
            crate::db::conversations::decline_offer(
                &state.pool,
                match_id,
                auth.user_id,
                currency.as_deref(),
                offer.note.as_deref(),
            )
            .await?
        }
        // An offer or counter changes no state of its own, but a thread that is over does not take
        // one.
        OfferKind::Offer | OfferKind::Counter => {
            crate::db::conversations::append_offer(
                &state.pool,
                match_id,
                auth.user_id,
                offer.kind,
                offer.amount_cents,
                currency.as_deref(),
                offer.note.as_deref(),
            )
            .await?
        }
    };

    match step {
        DealStep::Done(row) => Ok((StatusCode::CREATED, Json(row))),
        DealStep::Conflict(why) => Err(conflict(why)),
    }
}

async fn list_offers(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(match_id): Path<Uuid>,
) -> Result<Json<Vec<OfferRow>>, StatusError> {
    participant_thread(&state, match_id, auth.user_id).await?;
    let offers = crate::db::conversations::list_offers(&state.pool, match_id).await?;
    Ok(Json(offers))
}

/// The 404/403 pair every route on a thread starts with: an id nobody has is not found, and an id
/// that exists but is not yours is forbidden.
async fn participant_thread(
    state: &AppState,
    match_id: Uuid,
    user_id: Uuid,
) -> Result<Thread, StatusError> {
    let thread = crate::db::conversations::load_thread(&state.pool, match_id)
        .await?
        .ok_or_else(|| StatusError::with_status(StatusCode::NOT_FOUND, "conversation not found"))?;

    if !thread.is_participant(user_id) {
        return Err(StatusError::with_status(
            StatusCode::FORBIDDEN,
            "not a participant",
        ));
    }

    Ok(thread)
}

fn conflict(message: impl std::fmt::Display) -> StatusError {
    StatusError::with_status(StatusCode::CONFLICT, message)
}

/// Offers are for listings and wanted ads; an aid thread keeps its plain propose/accept flow, and
/// putting a price on one would be the start of charging for aid.
pub(crate) fn offers_allowed_on(post_kind: PostKind) -> Result<(), String> {
    if post_kind.is_market() {
        Ok(())
    } else {
        Err("offers are for listings and wanted ads".to_string())
    }
}

/// Pure checks on an offer body needing neither the thread nor the config; `pub(crate)` so
/// `tests::market` can pin every branch.
pub(crate) fn validate_offer(raw: &OfferRequest) -> Result<ValidOffer, String> {
    let kind = match trimmed(raw.kind.as_deref()) {
        None => {
            return Err(format!(
                "kind is required and must be one of {}",
                offer_kinds()
            ))
        }
        Some(value) => OfferKind::parse(value)
            .ok_or_else(|| format!("kind must be one of {} (got {value:?})", offer_kinds()))?,
    };

    let amount_cents = match (kind, raw.amount_cents) {
        // A decline carries no amount: the number it refuses is already on the thread, and a copy
        // would read as a counter.
        (OfferKind::Decline, Some(cents)) => {
            return Err(format!("a 'decline' carries no amount_cents (got {cents})"))
        }
        (OfferKind::Decline, None) => None,
        (_, None) => return Err(format!("amount_cents is required for '{kind}'")),
        (_, Some(cents)) if cents < 0 => {
            return Err(format!("amount_cents cannot be negative (got {cents})"))
        }
        (_, Some(cents)) => Some(cents),
    };

    let currency = match trimmed(raw.currency.as_deref()) {
        None => None,
        Some(code) if is_currency_code(code) => Some(code.to_string()),
        Some(code) => {
            return Err(format!(
                "currency must be a three-letter uppercase ISO-4217 code (got {code:?})"
            ))
        }
    };

    let note = match trimmed(raw.note.as_deref()) {
        None => None,
        Some(note) => {
            // Characters, not bytes: a non-Latin note would otherwise be cut to a third of the
            // promised length.
            let length = note.chars().count();
            if length > MAX_NOTE_CHARS {
                return Err(format!(
                    "note must be {MAX_NOTE_CHARS} characters or fewer (got {length})"
                ));
            }
            Some(note.to_string())
        }
    };

    Ok(ValidOffer {
        kind,
        amount_cents,
        currency,
        note,
    })
}

/// Currency precedence: the offer's own, else the post's, else `[market] default_currency`; there
/// is no fourth step because it would price a deal in a unit neither party named.
pub(crate) fn resolve_offer_currency(
    offered: Option<&str>,
    post: Option<&str>,
    server_default: Option<&str>,
) -> Result<String, String> {
    offered
        .or(post)
        .or(server_default)
        .map(str::to_string)
        .ok_or_else(|| {
            "currency is required: this offer carries none, the post has none, and \
             [market] default_currency is unset"
                .to_string()
        })
}

/// The four values `chk_matches_status` enforces, rendered from the enum so the list cannot fall
/// behind the CHECK.
pub(crate) fn parse_status(raw: &str) -> Result<MatchStatus, String> {
    let Some(value) = trimmed(Some(raw)) else {
        return Err(format!(
            "status is required and must be one of {}",
            match_statuses()
        ));
    };
    MatchStatus::parse(value)
        .ok_or_else(|| format!("status must be one of {} (got {value:?})", match_statuses()))
}

pub(crate) fn offer_kinds() -> String {
    join(OfferKind::ALL.iter().map(|k| k.as_str()))
}

pub(crate) fn match_statuses() -> String {
    join(MatchStatus::ALL.iter().map(|s| s.as_str()))
}

fn join<'a>(values: impl Iterator<Item = &'a str>) -> String {
    values.collect::<Vec<_>>().join(", ")
}

/// An absent field and a blank one mean the same thing: not supplied.
fn trimmed(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|value| !value.is_empty())
}
