use tokio::io::{AsyncBufReadExt, BufReader};

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
                    println!("Usage: add-superadmin <display_name>");
                } else {
                    cmd_add_superadmin(&state, &parts[1..].join(" ")).await;
                }
            }
            "remove-superadmin" => {
                if parts.len() < 2 {
                    println!("Usage: remove-superadmin <display_name>");
                } else {
                    cmd_remove_superadmin(&state, &parts[1..].join(" ")).await;
                }
            }
            "purge-expired" => cmd_purge_expired(&state).await,
            "ban-user" => {
                if parts.len() < 2 {
                    println!("Usage: ban-user <display_name>");
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
    println!("  help                  Show this message");
    println!("  stats                 Server statistics");
    println!("  list-users            List all registered users");
    println!("  list-directory        List directory entries");
    println!("  add-superadmin <name> Promote a user to superadmin");
    println!("  remove-superadmin <n> Demote a superadmin to user");
    println!("  purge-expired         Remove expired posts");
    println!("  ban-user <name>       Delete a user");
    println!("  quit                  Shutdown the server");
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

async fn cmd_add_superadmin(state: &AppState, name: &str) {
    let rows: Vec<(uuid::Uuid, String)> =
        sqlx::query_as("SELECT id, display_name FROM users WHERE LOWER(display_name) = LOWER($1)")
            .bind(name)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

    match rows.len() {
        0 => println!("No user found with name '{}'", name),
        1 => {
            sqlx::query("UPDATE users SET role = 'superadmin' WHERE id = $1")
                .bind(rows[0].0)
                .execute(&state.pool)
                .await
                .ok();
            println!("'{}' promoted to superadmin", rows[0].1);
        }
        n => {
            println!(
                "Found {} users named '{}'. Be more specific or use the full ID:",
                n, name
            );
            for (id, dname) in &rows {
                println!("  {} — {}", id, dname);
            }
        }
    }
}

async fn cmd_remove_superadmin(state: &AppState, name: &str) {
    let result = sqlx::query(
        "UPDATE users SET role = 'user' WHERE LOWER(display_name) = LOWER($1) AND role = 'superadmin'"
    )
    .bind(name)
    .execute(&state.pool).await;

    match result {
        Ok(r) if r.rows_affected() > 0 => println!("'{}' demoted to user", name),
        Ok(_) => println!("No superadmin found with name '{}'", name),
        Err(e) => println!("Error: {}", e),
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

async fn cmd_ban_user(state: &AppState, name: &str) {
    let rows: Vec<(uuid::Uuid, String)> =
        sqlx::query_as("SELECT id, display_name FROM users WHERE LOWER(display_name) = LOWER($1)")
            .bind(name)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

    match rows.len() {
        0 => println!("No user found with name '{}'", name),
        1 => {
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(rows[0].0)
                .execute(&state.pool)
                .await
                .ok();
            println!("User '{}' deleted", rows[0].1);
        }
        n => {
            println!("Found {} users named '{}'. Be more specific:", n, name);
            for (id, dname) in &rows {
                println!("  {} — {}", id, dname);
            }
        }
    }
}
