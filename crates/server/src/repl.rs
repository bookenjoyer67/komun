use tokio::io::{AsyncBufReadExt, BufReader};
use uuid::Uuid;

use crate::auth::record_audit;
use crate::AppState;

pub async fn run_repl(state: AppState) {
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    print_banner(&state).await;

    loop {
        eprint!("\x1b[1;36mkomun>\x1b[0m ");

        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(_) => break,
        };

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "help" | "?" => print_help(),
            "stats" => cmd_stats(&state).await,
            "list-users" | "users" => cmd_list_users(&state).await,
            "list-directory" | "directory" => cmd_list_directory(&state).await,
            "add-superadmin" => {
                if parts.len() < 2 {
                    print_usage("add-superadmin");
                } else {
                    cmd_add_superadmin(&state, &parts[1..].join(" ")).await;
                }
            }
            "remove-superadmin" => {
                if parts.len() < 2 {
                    print_usage("remove-superadmin");
                } else {
                    cmd_remove_superadmin(&state, &parts[1..].join(" ")).await;
                }
            }
            "purge-expired" => cmd_purge_expired(&state).await,
            "ban-user" => {
                if parts.len() < 2 {
                    print_usage("ban-user");
                } else {
                    cmd_ban_user(&state, &parts[1..].join(" ")).await;
                }
            }
            "quit" | "exit" | "q" => {
                println!("Shutting down...");
                std::process::exit(0);
            }
            other => println!(
                "Unknown command: {}. Type 'help' for available commands.",
                other
            ),
        }
    }
}

async fn print_banner(state: &AppState) {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
    let posts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM posts WHERE status = 'active'")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    println!();
    println!("  \x1b[1mkomun\x1b[0m v{}", env!("CARGO_PKG_VERSION"));
    println!(
        "  node: {} | {} users | {} active posts",
        state.config.node.name, users, posts
    );
    if state.config.discovery.directory_enabled {
        println!("  directory: enabled");
    }
    println!();
}

fn print_help() {
    println!("Available commands:");
    println!("  help                         Show this message");
    println!("  stats                        Server statistics");
    println!("  list-users                   List all registered users");
    println!("  list-directory               List directory entries");
    println!("  add-superadmin <id|email>    Promote a user to superadmin");
    println!("  remove-superadmin <id|email> Demote a superadmin to user");
    println!("  purge-expired                Remove expired posts");
    println!("  ban-user <id|email>          Delete a user");
    println!("  quit                         Shutdown the server");
}

fn print_usage(command: &str) {
    println!("Usage: {command} <user id (UUID) or email address>");
}

async fn cmd_stats(state: &AppState) {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
    let posts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM posts WHERE status = 'active'")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
    let matches: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM matches")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
    let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
    let dir: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM directory_entries")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    println!("Users:         {}", users);
    println!("Active posts:  {}", posts);
    println!("Matches:       {}", matches);
    println!("Messages:      {}", messages);
    println!("Directory:     {}", dir);
}

async fn cmd_list_users(state: &AppState) {
    let rows: Vec<(uuid::Uuid, String, String, chrono::DateTime<chrono::Utc>)> =
        sqlx::query_as("SELECT id, display_name, role, created_at FROM users ORDER BY created_at")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

    if rows.is_empty() {
        println!("No users registered.");
        return;
    }

    println!("{:<38} {:<20} {:<12} Created", "ID", "Name", "Role");
    for (id, name, role, created) in &rows {
        println!(
            "{:<38} {:<20} {:<12} {}",
            id,
            name,
            role,
            created.format("%Y-%m-%d")
        );
    }
}

async fn cmd_list_directory(state: &AppState) {
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT url, name, location_name FROM directory_entries ORDER BY registered_at",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    if rows.is_empty() {
        println!("No directory entries.");
        return;
    }

    for (url, name, loc) in &rows {
        println!(
            "{} — {} ({})",
            url,
            name,
            loc.as_deref().unwrap_or("no location")
        );
    }
}

/// A display name is never a target: names are not unique, so a name-matched write can land on
/// the wrong account or on every account that shares it.
enum Target {
    Id(Uuid),
    Email(String),
}

fn parse_target(raw: &str) -> Option<Target> {
    let raw = raw.trim();
    if let Ok(id) = Uuid::parse_str(raw) {
        return Some(Target::Id(id));
    }
    if raw.contains('@') && !raw.contains(char::is_whitespace) {
        return Some(Target::Email(raw.to_lowercase()));
    }
    None
}

