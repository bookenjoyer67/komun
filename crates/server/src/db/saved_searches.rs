//! A user's saved searches. A saved centre sits on the same grid as a post's, and what makes two
//! searches the same is `chk_saved_searches_unique` in `005_saved_searches.sql`.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

use komun_core::models::coarsen_coordinate;

pub const UNIQUE_CONSTRAINT: &str = "chk_saved_searches_unique";

const COLUMNS: &str = "id, label, q, kind, category, near_lat, near_lon, radius_km, created_at";

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct SavedSearch {
    pub id: Uuid,
    pub label: Option<String>,
    pub q: Option<String>,
    pub kind: Option<String>,
    pub category: Option<String>,
    pub near_lat: Option<f64>,
    pub near_lon: Option<f64>,
    pub radius_km: Option<f64>,
    pub created_at: DateTime<Utc>,
}

/// A search the API has validated. `near` may still be exact: [`create`] coarsens it on write.
#[derive(Debug, Clone, PartialEq)]
pub struct NewSavedSearch {
    pub label: Option<String>,
    pub q: Option<String>,
    pub kind: Option<String>,
    pub category: Option<String>,
    pub near: Option<(f64, f64)>,
    pub radius_km: Option<f64>,
}

pub enum Created {
    Saved(SavedSearch),
    Duplicate,
}

pub async fn create(pool: &PgPool, user_id: Uuid, input: &NewSavedSearch) -> Result<Created> {
    let (near_lat, near_lon) = input.near.unzip();
    let inserted = sqlx::query_as::<_, SavedSearch>(&format!(
        "INSERT INTO saved_searches
             (id, user_id, label, q, kind, category, near_lat, near_lon, radius_km)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING {COLUMNS}"
    ))
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(input.label.as_deref())
    .bind(input.q.as_deref())
    .bind(input.kind.as_deref())
    .bind(input.category.as_deref())
    .bind(near_lat.map(coarsen_coordinate))
    .bind(near_lon.map(coarsen_coordinate))
    .bind(input.radius_km)
    .fetch_one(pool)
    .await;

    match inserted {
        Ok(row) => Ok(Created::Saved(row)),
        Err(sqlx::Error::Database(db))
            if is_duplicate_search(db.code().as_deref(), db.constraint()) =>
        {
            Ok(Created::Duplicate)
        }
        Err(e) => Err(e.into()),
    }
}

/// Only the search key's own violation is a duplicate; any other unique violation stays a 500
/// rather than being reported to the user as a search they already saved.
pub fn is_duplicate_search(code: Option<&str>, constraint: Option<&str>) -> bool {
    code == Some("23505") && constraint == Some(UNIQUE_CONSTRAINT)
}

pub async fn list(pool: &PgPool, user_id: Uuid) -> Result<Vec<SavedSearch>> {
    let rows = sqlx::query_as::<_, SavedSearch>(&format!(
        "SELECT {COLUMNS} FROM saved_searches WHERE user_id = $1 ORDER BY created_at DESC, id DESC"
    ))
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// `false` both for a missing id and for another user's, so the caller cannot tell them apart.
pub async fn delete(pool: &PgPool, id: Uuid, user_id: Uuid) -> Result<bool> {
    let result = sqlx::query("DELETE FROM saved_searches WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() == 1)
}

#[derive(Debug, Clone, FromRow)]
pub struct DueSearch {
    pub id: Uuid,
    pub user_id: Uuid,
    pub label: Option<String>,
    pub q: Option<String>,
    pub kind: Option<String>,
    pub category: Option<String>,
    pub near_lat: Option<f64>,
    pub near_lon: Option<f64>,
    pub radius_km: Option<f64>,
    pub last_notified_at: DateTime<Utc>,
}

pub async fn due(pool: &PgPool, window_opened_by: DateTime<Utc>) -> Result<Vec<DueSearch>> {
    let rows = sqlx::query_as::<_, DueSearch>(
        "SELECT id, user_id, label, q, kind, category, near_lat, near_lon, radius_km,
                last_notified_at
         FROM saved_searches
         WHERE last_notified_at <= $1
         ORDER BY last_notified_at, id",
    )
    .bind(window_opened_by)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Moves the window forward only while it still starts where the caller read it, so of two runs
/// that read the same window exactly one claims it. The row stays locked until the caller's
/// transaction ends.
pub async fn claim_window(
    conn: &mut PgConnection,
    id: Uuid,
    seen: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE saved_searches SET last_notified_at = $3 WHERE id = $1 AND last_notified_at = $2",
    )
    .bind(id)
    .bind(seen)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// `(email, display_name)`, for a confirmed address only: an unconfirmed one may not be the
/// account holder's.
pub async fn verified_recipient(pool: &PgPool, user_id: Uuid) -> Result<Option<(String, String)>> {
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT email, display_name FROM users WHERE id = $1 AND email_verified_at IS NOT NULL",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_search_keys_own_violation_is_a_duplicate() {
        assert!(is_duplicate_search(Some("23505"), Some(UNIQUE_CONSTRAINT)));
        assert!(!is_duplicate_search(
            Some("23505"),
            Some("saved_searches_pkey")
        ));
        assert!(!is_duplicate_search(Some("23503"), Some(UNIQUE_CONSTRAINT)));
        assert!(!is_duplicate_search(Some("23505"), None));
        assert!(!is_duplicate_search(None, None));
    }
}
