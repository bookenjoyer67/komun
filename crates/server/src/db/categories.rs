//! The `categories` table.
//!
//! SPEC 1.6: the taxonomy is seed data, not a Rust enum, so an admin can add, relabel, reorder or
//! retire a category without a release. `posts.category` is a foreign key onto `slug` with
//! `ON DELETE RESTRICT`, which is why nothing here deletes a row — retiring one means
//! `active = false`.

use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};

use komun_core::models::{Category, CategoryScope, CreateCategory, UpdateCategory};

/// Every column a [`Category`] is built from, in one place so the four queries below cannot
/// drift from each other.
const CATEGORY_COLUMNS: &str = "slug, label, scope, sort_order, active, created_at, updated_at";

/// The active taxonomy, optionally narrowed to one scope.
///
/// `scope` is a **union, not an equality**: asking for `market` returns the market-only rows AND
/// the `both` rows, because a `both` category is usable from the market form. Against the seeded
/// list that is 15 + 6 = 21 rows for `market` and 2 + 6 = 8 for `aid`.
///
/// The filter runs in SQL rather than in Rust so the database does the work and the partial index
/// `idx_categories_scope` can be used. `include_inactive` is the admin's switch — a retired
/// category has to stay visible to whoever retired it, and invisible to everyone else.
pub async fn list(
    pool: &PgPool,
    scope: Option<CategoryScope>,
    include_inactive: bool,
) -> Result<Vec<Category>> {
    let rows = sqlx::query_as::<_, CategoryRow>(&format!(
        r#"SELECT {CATEGORY_COLUMNS}
           FROM categories
           WHERE ($1::bool OR active)
             AND ($2::text IS NULL OR scope = $2 OR scope = 'both')
           ORDER BY sort_order, label"#
    ))
    .bind(include_inactive)
    .bind(scope.map(|s| s.as_str()))
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

/// `Ok(None)` rather than an error, so the caller can answer 404 instead of 500.
pub async fn get(pool: &PgPool, slug: &str) -> Result<Option<Category>> {
    let row = sqlx::query_as::<_, CategoryRow>(&format!(
        "SELECT {CATEGORY_COLUMNS} FROM categories WHERE slug = $1"
    ))
    .bind(slug)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(Into::into))
}

/// Create a category. `Ok(None)` means the slug is already taken.
///
/// `ON CONFLICT DO NOTHING` rather than letting the primary key violation escape: a duplicate
/// slug is a client error worth a 409, not a 500 with a constraint name in the response body.
pub async fn create(pool: &PgPool, input: &CreateCategory) -> Result<Option<Category>> {
    let row = sqlx::query_as::<_, CategoryRow>(&format!(
        r#"INSERT INTO categories (slug, label, scope, sort_order)
           VALUES ($1, $2, $3, COALESCE($4::int, 0))
           ON CONFLICT (slug) DO NOTHING
           RETURNING {CATEGORY_COLUMNS}"#
    ))
    .bind(&input.slug)
    .bind(&input.label)
    .bind(input.scope.as_str())
    .bind(input.sort_order)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(Into::into))
}

/// Apply a partial update and return the new row. `Ok(None)` means no such category.
///
/// The slug is not updatable, and [`UpdateCategory`] has no field for it: `posts.category`
/// references this column, so a rename would either orphan posts or need a cascade the schema
/// deliberately withholds. The label is the part that is meant to change.
pub async fn update(pool: &PgPool, slug: &str, input: &UpdateCategory) -> Result<Option<Category>> {
    let row = sqlx::query_as::<_, CategoryRow>(&format!(
        r#"UPDATE categories SET
             label      = COALESCE($2::text, label),
             scope      = COALESCE($3::text, scope),
             sort_order = COALESCE($4::int,  sort_order),
             active     = COALESCE($5::bool, active),
             updated_at = now()
           WHERE slug = $1
           RETURNING {CATEGORY_COLUMNS}"#
    ))
    .bind(slug)
    .bind(input.label.as_deref())
    .bind(input.scope.map(|s| s.as_str()))
    .bind(input.sort_order)
    .bind(input.active)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(Into::into))
}

/// Recompute the search vector of every post in a category, returning how many were touched.
///
/// `posts_search_update()` indexes the category **label**, not the slug (001_schema.sql), so a
/// relabel leaves every affected post findable only under the old word. The trigger is
/// `BEFORE INSERT OR UPDATE`, so writing `updated_at` back to itself is what re-runs it — a real
/// no-op to the data, which is the point: a rename must not make every post in the category look
/// as though its author had just edited it.
pub async fn refresh_search_vectors(pool: &PgPool, slug: &str) -> Result<u64> {
    let result = sqlx::query("UPDATE posts SET updated_at = updated_at WHERE category = $1")
        .bind(slug)
        .execute(pool)
        .await?;

    Ok(result.rows_affected())
}

#[derive(FromRow)]
struct CategoryRow {
    slug: String,
    label: String,
    scope: String,
    sort_order: i32,
    active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<CategoryRow> for Category {
    fn from(r: CategoryRow) -> Self {
        Category {
            slug: r.slug,
            label: r.label,
            // `chk_categories_scope` means this cannot happen, but a panic inside a request
            // handler is not the way to find out that it did. `Both` is the inert reading: it
            // claims no scope the row did not already have access to under the union rule.
            scope: CategoryScope::parse(&r.scope).unwrap_or(CategoryScope::Both),
            sort_order: r.sort_order,
            active: r.active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}
