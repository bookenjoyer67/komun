//! Conversations. The server never sees message text: what crosses this boundary is opaque
//! ciphertext bytes, base64-encoded at the edge so the JSON stays printable.

use anyhow::{anyhow, Result};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use komun_core::models::{MatchStatus, OfferKind, PostKind};

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[derive(Serialize)]
pub struct ConversationPreview {
    pub match_id: Uuid,
    pub post_id: Uuid,
    pub post_title: String,
    pub post_kind: String,
    pub other_party_id: Uuid,
    pub other_party_name: String,
    /// The last message, still sealed — the client holds the key and renders the preview itself.
    pub last_message_ciphertext: Option<String>,
    pub last_message_nonce: Option<String>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct Conversation {
    pub match_id: Uuid,
    pub post_id: Uuid,
    pub post_title: String,
    pub post_kind: String,
    pub responder_id: Uuid,
    pub author_id: Uuid,
    pub responder_name: String,
    pub author_name: String,
    pub status: String,
    pub messages: Vec<MessageRow>,
    pub created_at: DateTime<Utc>,
}

/// What goes over the wire: base64 of the stored bytes, never text.
#[derive(Serialize, Clone)]
pub struct MessageRow {
    pub id: Uuid,
    pub match_id: Uuid,
    pub sender_id: Uuid,
    pub ciphertext: String,
    pub nonce: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct MessageDbRow {
    id: Uuid,
    match_id: Uuid,
    sender_id: Uuid,
    ciphertext: Vec<u8>,
    nonce: Option<Vec<u8>>,
    created_at: DateTime<Utc>,
}

impl From<MessageDbRow> for MessageRow {
    fn from(r: MessageDbRow) -> Self {
        MessageRow {
            id: r.id,
            match_id: r.match_id,
            sender_id: r.sender_id,
            ciphertext: b64(&r.ciphertext),
            nonce: r.nonce.as_deref().map(b64),
            created_at: r.created_at,
        }
    }
}

#[derive(FromRow)]
struct ConversationPreviewRow {
    match_id: Uuid,
    post_id: Uuid,
    post_title: String,
    post_kind: String,
    other_party_id: Uuid,
    other_party_name: String,
    last_message_ciphertext: Option<Vec<u8>>,
    last_message_nonce: Option<Vec<u8>>,
    last_message_at: Option<DateTime<Utc>>,
    status: String,
    created_at: DateTime<Utc>,
}

impl From<ConversationPreviewRow> for ConversationPreview {
    fn from(r: ConversationPreviewRow) -> Self {
        ConversationPreview {
            match_id: r.match_id,
            post_id: r.post_id,
            post_title: r.post_title,
            post_kind: r.post_kind,
            other_party_id: r.other_party_id,
            other_party_name: r.other_party_name,
            last_message_ciphertext: r.last_message_ciphertext.as_deref().map(b64),
            last_message_nonce: r.last_message_nonce.as_deref().map(b64),
            last_message_at: r.last_message_at,
            status: r.status,
            created_at: r.created_at,
        }
    }
}

pub async fn create_match(
    pool: &PgPool,
    post_id: Uuid,
    responder_id: Uuid,
    ciphertext: &[u8],
    nonce: Option<&[u8]>,
) -> Result<(Uuid, Uuid)> {
    let post_author = sqlx::query_scalar::<_, Uuid>("SELECT author_id FROM posts WHERE id = $1")
        .bind(post_id)
        .fetch_optional(pool)
        .await?;

    if let Some(author_id) = post_author {
        if author_id == responder_id {
            return Err(anyhow!("cannot respond to your own post"));
        }
    }

    let mut tx = pool.begin().await?;
    let match_id = Uuid::now_v7();
    let message_id = Uuid::now_v7();
    let now = Utc::now();

    // `matches.message` stays NULL: writing the opening message into that plaintext TEXT column
    // would restore the readable copy this card exists to remove.
    sqlx::query(
        "INSERT INTO matches (id, post_id, responder_id, status, created_at) VALUES ($1, $2, $3, 'proposed', $4)"
    )
    .bind(match_id)
    .bind(post_id)
    .bind(responder_id)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO messages (id, match_id, sender_id, ciphertext, nonce, created_at) VALUES ($1, $2, $3, $4, $5, $6)"
    )
    .bind(message_id)
    .bind(match_id)
    .bind(responder_id)
    .bind(ciphertext)
    .bind(nonce)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    if let Some(author_id) = post_author {
        let responder_name =
            sqlx::query_scalar::<_, String>("SELECT display_name FROM users WHERE id = $1")
                .bind(responder_id)
                .fetch_optional(pool)
                .await?
                .unwrap_or_default();
        let post_title = sqlx::query_scalar::<_, String>("SELECT title FROM posts WHERE id = $1")
            .bind(post_id)
            .fetch_optional(pool)
            .await?
            .unwrap_or_default();
        super::notifications::create(
            pool,
            author_id,
            "response",
            &format!("{} responded to: {}", responder_name, post_title),
            None,
            Some(&format!("/messages/{}", match_id)),
        )
        .await
        .ok();
    }

