use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Endorsement {
    pub id: Uuid,
    pub endorser_id: Uuid,
    pub endorsee_id: Uuid,
    pub note: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct EndorsementWithName {
    pub id: Uuid,
    pub endorser_id: Uuid,
    pub endorser_name: String,
    pub note: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn create(
    pool: &PgPool,
    endorser_id: Uuid,
    endorsee_id: Uuid,
    note: Option<String>,
) -> Result<Endorsement, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query_as::<_, Endorsement>(
        "INSERT INTO endorsements (id, endorser_id, endorsee_id, note) VALUES ($1, $2, $3, $4) RETURNING id, endorser_id, endorsee_id, note, created_at"
    )
    .bind(id)
    .bind(endorser_id)
    .bind(endorsee_id)
    .bind(&note)
    .fetch_one(pool)
    .await
}

pub async fn remove(
    pool: &PgPool,
    endorser_id: Uuid,
    endorsee_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let rows = sqlx::query("DELETE FROM endorsements WHERE endorser_id = $1 AND endorsee_id = $2")
        .bind(endorser_id)
        .bind(endorsee_id)
        .execute(pool)
        .await?;
    Ok(rows.rows_affected() > 0)
}

pub async fn list_for_user(
    pool: &PgPool,
    endorsee_id: Uuid,
) -> Result<Vec<EndorsementWithName>, sqlx::Error> {
    sqlx::query_as::<_, EndorsementWithName>(
        r#"SELECT e.id, e.endorser_id, e.note, e.created_at,
           u.display_name as endorser_name
           FROM endorsements e
           JOIN users u ON u.id = e.endorser_id
           WHERE e.endorsee_id = $1
           ORDER BY e.created_at DESC"#,
    )
    .bind(endorsee_id)
    .fetch_all(pool)
    .await
}

// A3: `count_for_user` had no callers. Both places that want the number — `api/users.rs`'s
// profile query and `api/search.rs`'s user search — count it in SQL alongside the row they are
// already fetching, rather than making a second round trip.
