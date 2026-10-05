use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

db_enum!(
    /// Which half of the app a category is offered in.
    CategoryScope {
        Aid => "aid",
        Market => "market",
        Both => "both",
    }
);

/// A row of the `categories` table; the taxonomy is seed data, not a Rust enum, editable without a release.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub slug: String,
    pub label: String,
    pub scope: CategoryScope,
    pub sort_order: i32,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCategory {
    pub slug: String,
    pub label: String,
    pub scope: CategoryScope,
    pub sort_order: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCategory {
    pub label: Option<String>,
    pub scope: Option<CategoryScope>,
    pub sort_order: Option<i32>,
    pub active: Option<bool>,
}
