use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::StatusError;
use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/search", get(search))
        .route("/search/users", get(search_users))
        .with_state(state)
}

#[derive(Deserialize)]
struct SearchParams {
    q: String,
    kind: Option<String>,
    limit: Option<i64>,
}

#[derive(Serialize, FromRow)]
struct SearchResult {
    id: Uuid,
    kind: String,
    category: String,
    title: String,
    body: Option<String>,
    location_name: Option<String>,
    urgency: Option<String>,
    status: String,
    // `posts.tags` is `TEXT[]`, so it must decode as `Vec<String>`; sqlx decodes
    // `serde_json::Value` only from json/jsonb.
    tags: Option<Vec<String>>,
    author_id: Uuid,
    verified_by: Option<Uuid>,
    created_at: DateTime<Utc>,
    rank: f32,
}

#[derive(Serialize, FromRow)]
struct UserSearchResult {
    id: Uuid,
    display_name: String,
    role: String,
    endorsement_count: Option<i64>,
}

async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Vec<SearchResult>>, StatusError> {
    let limit = params.limit.unwrap_or(20).clamp(1, 50);
    let query = params.q.trim();
    if query.is_empty() {
        return Ok(Json(vec![]));
    }

    let results = sqlx::query_as::<_, SearchResult>(
        r#"SELECT p.id, p.kind, p.category, p.title, p.body,
           p.location_name, p.urgency, p.status, p.tags, p.author_id, p.verified_by,
           p.created_at,
           ts_rank(p.search_vector, plainto_tsquery('english', $1)) AS rank
           FROM posts p
           WHERE p.search_vector @@ plainto_tsquery('english', $1)
             AND p.status = 'active'
             AND p.visibility = 'public'
             AND ($2::text IS NULL OR p.kind = $2)
           ORDER BY rank DESC
           LIMIT $3"#,
    )
    .bind(query)
    .bind(params.kind.as_deref())
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(results))
}

async fn search_users(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Vec<UserSearchResult>>, StatusError> {
    let query = params.q.trim();
    if query.is_empty() {
        return Ok(Json(vec![]));
    }

    let pattern = format!("%{}%", query);
    let results = sqlx::query_as::<_, UserSearchResult>(
        r#"SELECT u.id, u.display_name, u.role,
           (SELECT COUNT(*) FROM endorsements WHERE endorsee_id = u.id) AS endorsement_count
           FROM users u
           WHERE u.display_name ILIKE $1
           ORDER BY u.display_name
           LIMIT 20"#,
    )
    .bind(&pattern)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(results))
}