    Ok((match_id, message_id))
}

pub async fn list_conversations(pool: &PgPool, user_id: Uuid) -> Result<Vec<ConversationPreview>> {
    let rows = sqlx::query_as::<_, ConversationPreviewRow>(
        r#"SELECT
            m.id AS match_id,
            m.post_id,
            p.title AS post_title,
            p.kind AS post_kind,
            CASE WHEN p.author_id = $1 THEN m.responder_id ELSE p.author_id END AS other_party_id,
            CASE WHEN p.author_id = $1 THEN ru.display_name ELSE au.display_name END AS other_party_name,
            last.ciphertext AS last_message_ciphertext,
            last.nonce AS last_message_nonce,
            last.created_at AS last_message_at,
            m.status,
            m.created_at
        FROM matches m
        JOIN posts p ON p.id = m.post_id
        JOIN users ru ON ru.id = m.responder_id
        JOIN users au ON au.id = p.author_id
        LEFT JOIN LATERAL (
            SELECT ciphertext, nonce, created_at
            FROM messages WHERE match_id = m.id
            ORDER BY created_at DESC LIMIT 1
        ) last ON true
        WHERE m.responder_id = $1 OR p.author_id = $1
        ORDER BY COALESCE(last.created_at, m.created_at) DESC"#
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get_conversation(
    pool: &PgPool,
    match_id: Uuid,
    user_id: Uuid,
) -> Result<Conversation> {
    let row = sqlx::query_as::<_, MatchDetailRow>(
        r#"SELECT
            m.id AS match_id, m.post_id, p.title AS post_title, p.kind AS post_kind,
            m.responder_id, p.author_id,
            ru.display_name AS responder_name, au.display_name AS author_name,
            m.status, m.created_at
        FROM matches m
        JOIN posts p ON p.id = m.post_id
        JOIN users ru ON ru.id = m.responder_id
        JOIN users au ON au.id = p.author_id
        WHERE m.id = $1 AND (m.responder_id = $2 OR p.author_id = $2)"#,
    )
    .bind(match_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow!("conversation not found"))?;

    let messages = sqlx::query_as::<_, MessageDbRow>(
        "SELECT id, match_id, sender_id, ciphertext, nonce, created_at FROM messages WHERE match_id = $1 ORDER BY created_at ASC"
    )
    .bind(match_id)
    .fetch_all(pool)
    .await?;

    Ok(Conversation {
        match_id: row.match_id,
        post_id: row.post_id,
        post_title: row.post_title,
        post_kind: row.post_kind,
        responder_id: row.responder_id,
        author_id: row.author_id,
        responder_name: row.responder_name,
        author_name: row.author_name,
        status: row.status,
        messages: messages.into_iter().map(Into::into).collect(),
        created_at: row.created_at,
    })
}

pub async fn send_message(
    pool: &PgPool,
    match_id: Uuid,
    sender_id: Uuid,
    ciphertext: &[u8],
    nonce: Option<&[u8]>,
) -> Result<MessageRow> {
    let participant = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM matches m JOIN posts p ON p.id = m.post_id WHERE m.id = $1 AND ($2 = m.responder_id OR $2 = p.author_id))"
    )
    .bind(match_id)
    .bind(sender_id)
    .fetch_one(pool)
    .await?;

    if !participant {
        return Err(anyhow!("not a participant in this conversation"));
    }

    let id = Uuid::now_v7();
    let now = Utc::now();

    sqlx::query(
        "INSERT INTO messages (id, match_id, sender_id, ciphertext, nonce, created_at) VALUES ($1, $2, $3, $4, $5, $6)"
    )
    .bind(id)
    .bind(match_id)
    .bind(sender_id)
    .bind(ciphertext)
    .bind(nonce)
    .bind(now)
    .execute(pool)
    .await?;

    let other_party = sqlx::query_scalar::<_, Uuid>(
        r#"SELECT CASE WHEN p.author_id = $2 THEN m.responder_id ELSE p.author_id END
           FROM matches m JOIN posts p ON p.id = m.post_id WHERE m.id = $1"#,
    )
    .bind(match_id)
    .bind(sender_id)
    .fetch_optional(pool)
    .await?;

    if let Some(recipient_id) = other_party {
        let sender_name =
            sqlx::query_scalar::<_, String>("SELECT display_name FROM users WHERE id = $1")
                .bind(sender_id)
                .fetch_optional(pool)
                .await?
                .unwrap_or_default();
        // The notification carries who, not what — it is stored in cleartext.
        super::notifications::create(
            pool,
            recipient_id,
            "message",
            &format!("New message from {}", sender_name),
            None,
            Some(&format!("/messages/{}", match_id)),
        )
        .await
        .ok();
    }

    Ok(MessageRow {
        id,
        match_id,
        sender_id,
        ciphertext: b64(ciphertext),
        nonce: nonce.map(b64),
        created_at: now,
    })
}

#[derive(FromRow)]
struct MatchDetailRow {
    match_id: Uuid,
    post_id: Uuid,
    post_title: String,
    post_kind: String,
    responder_id: Uuid,
    author_id: Uuid,
    responder_name: String,
    author_name: String,
    status: String,
    created_at: DateTime<Utc>,
}

/// The outcome of a step the thread's own state may refuse. `Conflict` is a legal answer to a
/// legal request that arrived too late, carrying the sentence the API turns into a 409 — kept out
/// of `Err` so a database failure is never reported as a state error.
#[derive(Debug)]
pub enum DealStep<T> {
    Done(T),
    Conflict(String),
}

#[derive(Serialize, Clone, Debug)]
pub struct OfferRow {
    pub id: Uuid,
    pub match_id: Uuid,
    pub actor_id: Uuid,
    pub kind: OfferKind,
    pub amount_cents: Option<i64>,
    pub currency: Option<String>,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct OfferDbRow {
    id: Uuid,
    match_id: Uuid,
    actor_id: Uuid,
    kind: String,
    amount_cents: Option<i64>,
    currency: Option<String>,
    note: Option<String>,
    created_at: DateTime<Utc>,
}

impl TryFrom<OfferDbRow> for OfferRow {
    type Error = anyhow::Error;

