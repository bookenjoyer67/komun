use anyhow::Result;
use chrono::Utc;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use komun_core::models::{
    coarsen_coordinate, CreatePost, ItemCondition, Post, PostKind, PostStatus, Urgency, Visibility,
};

/// Every column the `Post` model is built from, in one place so `list` and `get` cannot drift.
/// Qualified with `p.` because both queries join `categories`, making `created_at`/`updated_at`
/// ambiguous.
const POST_COLUMNS: &str = r#"p.id, p.author_id, p.kind, p.category, p.title, p.body,
    p.location_name, p.location_lat, p.location_lon, p.urgency, p.quantity, p.status,
    p.visibility, p.expires_at, p.tags, p.contact_method, p.images, p.verified_by, p.verified_at,
    p.market_listed, p.price_cents, p.currency, p.price_negotiable, p.item_condition,
    p.sold_at, p.buyer_id, p.created_at, p.updated_at"#;

/// `LEFT JOIN`, not `JOIN`: an inner join would silently drop a post if the category row ever
/// went missing, and losing a post from the feed is worse than showing one without a label.
const CATEGORY_JOIN: &str = "LEFT JOIN categories c ON c.slug = p.category";

/// An unbounded feed is a denial of service the caller need not ask for.
pub const DEFAULT_LIMIT: i64 = 100;
pub const MAX_LIMIT: i64 = 200;

/// The validated shape of a list request. Built by `api::posts::validate_filters`, which turns a
/// malformed query string into a 400, so every field here is known-good and this layer only binds
/// it.
#[derive(Debug, Clone)]
pub struct PostFilter {
    pub kind: Option<PostKind>,
    pub category: Option<String>,
    pub status: Option<PostStatus>,
    pub q: Option<String>,
    pub min_price_cents: Option<i64>,
    pub max_price_cents: Option<i64>,
    pub currency: Option<String>,
    pub item_condition: Option<ItemCondition>,
    pub limit: i64,
    pub offset: i64,
}

impl Default for PostFilter {
    fn default() -> Self {
        Self {
            kind: None,
            category: None,
            status: None,
            q: None,
            min_price_cents: None,
            max_price_cents: None,
            currency: None,
            item_condition: None,
            limit: DEFAULT_LIMIT,
            offset: 0,
        }
    }
}

/// Whether the public feed shows a post: the same rule `list` applies in SQL. Every status is
/// named, so a new one does not compile until someone decides whether the public sees it.
pub fn publicly_visible(status: PostStatus, visibility: Visibility) -> bool {
    let open = match status {
        PostStatus::Active | PostStatus::Matched | PostStatus::Fulfilled | PostStatus::Expired => {
            true
        }
        PostStatus::Withdrawn | PostStatus::Hidden | PostStatus::Flagged => false,
    };
    open && visibility == Visibility::Public
}

