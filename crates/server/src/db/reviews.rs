//! Deal reviews.
//!
//! A rating follows somebody around the marketplace after the deal ends, so the rules are narrow:
//! only a participant, only against a completed deal, only once per person per deal — enforced by
//! `UNIQUE (match_id, reviewer_id)` and mapped here to a 409 rather than a 500. There is no
//! counter column; `rating_avg` and `rating_count` are computed from these rows by
//! `db::users::get_profile`.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use komun_core::models::MatchStatus;

use super::conversations::{lock_status, DealStep};

pub const ALREADY_REVIEWED: &str = "you have already reviewed this deal";

#[derive(Serialize, Clone, Debug, FromRow)]
pub struct ReviewRow {
    pub id: Uuid,
    pub match_id: Uuid,
    pub reviewer_id: Uuid,
    pub reviewee_id: Uuid,
    pub rating: i16,
    pub body: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A review as it appears on somebody's profile. Reviews are attributed: the reviewer's id and
/// name travel with every row, because an unattributed rating is a number a marketplace cannot
/// act on.
#[derive(Serialize, Clone, Debug, FromRow)]
pub struct ReviewView {
    pub id: Uuid,
    pub match_id: Uuid,
    pub reviewer_id: Uuid,
    pub reviewer_display_name: String,
    pub rating: i16,
    pub body: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Whether a thread in `current` may be reviewed. Pure so the rule can be pinned without a
/// database; the status is read under the row lock inside [`create`]. Only a completed deal is
/// reviewable — otherwise a proposal or a withdrawn thread could be a one-star review of a
/// stranger.
pub fn check_reviewable(current: MatchStatus) -> Result<(), String> {
    if current == MatchStatus::Completed {
        return Ok(());
    }
    // The status is named because "not completed" tells the caller they were wrong but not what
    // to do next.
    Err(format!("this deal is not completed (status: {current})"))
}

/// Write one review, or say why not. `reviewee_id` is supplied by this function's caller, not the
/// HTTP client: the handler derives it from the thread (`Thread::other_participant`).
pub async fn create(
    pool: &PgPool,
    match_id: Uuid,
    reviewer_id: Uuid,
    reviewee_id: Uuid,
    rating: i16,
    body: Option<&str>,
) -> Result<DealStep<ReviewRow>> {
    let mut tx = pool.begin().await?;

    let status = lock_status(&mut tx, match_id).await?;
    if let Err(why) = check_reviewable(status) {
        return Ok(DealStep::Conflict(why));
    }

    let inserted = sqlx::query_as::<_, ReviewRow>(
        r#"INSERT INTO deal_reviews (id, match_id, reviewer_id, reviewee_id, rating, body, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           RETURNING id, match_id, reviewer_id, reviewee_id, rating, body, created_at"#,
    )
    .bind(Uuid::now_v7())
    .bind(match_id)
    .bind(reviewer_id)
    .bind(reviewee_id)
    .bind(rating)
    .bind(body)
    .bind(Utc::now())
    .fetch_one(&mut *tx)
    .await;

    let row = match inserted {
        Ok(row) => row,
        // The duplicate is caught by the database, not a SELECT first: a check-then-insert has a
        // window two concurrent requests both pass. This arm exists to map 23505, so it reads as
        // "you already did this" rather than an internal error.
        Err(e) if is_unique_violation(&e) => {
            return Ok(DealStep::Conflict(ALREADY_REVIEWED.to_string()))
        }
        Err(e) => return Err(e.into()),
    };

    tx.commit().await?;
    Ok(DealStep::Done(row))
}

/// Somebody's reviews, newest first. `id` breaks the tie because `created_at` is
/// microsecond-resolution and Postgres may reorder equal timestamps — which under LIMIT/OFFSET
/// means a row on two pages or none.
pub async fn list_for_user(
    pool: &PgPool,
    reviewee_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<ReviewView>> {
    let rows = sqlx::query_as::<_, ReviewView>(
        r#"SELECT r.id, r.match_id, r.reviewer_id,
                  u.display_name AS reviewer_display_name,
                  r.rating, r.body, r.created_at
           FROM deal_reviews r
           JOIN users u ON u.id = r.reviewer_id
           WHERE r.reviewee_id = $1
           ORDER BY r.created_at DESC, r.id DESC
           LIMIT $2 OFFSET $3"#,
    )
    .bind(reviewee_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// `23505` is Postgres' unique-violation SQLSTATE; `deal_reviews` carries exactly one unique
/// constraint, so on this table the code identifies it unambiguously.
fn is_unique_violation(e: &sqlx::Error) -> bool {
    match e {
        sqlx::Error::Database(db) => db.code().is_some_and(|code| code == "23505"),
        _ => false,
    }
}