    /// `TryFrom` rather than a defaulting `From`: folding an unknown value into `offer` would
    /// report a decline as a bid. `chk_match_offers_kind` makes this unreachable, so reaching it
    /// is worth a 500 over a quietly wrong negotiation history.
    fn try_from(r: OfferDbRow) -> Result<Self> {
        Ok(OfferRow {
            id: r.id,
            match_id: r.match_id,
            actor_id: r.actor_id,
            kind: OfferKind::parse(&r.kind)
                .ok_or_else(|| anyhow!("offer {} has unknown kind {:?}", r.id, r.kind))?,
            amount_cents: r.amount_cents,
            currency: r.currency,
            note: r.note,
            created_at: r.created_at,
        })
    }
}

/// What a route needs before it may act. Deliberately excludes the thread's status: every decision
/// that turns on it is made inside a locked transaction, and a copy out here would be a staler
/// answer to the same question within reach.
#[derive(Debug, Clone)]
pub struct Thread {
    pub responder_id: Uuid,
    pub author_id: Uuid,
    pub post_kind: PostKind,
    pub post_currency: Option<String>,
}

impl Thread {
    pub fn is_participant(&self, user_id: Uuid) -> bool {
        user_id == self.responder_id || user_id == self.author_id
    }

    /// Who the other side of this deal is. Derived from the thread, never client-supplied: a
    /// request naming its own reviewee would let anyone with a completed deal review a stranger.
    /// `None` is the 403 — someone not on this thread has no counterparty here.
    pub fn other_participant(&self, user_id: Uuid) -> Option<Uuid> {
        if user_id == self.author_id {
            Some(self.responder_id)
        } else if user_id == self.responder_id {
            Some(self.author_id)
        } else {
            None
        }
    }
}

#[derive(FromRow)]
struct ThreadRow {
    post_id: Uuid,
    responder_id: Uuid,
    author_id: Uuid,
    post_kind: String,
    post_currency: Option<String>,
}

impl TryFrom<ThreadRow> for Thread {
    type Error = anyhow::Error;