/// The public feed: a flat, server-wide collection.
///
/// Filters `visibility = 'public'` because this route has no authenticated caller to compare a
/// `private` post against, and excludes the moderation statuses so a hidden post does not return
/// to the feed. A price filter also drops aid posts: their `price_cents` is NULL, so `NULL >= $5`
/// is NULL — asking for a price range asks for things that have a price.
pub async fn list(pool: &PgPool, filter: &PostFilter) -> Result<Vec<Post>> {
    let search = filter.q.as_deref().map(|s| format!("%{}%", s));
    let rows = sqlx::query_as::<_, PostRow>(&format!(
        r#"SELECT {POST_COLUMNS}, c.label AS category_label
           FROM posts p
           {CATEGORY_JOIN}
           WHERE p.status NOT IN ('withdrawn', 'hidden', 'flagged')
             AND p.visibility = 'public'
             AND ($1::text IS NULL OR p.kind = $1)
             AND ($2::text IS NULL OR p.category = $2)
             AND ($3::text IS NULL OR p.status = $3)
             AND ($4::text IS NULL OR p.title ILIKE $4 OR p.body ILIKE $4)
             AND ($5::bigint IS NULL OR p.price_cents >= $5)
             AND ($6::bigint IS NULL OR p.price_cents <= $6)
             AND ($7::text IS NULL OR p.currency = $7)
             AND ($8::text IS NULL OR p.item_condition = $8)
           ORDER BY
             CASE WHEN p.urgency = 'critical' THEN 0
                  WHEN p.urgency = 'high' THEN 1
                  WHEN p.urgency = 'medium' THEN 2
                  ELSE 3 END,
             p.created_at DESC
           LIMIT $9 OFFSET $10"#
    ))
    .bind(filter.kind.map(|k| k.as_str()))
    .bind(filter.category.as_deref())
    .bind(filter.status.map(|s| s.as_str()))
    .bind(search)
    .bind(filter.min_price_cents)
    .bind(filter.max_price_cents)
    .bind(filter.currency.as_deref())
    .bind(filter.item_condition.map(|c| c.as_str()))
    .bind(filter.limit)
    .bind(filter.offset)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

/// `Ok(None)` rather than an error, so the caller can answer 404 instead of 500.
pub async fn get(pool: &PgPool, id: Uuid) -> Result<Option<Post>> {
    let row = sqlx::query_as::<_, PostRow>(&format!(
        "SELECT {POST_COLUMNS}, c.label AS category_label
         FROM posts p
         {CATEGORY_JOIN}
         WHERE p.id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(Into::into))
}

/// The caller validates the coordinates first: coarsening here would turn an out-of-range value
/// into a legal one.
pub async fn create(pool: &PgPool, author_id: Uuid, input: CreatePost) -> Result<Post> {
    let id = Uuid::now_v7();
    let now = Utc::now();

    // The DB string comes from the enum itself: the value lands in a CHECK-constrained column, so
    // the wire format must not be in charge of it.
    let kind = input.kind.as_str();
    let urgency = input.urgency.map(|u| u.as_str());
    let visibility = input.visibility.unwrap_or(Visibility::Public).as_str();
    let item_condition = input.item_condition.map(|c| c.as_str());
    let tags = input.tags.unwrap_or_default();

    sqlx::query(
        r#"INSERT INTO posts (id, author_id, kind, category, title, body,
           location_name, location_lat, location_lon, urgency, quantity, status,
           visibility, expires_at, tags, contact_method,
           market_listed, price_cents, currency, price_negotiable, item_condition,
           created_at, updated_at)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'active',$12,$13,$14,$15,
                   $16,$17,$18,$19,$20,$21,$21)"#,
    )
    .bind(id)
    .bind(author_id)
    .bind(kind)
    .bind(&input.category)
    .bind(&input.title)
    .bind(&input.body)
    .bind(&input.location_name)
    .bind(input.location_lat.map(coarsen_coordinate))
    .bind(input.location_lon.map(coarsen_coordinate))
    .bind(urgency)
    .bind(input.quantity)
    .bind(visibility)
    .bind(input.expires_at)
    .bind(&tags)
    .bind(&input.contact_method)
    .bind(input.market_listed)
    .bind(input.price_cents)
    .bind(&input.currency)
    .bind(input.price_negotiable)
    .bind(item_condition)
    .bind(now)
    .execute(pool)
    .await?;

    get(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("post disappeared immediately after insert"))
}