/// The account a command acts on, or `None` once the reason has been printed.
async fn target_id(state: &AppState, raw: &str, command: &str) -> Option<Uuid> {
    let Some(target) = parse_target(raw) else {
        print_usage(command);
        return None;
    };
    let email = match target {
        Target::Id(id) => return Some(id),
        Target::Email(email) => email,
    };
    let found = sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(&state.pool)
        .await;
    match found {
        Ok(Some(id)) => Some(id),
        Ok(None) => {
            println!("No such user");
            None
        }
        Err(e) => {
            db_failed(e);
            None
        }
    }
}

/// The error text goes to the log only; the terminal may be shared or recorded.
fn db_failed(e: sqlx::Error) {
    tracing::error!("repl: database error: {e}");
    println!("Database error; see the server log");
}

/// The operator at the console has no account, so the actor is NULL and `via` names the path.
async fn audit_repl(state: &AppState, action: &str, subject: Uuid, mut detail: serde_json::Value) {
    detail["via"] = serde_json::json!("repl");
    record_audit(&state.pool, None, action, Some(subject), detail).await;
}

async fn cmd_add_superadmin(state: &AppState, raw: &str) {
    let Some(id) = target_id(state, raw, "add-superadmin").await else {
        return;
    };
    let previous = sqlx::query_scalar::<_, String>(
        "UPDATE users u SET role = 'superadmin' FROM users old
         WHERE u.id = $1 AND old.id = $1
         RETURNING old.role",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await;

    match previous {
        Ok(Some(previous)) => {
            let detail = serde_json::json!({ "from": previous, "to": "superadmin" });
            audit_repl(state, "admin.role_change", id, detail).await;
            println!("{id} promoted to superadmin");
        }
        Ok(None) => println!("No such user"),
        Err(e) => db_failed(e),
    }
}

async fn cmd_remove_superadmin(state: &AppState, raw: &str) {
    let Some(id) = target_id(state, raw, "remove-superadmin").await else {
        return;
    };
    let previous = sqlx::query_scalar::<_, String>(
        "UPDATE users u SET role = 'user' FROM users old
         WHERE u.id = $1 AND old.id = $1 AND old.role = 'superadmin'
         RETURNING old.role",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await;

    match previous {
        Ok(Some(previous)) => {
            let detail = serde_json::json!({ "from": previous, "to": "user" });
            audit_repl(state, "admin.role_change", id, detail).await;
            println!("{id} demoted to user");
        }
        Ok(None) => println!("No superadmin with that id or email"),
        Err(e) => db_failed(e),
    }
}

async fn cmd_purge_expired(state: &AppState) {
    let result = sqlx::query(
        "UPDATE posts SET status = 'expired', updated_at = now() WHERE expires_at < now() AND status = 'active'"
    )
    .execute(&state.pool).await;

    match result {
        Ok(r) => println!("Expired {} posts", r.rows_affected()),
        Err(e) => println!("Error: {}", e),
    }
}

async fn cmd_ban_user(state: &AppState, raw: &str) {
    let Some(id) = target_id(state, raw, "ban-user").await else {
        return;
    };
    let deleted = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await;

    match deleted {
        Ok(r) if r.rows_affected() > 0 => {
            audit_repl(state, "admin.delete_user", id, serde_json::json!({})).await;
            println!("User {id} deleted");
        }
        Ok(_) => println!("No such user"),
        Err(e) => db_failed(e),
    }
}

/// Function-level only: the stdin loop, argument splitting and printed output are not exercised.
///
/// Needs a live Postgres; run on the host against a disposable database with
/// `KOMUN_TEST_DATABASE_URL=postgres://... cargo test -p komun-server repl -- --ignored`.
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sqlx::postgres::PgPoolOptions;
    use sqlx::PgPool;

    use super::*;
    use crate::config::Config;
    use crate::{rate_limit, sessions};

    const DATABASE_ENV: &str = "KOMUN_TEST_DATABASE_URL";

    /// Without the variable the test panics rather than passing unearned.
    async fn live_state() -> AppState {
        let Ok(url) = std::env::var(DATABASE_ENV) else {
            panic!("{DATABASE_ENV} is not set: point it at a disposable Postgres database");
        };
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await
            .expect("connect to the test database");
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("apply migrations to the test database");
        AppState {
            pool,
            config: Arc::new(Config::default()),
            rate_limiter: Arc::new(rate_limit::RateLimiter::new()),
            mailer: Arc::new(None),
            trusted_proxies: Arc::new(Vec::new()),
            salt_pepper: Arc::new(sessions::generate_pepper()),
        }
    }

    struct Seeded {
        id: Uuid,
        email: String,
    }

    /// The email is unique per call, the display name is not.
    async fn seed_user(pool: &PgPool, role: &str, display_name: &str) -> Seeded {
        let id = Uuid::now_v7();
        let email = format!("r14-repl-{id}@test.invalid");
        sqlx::query(
            "INSERT INTO users (id, email, email_verified_at, password_hash, auth_salt, display_name, role)
             VALUES ($1, $2, now(), 'unused', $3, $4, $5)",
        )
        .bind(id)
        .bind(&email)
        .bind(vec![0x5a_u8; 16])
        .bind(display_name)
        .bind(role)
        .execute(pool)
        .await
        .expect("insert test user");
        Seeded { id, email }
    }

    async fn role_of(pool: &PgPool, id: Uuid) -> Option<String> {
        sqlx::query_scalar("SELECT role FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .expect("read role")
    }

    async fn audit_rows(
        pool: &PgPool,
        action: &str,
        subject: Uuid,
    ) -> Vec<(Option<Uuid>, Option<String>)> {
        sqlx::query_as(
            "SELECT actor_id, detail->>'via' FROM audit_events
             WHERE action = $1 AND subject_id = $2",
        )
        .bind(action)
        .bind(subject)
        .fetch_all(pool)
        .await
        .expect("read audit rows")
    }

    #[tokio::test]
    #[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
    async fn add_superadmin_by_id_promotes_that_user() {
        let state = live_state().await;
        let user = seed_user(&state.pool, "user", "R14 repl").await;

        cmd_add_superadmin(&state, &user.id.to_string()).await;

        let role = role_of(&state.pool, user.id).await;
        assert_eq!(role.as_deref(), Some("superadmin"));
    }

    #[tokio::test]
    #[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
    async fn add_superadmin_by_email_promotes_that_user() {
        let state = live_state().await;
        let user = seed_user(&state.pool, "user", "R14 repl").await;

        cmd_add_superadmin(&state, &user.email).await;

        let role = role_of(&state.pool, user.id).await;
        assert_eq!(role.as_deref(), Some("superadmin"));
    }

    #[tokio::test]
    #[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
    async fn remove_superadmin_demotes_only_the_named_account() {
        let state = live_state().await;
        let shared = format!("r14repl{}", Uuid::now_v7().simple());
        let target = seed_user(&state.pool, "superadmin", &shared).await;
        let namesake = seed_user(&state.pool, "superadmin", &shared).await;

        cmd_remove_superadmin(&state, &target.id.to_string()).await;

        let demoted = role_of(&state.pool, target.id).await;
        assert_eq!(demoted.as_deref(), Some("user"));
        let kept = role_of(&state.pool, namesake.id).await;
        assert_eq!(
            kept.as_deref(),
            Some("superadmin"),
            "an account that only shares the display name must keep its role"
        );
    }

    #[tokio::test]
    #[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
    async fn a_repl_role_change_is_audited_once() {
        let state = live_state().await;
        let user = seed_user(&state.pool, "user", "R14 repl").await;

        cmd_add_superadmin(&state, &user.id.to_string()).await;

        let rows = audit_rows(&state.pool, "admin.role_change", user.id).await;
        assert_eq!(rows, vec![(None, Some("repl".to_string()))]);
    }

    #[tokio::test]
    #[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
    async fn ban_by_email_deletes_the_user_and_is_audited_once() {
        let state = live_state().await;
        let user = seed_user(&state.pool, "user", "R14 repl").await;

        cmd_ban_user(&state, &user.email).await;

        let role = role_of(&state.pool, user.id).await;
        assert_eq!(role, None, "the user is gone");
        let rows = audit_rows(&state.pool, "admin.delete_user", user.id).await;
        assert_eq!(rows, vec![(None, Some("repl".to_string()))]);
    }

    #[tokio::test]
    #[ignore = "requires KOMUN_TEST_DATABASE_URL (live Postgres); run with --ignored"]
    async fn a_display_name_is_not_a_target() {
        let state = live_state().await;
        let name = format!("r14repl{}", Uuid::now_v7().simple());
        let user = seed_user(&state.pool, "user", &name).await;

        cmd_add_superadmin(&state, &name).await;

        let role = role_of(&state.pool, user.id).await;
        assert_eq!(role.as_deref(), Some("user"));
        assert!(audit_rows(&state.pool, "admin.role_change", user.id)
            .await
            .is_empty());
    }
}