    fn try_from(r: ThreadRow) -> Result<Self> {
        Ok(Thread {
            responder_id: r.responder_id,
            author_id: r.author_id,
            // Unlike `db::posts`, which defaults an unreadable kind so a feed still renders, this
            // one decides whether offers are allowed at all: guessing would open a market flow on
            // an aid thread or close one on a listing.
            post_kind: PostKind::parse(&r.post_kind)
                .ok_or_else(|| anyhow!("post {} has unknown kind {:?}", r.post_id, r.post_kind))?,
            post_currency: r.post_currency,
        })
    }
}

/// `Ok(None)` for an id that does not exist, so the caller can answer 404 rather than 500.
pub async fn load_thread(pool: &PgPool, match_id: Uuid) -> Result<Option<Thread>> {
    let row = sqlx::query_as::<_, ThreadRow>(
        r#"SELECT m.post_id, m.responder_id,
                  p.author_id, p.kind AS post_kind, p.currency AS post_currency
           FROM matches m
           JOIN posts p ON p.id = m.post_id
           WHERE m.id = $1"#,
    )
    .bind(match_id)
    .fetch_optional(pool)
    .await?;

    row.map(Thread::try_from).transpose()
}

pub const SELF_ACCEPT: &str =
    "you cannot accept your own offer: the other party is the one who agrees to it";

pub const NOTHING_TO_ACCEPT: &str =
    "there is no offer on this conversation yet, so there is nothing to accept";

/// Whether a thread in `from` may be moved to `to`.
///
/// Pure and shared by the handler and the locked transaction, so there is only one idea of a legal
/// transition. Going back to `proposed` is disallowed: `agreed_price_cents` would survive on a
/// thread that no longer has an agreement.
pub fn check_transition(from: MatchStatus, to: MatchStatus) -> Result<(), String> {
    use MatchStatus::*;

    let allowed = matches!(
        (from, to),
        (Proposed, Proposed)
            | (Proposed, Accepted)
            | (Proposed, Withdrawn)
            | (Accepted, Completed)
            | (Accepted, Withdrawn)
    );
    if allowed {
        return Ok(());
    }

    // Each branch names the status the thread is in: the one fact a caller cannot guess, without
    // which a 409 is unactionable.
    Err(match (from, to) {
        (Proposed, Completed) => format!(
            "this deal is still '{from}': it can only be completed once an offer has been accepted"
        ),
        _ if from == to => format!("this conversation is already '{from}'"),
        _ => format!("a conversation that is '{from}' cannot be moved to '{to}'"),
    })
}

/// Accepting is a `proposed -> accepted` move, refused for the same reasons — including on a
/// withdrawn thread, which is neither accepted nor completed but is over.
pub fn check_accept_allowed(current: MatchStatus) -> Result<(), String> {
    check_transition(current, MatchStatus::Accepted)
}

/// Who may press accept: whoever did not make the number being accepted. Pure so the rule can be
/// pinned without a database; the lookup that feeds it runs inside the accept transaction.
pub fn check_accept_actor(last_offeror: Option<Uuid>, actor_id: Uuid) -> Result<(), String> {
    match last_offeror {
        None => Err(NOTHING_TO_ACCEPT.to_string()),
        Some(offeror) if offeror == actor_id => Err(SELF_ACCEPT.to_string()),
        Some(_) => Ok(()),
    }
}

/// Append one row. The negotiation is append-only: a counter that could rewrite the offer it
/// answers is not a record of anything.
async fn insert_offer(
    conn: &mut sqlx::PgConnection,
    match_id: Uuid,
    actor_id: Uuid,
    kind: OfferKind,
    amount_cents: Option<i64>,
    currency: Option<&str>,
    note: Option<&str>,
) -> Result<OfferRow> {
    let row = sqlx::query_as::<_, OfferDbRow>(
        r#"INSERT INTO match_offers (id, match_id, actor_id, kind, amount_cents, currency, note, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
           RETURNING id, match_id, actor_id, kind, amount_cents, currency, note, created_at"#,
    )
    .bind(Uuid::now_v7())
    .bind(match_id)
    .bind(actor_id)
    .bind(kind.as_str())
    .bind(amount_cents)
    .bind(currency)
    .bind(note)
    .bind(Utc::now())
    .fetch_one(conn)
    .await?;

    row.try_into()
}

/// Whether a thread in `current` may take a new `offer` or `counter`. A withdrawn thread took a
/// decline and a completed one took a deal; a number appended after either reads as a live offer
/// waiting for an answer that can never come. `Accepted` stays open on purpose — a counter after
/// an accept is how a wrong deal is renegotiated before completion, and refusing it would send
/// that conversation off the thread the reviews are anchored to.
pub fn check_offer_allowed(current: MatchStatus) -> Result<(), String> {
    match current {
        MatchStatus::Proposed | MatchStatus::Accepted => Ok(()),
        // Written as the live set, not `!current.is_resolved()`, so a future status is refused
        // until someone decides it is live: offers fail closed.
        _ => Err(format!(
            "this conversation is '{current}' and can no longer take new offers"
        )),
    }
}

/// An `offer` or a `counter`: an append that moves no state of its own. Still runs under the row
/// lock, because whether it may be appended turns on a status another request can change at the
/// same moment — without the lock, a decline committing mid-flight leaves an offer under a closed
/// thread.
pub async fn append_offer(
    pool: &PgPool,
    match_id: Uuid,
    actor_id: Uuid,
    kind: OfferKind,
    amount_cents: Option<i64>,
    currency: Option<&str>,
    note: Option<&str>,
) -> Result<DealStep<OfferRow>> {
    let mut tx = pool.begin().await?;

    let status = lock_status(&mut tx, match_id).await?;
    if let Err(why) = check_offer_allowed(status) {
        return Ok(DealStep::Conflict(why));
    }

    let row = insert_offer(
        &mut tx,
        match_id,
        actor_id,
        kind,
        amount_cents,
        currency,
        note,
    )
    .await?;

    tx.commit().await?;
    Ok(DealStep::Done(row))
}

/// The whole negotiation, oldest first. `id` breaks the tie: `created_at` is microsecond-resolution
/// and Postgres may reorder rows written in the same microsecond, which an append-only trail must
/// not allow. UUIDv7 sorts by time, so the tie-break agrees with the clock.
pub async fn list_offers(pool: &PgPool, match_id: Uuid) -> Result<Vec<OfferRow>> {
    let rows = sqlx::query_as::<_, OfferDbRow>(
        r#"SELECT id, match_id, actor_id, kind, amount_cents, currency, note, created_at
           FROM match_offers
           WHERE match_id = $1
           ORDER BY created_at ASC, id ASC"#,
    )
    .bind(match_id)
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(OfferRow::try_from).collect()
}

/// Read the thread's status with the row locked for the rest of the transaction. Every decision on
/// this thread, reviews included, reads the status here — under the lock, inside the acting
/// transaction.
pub(crate) async fn lock_status(
    conn: &mut sqlx::PgConnection,
    match_id: Uuid,
) -> Result<MatchStatus> {
    let status: String = sqlx::query_scalar("SELECT status FROM matches WHERE id = $1 FOR UPDATE")
        .bind(match_id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| anyhow!("conversation {match_id} disappeared mid-transaction"))?;

    MatchStatus::parse(&status)
        .ok_or_else(|| anyhow!("match {match_id} has unknown status {status:?}"))
}

/// Accepting an offer, atomically. One transaction does all four things — the accept row, the
/// agreed price, currency and status — because any two apart is an observable wrong state. The row
/// is locked before the first check, so two accepts at the same instant cannot both pass and have
/// the slower overwrite the agreed price.
pub async fn accept_offer(
    pool: &PgPool,
    match_id: Uuid,
    actor_id: Uuid,
    amount_cents: i64,
    currency: Option<&str>,
    note: Option<&str>,
) -> Result<DealStep<OfferRow>> {
    let mut tx = pool.begin().await?;

    let status = lock_status(&mut tx, match_id).await?;
    if let Err(why) = check_accept_allowed(status) {
        return Ok(DealStep::Conflict(why));
    }

    // The last row that put a number on the table: a `decline` or an earlier `accept` is not an
    // offer, so neither can be the thing being accepted.
    let last_offeror: Option<Uuid> = sqlx::query_scalar(
        r#"SELECT actor_id FROM match_offers
           WHERE match_id = $1 AND kind IN ('offer', 'counter')
           ORDER BY created_at DESC, id DESC
           LIMIT 1"#,
    )
    .bind(match_id)
    .fetch_optional(&mut *tx)
    .await?;

    if let Err(why) = check_accept_actor(last_offeror, actor_id) {
        return Ok(DealStep::Conflict(why));
    }

    let row = insert_offer(
        &mut tx,
        match_id,
        actor_id,
        OfferKind::Accept,
        Some(amount_cents),
        currency,
        note,
    )
    .await?;

    sqlx::query(
        "UPDATE matches SET status = 'accepted', agreed_price_cents = $2, currency = $3 WHERE id = $1",
    )
    .bind(match_id)
    .bind(amount_cents)
    .bind(currency)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(DealStep::Done(row))
}

/// A `decline` is recorded as a row, then closes the thread as `withdrawn`, so the trail says who
/// walked away and why rather than just that it ended.
pub async fn decline_offer(
    pool: &PgPool,
    match_id: Uuid,
    actor_id: Uuid,
    currency: Option<&str>,
    note: Option<&str>,
) -> Result<DealStep<OfferRow>> {
    let mut tx = pool.begin().await?;

    let status = lock_status(&mut tx, match_id).await?;
    if let Err(why) = check_transition(status, MatchStatus::Withdrawn) {
        return Ok(DealStep::Conflict(why));
    }

    let row = insert_offer(
        &mut tx,
        match_id,
        actor_id,
        OfferKind::Decline,
        None,
        currency,
        note,
    )
    .await?;

    sqlx::query("UPDATE matches SET status = 'withdrawn', resolved_at = $2 WHERE id = $1")
        .bind(match_id)
        .bind(Utc::now())
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(DealStep::Done(row))
}

/// The deal lifecycle behind `PATCH /api/conversations/{id}/status`. Every move goes through one
/// locked transaction and one rule (`check_transition`), so a completed deal and a sold listing
/// cannot diverge if a second statement fails.
pub async fn update_status(pool: &PgPool, match_id: Uuid, to: MatchStatus) -> Result<DealStep<()>> {
    let mut tx = pool.begin().await?;

    let from = lock_status(&mut tx, match_id).await?;
    if let Err(why) = check_transition(from, to) {
        return Ok(DealStep::Conflict(why));
    }

    // One listing, one sale. Two matches on the same post may each reach `accepted`, but only the
    // first to complete is the sale; without this the second silently rewrites `sold_at` and
    // `buyer_id`. `FOR UPDATE OF p` serialises two completions rather than letting both read NULL
    // and both write.
    if to == MatchStatus::Completed {
        let sold_at: Option<Option<DateTime<Utc>>> = sqlx::query_scalar(
            r#"SELECT p.sold_at
               FROM posts p
               JOIN matches m ON m.post_id = p.id
               WHERE m.id = $1
               FOR UPDATE OF p"#,
        )
        .bind(match_id)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(Some(_)) = sold_at {
            return Ok(DealStep::Conflict(
                "this listing is already sold".to_string(),
            ));
        }
    }

    // `resolved_at` marks the end of a thread, so only the two terminal statuses set it.
    let resolved_at = if to.is_resolved() {
        Some(Utc::now())
    } else {
        None
    };

    sqlx::query("UPDATE matches SET status = $2, resolved_at = $3 WHERE id = $1")
        .bind(match_id)
        .bind(to.as_str())
        .bind(resolved_at)
        .execute(&mut *tx)
        .await?;

    if to == MatchStatus::Completed {
        // `sold_at`/`buyer_id` are only allowed on a `listing` or `want` by the CHECK on `posts`,
        // while `fulfilled` applies to every kind. One statement, so a completed deal and a sold
        // post commit together or not at all. The buyer is the non-author participant, derived
        // from the row rather than from whoever pressed the button.
        sqlx::query(
            r#"UPDATE posts p SET
                 status = 'fulfilled',
                 sold_at = CASE WHEN p.kind IN ('listing', 'want') THEN now() ELSE p.sold_at END,
                 buyer_id = CASE WHEN p.kind IN ('listing', 'want') THEN m.responder_id ELSE p.buyer_id END,
                 updated_at = now()
               FROM matches m
               WHERE m.id = $1 AND p.id = m.post_id"#,
        )
        .bind(match_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(DealStep::Done(()))
}