/// An author's edit. The WHERE repeats the handler's rule, so a post moderated or sold between
/// the handler's read and this write is left alone; 0 rows written means that happened.
/// `IS DISTINCT FROM` rather than `NOT ($5 = 'active' AND ...)`: with no status in the edit `$5`
/// is NULL, and the `NOT` form would then be NULL and refuse a text edit on a sold listing.
pub async fn update(
    pool: &PgPool,
    id: Uuid,
    title: Option<String>,
    body: Option<String>,
    urgency: Option<Urgency>,
    status: Option<PostStatus>,
) -> Result<u64> {
    let result = sqlx::query(
        r#"UPDATE posts SET
           title = COALESCE($2, title),
           body = COALESCE($3, body),
           urgency = COALESCE($4, urgency),
           status = COALESCE($5, status),
           updated_at = $6
           WHERE id = $1
             AND status NOT IN ('hidden', 'flagged')
             AND ($5::text IS DISTINCT FROM 'active' OR sold_at IS NULL)"#,
    )
    .bind(id)
    .bind(title)
    .bind(body)
    .bind(urgency.map(|u| u.as_str()))
    .bind(status.map(|s| s.as_str()))
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn withdraw(pool: &PgPool, id: Uuid) -> Result<()> {
    sqlx::query("UPDATE posts SET status = 'withdrawn', updated_at = $2 WHERE id = $1")
        .bind(id)
        .bind(Utc::now())
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(FromRow)]
struct PostRow {
    id: Uuid,
    author_id: Uuid,
    kind: String,
    category: String,
    /// From the `categories` join. `Option` because the join is a LEFT JOIN.
    category_label: Option<String>,
    title: String,
    body: Option<String>,
    location_name: Option<String>,
    location_lat: Option<f64>,
    location_lon: Option<f64>,
    urgency: Option<String>,
    quantity: Option<i32>,
    status: String,
    visibility: String,
    expires_at: Option<chrono::DateTime<Utc>>,
    tags: Option<Vec<String>>,
    contact_method: Option<String>,
    images: Option<Vec<String>>,
    verified_by: Option<Uuid>,
    verified_at: Option<chrono::DateTime<Utc>>,
    market_listed: bool,
    price_cents: Option<i64>,
    currency: Option<String>,
    price_negotiable: bool,
    item_condition: Option<String>,
    sold_at: Option<chrono::DateTime<Utc>>,
    buyer_id: Option<Uuid>,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
}

impl From<PostRow> for Post {
    fn from(r: PostRow) -> Self {
        Post {
            id: r.id,
            author_id: r.author_id,
            // Parses every kind the schema allows; a `listing` or `want` must not fold into `Need`.
            kind: PostKind::parse(&r.kind).unwrap_or(PostKind::Need),
            category: r.category,
            // The label is what a human reads and the only part of the taxonomy renamable at
            // runtime; serving the slug alone forces every client to keep its own copy.
            category_label: r.category_label,
            title: r.title,
            body: r.body,
            location_name: r.location_name,
            // Coarsened again on read: rows written before coarsening keep their exact stored
            // values, and this is the only place they leave the database.
            location_lat: r.location_lat.map(coarsen_coordinate),
            location_lon: r.location_lon.map(coarsen_coordinate),
            urgency: r.urgency.as_deref().and_then(Urgency::parse),
            quantity: r.quantity,
            status: PostStatus::parse(&r.status).unwrap_or(PostStatus::Active),
            visibility: Visibility::parse(&r.visibility).unwrap_or(Visibility::Public),
            expires_at: r.expires_at,
            tags: r.tags.unwrap_or_default(),
            contact_method: r.contact_method,
            images: r.images.unwrap_or_default(),
            verified_by: r.verified_by,
            verified_at: r.verified_at,
            market_listed: r.market_listed,
            price_cents: r.price_cents,
            currency: r.currency,
            price_negotiable: r.price_negotiable,
            item_condition: r.item_condition.as_deref().and_then(ItemCondition::parse),
            sold_at: r.sold_at,
            buyer_id: r.buyer_id,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: &str) -> PostRow {
        let now = Utc::now();
        PostRow {
            id: Uuid::now_v7(),
            author_id: Uuid::now_v7(),
            kind: kind.to_string(),
            category: "food".to_string(),
            category_label: Some("Food".to_string()),
            title: "t".to_string(),
            body: None,
            location_name: None,
            location_lat: None,
            location_lon: None,
            urgency: None,
            quantity: None,
            status: "active".to_string(),
            visibility: "public".to_string(),
            expires_at: None,
            tags: None,
            contact_method: None,
            images: None,
            verified_by: None,
            verified_at: None,
            market_listed: false,
            price_cents: None,
            currency: None,
            price_negotiable: false,
            item_condition: None,
            sold_at: None,
            buyer_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn every_kind_the_schema_allows_survives_the_row_conversion() {
        for kind in PostKind::ALL {
            let post: Post = row(kind.as_str()).into();
            assert_eq!(post.kind, *kind, "kind {:?} did not round-trip", kind);
        }
    }

    #[test]
    fn the_marketplace_facet_comes_from_the_row_not_from_a_hardcoded_default() {
        let mut r = row("listing");
        r.market_listed = true;
        r.price_cents = Some(2500);
        r.currency = Some("EUR".to_string());
        r.price_negotiable = true;
        r.item_condition = Some("like_new".to_string());

        let post: Post = r.into();
        assert!(post.market_listed);
        assert_eq!(post.price_cents, Some(2500));
        assert_eq!(post.currency.as_deref(), Some("EUR"));
        assert!(post.price_negotiable);
        assert_eq!(post.item_condition, Some(ItemCondition::LikeNew));
    }

    /// A value no enum knows must not panic or silently become a different valid value;
    /// `need`/`active`/`public` are the inert defaults.
    #[test]
    fn an_unrecognised_column_value_degrades_instead_of_panicking() {
        let mut r = row("something_new");
        r.status = "something_new".to_string();
        r.visibility = "something_new".to_string();
        r.urgency = Some("something_new".to_string());
        r.item_condition = Some("something_new".to_string());

        let post: Post = r.into();
        assert_eq!(post.kind, PostKind::Need);
        assert_eq!(post.status, PostStatus::Active);
        assert_eq!(post.visibility, Visibility::Public);
        assert_eq!(post.urgency, None);
        assert_eq!(post.item_condition, None);
    }

    /// Rows written before coarsening keep their exact values, so the read path is what serves
    /// them coarse.
    #[test]
    fn an_exact_stored_location_is_served_coarse() {
        let mut r = row("need");
        r.location_lat = Some(37.80443);
        r.location_lon = Some(-122.27121);

        let post: Post = r.into();
        assert_eq!(post.location_lat, Some(37.8));
        assert_eq!(post.location_lon, Some(-122.3));
    }

    #[test]
    fn a_post_without_a_location_still_has_none() {
        let post: Post = row("need").into();
        assert_eq!(post.location_lat, None);
        assert_eq!(post.location_lon, None);
    }
}
