use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

db_enum!(
    MatchStatus {
        Proposed => "proposed",
        Accepted => "accepted",
        Completed => "completed",
        Withdrawn => "withdrawn",
    }
);

impl MatchStatus {
    /// The two terminal statuses. A thread that reached either one is over: `resolved_at` is set
    /// for it, and no further offer may be appended to it.
    pub fn is_resolved(&self) -> bool {
        matches!(self, MatchStatus::Completed | MatchStatus::Withdrawn)
    }
}

db_enum!(
    /// A step in a marketplace negotiation, carried on the same thread as the messages.
    OfferKind {
        Offer => "offer",
        Counter => "counter",
        Accept => "accept",
        Decline => "decline",
    }
);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AidMatch {
    pub id: Uuid,
    pub post_id: Uuid,
    pub responder_id: Uuid,
    pub responder_post_id: Option<Uuid>,
    pub message: Option<String>,
    pub status: MatchStatus,
    pub agreed_price_cents: Option<i64>,
    pub currency: Option<String>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMatch {
    pub message: Option<String>,
    pub responder_post_id: Option<Uuid>,
    /// Opening amount for a marketplace thread, written as the first `match_offers` row.
    pub amount_cents: Option<i64>,
}

/// A message on a match thread. The server stores and relays ciphertext only —
/// there is no plaintext body column and no key with which to read one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: Uuid,
    pub match_id: Uuid,
    pub sender_id: Uuid,
    /// Base64 ChaCha20Poly1305 ciphertext.
    pub ciphertext: String,
    /// Base64 nonce for `ciphertext`.
    pub nonce: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessage {
    pub ciphertext: String,
    pub nonce: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchOffer {
    pub id: Uuid,
    pub match_id: Uuid,
    pub actor_id: Uuid,
    pub kind: OfferKind,
    pub amount_cents: Option<i64>,
    pub currency: Option<String>,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOffer {
    pub kind: OfferKind,
    pub amount_cents: Option<i64>,
    pub currency: Option<String>,
    pub note: Option<String>,
}

/// A star rating written against a completed deal. One per reviewer per match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DealReview {
    pub id: Uuid,
    pub match_id: Uuid,
    pub reviewer_id: Uuid,
    pub reviewee_id: Uuid,
    pub rating: i16,
    pub body: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDealReview {
    pub rating: i16,
    pub body: Option<String>,
}
