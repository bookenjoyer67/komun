use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Serialize, FromRow)]
pub struct Notification {
    pub id: Uuid,
    pub user_id: Uuid,
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub link: Option<String>,
    pub read: Option<bool>,
    pub created_at: DateTime<Utc>,
}

pub async fn create(
    pool: &PgPool,
    user_id: Uuid,
    kind: &str,
    title: &str,
    body: Option<&str>,
    link: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO notifications (id, user_id, kind, title, body, link) VALUES ($1, $2, $3, $4, $5, $6)"
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(kind)
    .bind(title)
    .bind(body)
    .bind(link)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list(pool: &PgPool, user_id: Uuid) -> Result<Vec<Notification>> {
    let rows = sqlx::query_as::<_, Notification>(
        "SELECT id, user_id, kind, title, body, link, read, created_at FROM notifications WHERE user_id = $1 ORDER BY created_at DESC LIMIT 50"
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn unread_count(pool: &PgPool, user_id: Uuid) -> Result<i64> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM notifications WHERE user_id = $1 AND read = false",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn mark_read(pool: &PgPool, notification_id: Uuid, user_id: Uuid) -> Result<()> {
    sqlx::query("UPDATE notifications SET read = true WHERE id = $1 AND user_id = $2")
        .bind(notification_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn mark_all_read(pool: &PgPool, user_id: Uuid) -> Result<()> {
    sqlx::query("UPDATE notifications SET read = true WHERE user_id = $1 AND read = false")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}
