use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    middleware,
    routing::{delete, get, patch},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::StatusError;
use crate::auth::{record_audit, require_superadmin, AuthUser};
use crate::db::sessions as session_db;
use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/admin/stats", get(stats))
        .route("/admin/users", get(list_users))
        .route("/admin/users/{id}", delete(delete_user))
        .route("/admin/users/{id}/role", patch(change_role))
        // A compromised account has to be kickable without waiting for its sessions to expire.
        .route(
            "/admin/users/{id}/sessions",
            get(list_user_sessions).delete(revoke_user_sessions),
        )
        .route("/admin/directory", get(list_directory))
        .route("/admin/directory/{url}", delete(remove_directory_entry))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_superadmin,
        ))
        .with_state(state)
}

async fn stats(State(state): State<AppState>) -> Result<Json<serde_json::Value>, StatusError> {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await?;
    let active_posts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM posts WHERE status = 'active'")
            .fetch_one(&state.pool)
            .await?;
    let total_posts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM posts")
        .fetch_one(&state.pool)
        .await?;
    let matches: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM matches")
        .fetch_one(&state.pool)
        .await?;
    let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&state.pool)
        .await?;
    let directory: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM directory_entries")
        .fetch_one(&state.pool)
        .await?;

    Ok(Json(serde_json::json!({
        "users": users,
        "active_posts": active_posts,
        "total_posts": total_posts,
        "matches": matches,
        "messages": messages,
        "directory_entries": directory,
    })))
}

#[derive(Serialize, FromRow)]
struct AdminUser {
    id: Uuid,
    display_name: String,
    role: String,
    last_seen: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

async fn list_users(State(state): State<AppState>) -> Result<Json<Vec<AdminUser>>, StatusError> {
    let users = sqlx::query_as::<_, AdminUser>(
        "SELECT id, display_name, role, last_seen, created_at FROM users ORDER BY created_at DESC",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(users))
}

async fn delete_user(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusError> {
    if id == auth.user_id {
        return Err(StatusError::with_status(
            StatusCode::BAD_REQUEST,
            "cannot delete yourself",
        ));
    }
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({"status": "deleted"})))
}

#[derive(Deserialize)]
struct ChangeRoleRequest {
    role: String,
}

/// Promote or demote an account. Superadmin only; the role backing that check is re-read on every
/// request, so a demotion lands on the demoted admin's very next call.
async fn change_role(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(input): Json<ChangeRoleRequest>,
) -> Result<Json<serde_json::Value>, StatusError> {
    if id == auth.user_id {
        return Err(StatusError::with_status(
            StatusCode::BAD_REQUEST,
            "cannot change your own role",
        ));
    }
    let valid_roles = ["user", "admin", "superadmin"];
    if !valid_roles.contains(&input.role.as_str()) {
        return Err(StatusError::with_status(
            StatusCode::BAD_REQUEST,
            "invalid role: must be user, admin or superadmin",
        ));
    }

    // Read the old role in the same statement that writes the new one; as two queries a concurrent
    // change could slip between them and be audited as though it never happened.
    let previous: Option<String> = sqlx::query_scalar(
        "UPDATE users u SET role = $2 FROM users old
         WHERE u.id = $1 AND old.id = $1
         RETURNING old.role",
    )
    .bind(id)
    .bind(&input.role)
    .fetch_optional(&state.pool)
    .await?;

    let Some(previous) = previous else {
        return Err(StatusError::with_status(
            StatusCode::NOT_FOUND,
            "no such user",
        ));
    };

    // Admin changes are audited whether or not the role moved; the no-op is itself evidence of
    // intent.
    record_audit(
        &state.pool,
        Some(auth.user_id),
        "admin.role_change",
        Some(id),
        serde_json::json!({ "from": previous, "to": input.role }),
    )
    .await;

    Ok(Json(serde_json::json!({
        "status": "updated",
        "role": input.role,
        "previous_role": previous,
    })))
}

/// Where an account is signed in. Like `/auth/sessions`, it omits the token hash and raw user
/// agent.
#[derive(Serialize)]
struct AdminSession {
    id: Uuid,
    device_label: Option<String>,
    ip: Option<String>,
    created_at: DateTime<Utc>,
    last_used_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

async fn list_user_sessions(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<AdminSession>>, StatusError> {
    let rows = session_db::list_for_user(&state.pool, id).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| AdminSession {
                id: r.id,
                device_label: r.device_label,
                ip: r.ip,
                created_at: r.created_at,
                last_used_at: r.last_used_at,
                expires_at: r.expires_at,
            })
            .collect(),
    ))
}

/// Sign an account out everywhere. Revocation is a column update the middleware checks on every
/// request, so the effect is immediate rather than at token expiry.
async fn revoke_user_sessions(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusError> {
    let revoked = session_db::revoke_all(&state.pool, id).await?;

    record_audit(
        &state.pool,
        Some(auth.user_id),
        "admin.revoke_sessions",
        Some(id),
        serde_json::json!({ "count": revoked }),
    )
    .await;

    Ok(Json(serde_json::json!({ "revoked": revoked })))
}

#[derive(Serialize, FromRow)]
struct AdminDirectoryEntry {
    url: String,
    name: String,
    location_name: Option<String>,
    last_seen: DateTime<Utc>,
    registered_at: DateTime<Utc>,
}

async fn list_directory(
    State(state): State<AppState>,
) -> Result<Json<Vec<AdminDirectoryEntry>>, StatusError> {
    let entries = sqlx::query_as::<_, AdminDirectoryEntry>(
        "SELECT url, name, location_name, last_seen, registered_at FROM directory_entries ORDER BY registered_at DESC"
    )
    .fetch_all(&state.pool).await?;
    Ok(Json(entries))
}

async fn remove_directory_entry(
    State(state): State<AppState>,
    Path(url): Path<String>,
) -> Result<Json<serde_json::Value>, StatusError> {
    sqlx::query("DELETE FROM directory_entries WHERE url = $1")
        .bind(&url)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({"status": "removed"})))
}
