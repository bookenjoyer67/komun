use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

db_enum!(
    PostKind {
        Resource => "resource",
        Need => "need",
        Offer => "offer",
        Listing => "listing",
        Want => "want",
    }
);

impl PostKind {
    /// Marketplace kinds are the only ones that may carry price or condition.
    pub fn is_market(&self) -> bool {
        matches!(self, PostKind::Listing | PostKind::Want)
    }
}

db_enum!(
    Urgency {
        Critical => "critical",
        High => "high",
        Medium => "medium",
        Low => "low",
    }
);

db_enum!(
    PostStatus {
        Active => "active",
        Matched => "matched",
        Fulfilled => "fulfilled",
        Expired => "expired",
        Withdrawn => "withdrawn",
        Hidden => "hidden",
        Flagged => "flagged",
    }
);

db_enum!(
    Visibility {
        Public => "public",
        Private => "private",
    }
);

db_enum!(
    ItemCondition {
        New => "new",
        LikeNew => "like_new",
        Good => "good",
        Fair => "fair",
        Poor => "poor",
        ForParts => "for_parts",
    }
);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Post {
    pub id: Uuid,
    pub author_id: Uuid,
    pub kind: PostKind,
    /// Slug into the `categories` table.
    pub category: String,
    /// Human label for `category`, joined in by list/detail queries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_label: Option<String>,
    pub title: String,
    pub body: Option<String>,
    pub location_name: Option<String>,
    pub location_lat: Option<f64>,
    pub location_lon: Option<f64>,
    pub urgency: Option<Urgency>,
    pub quantity: Option<i32>,
    pub status: PostStatus,
    pub visibility: Visibility,
    pub expires_at: Option<DateTime<Utc>>,
    pub tags: Vec<String>,
    pub contact_method: Option<String>,
    pub images: Vec<String>,
    pub verified_by: Option<Uuid>,
    pub verified_at: Option<DateTime<Utc>>,
    // marketplace facet — only meaningful on PostKind::Listing / PostKind::Want
    pub market_listed: bool,
    pub price_cents: Option<i64>,
    pub currency: Option<String>,
    pub price_negotiable: bool,
    pub item_condition: Option<ItemCondition>,
    pub sold_at: Option<DateTime<Utc>>,
    pub buyer_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePost {
    pub kind: PostKind,
    pub category: String,
    pub title: String,
    pub body: Option<String>,
    pub location_name: Option<String>,
    pub location_lat: Option<f64>,
    pub location_lon: Option<f64>,
    pub urgency: Option<Urgency>,
    pub quantity: Option<i32>,
    pub visibility: Option<Visibility>,
    pub expires_at: Option<DateTime<Utc>>,
    pub tags: Option<Vec<String>>,
    pub contact_method: Option<String>,
    #[serde(default)]
    pub market_listed: bool,
    pub price_cents: Option<i64>,
    pub currency: Option<String>,
    #[serde(default)]
    pub price_negotiable: bool,
    pub item_condition: Option<ItemCondition>,
}
