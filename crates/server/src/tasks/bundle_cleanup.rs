use std::time::Duration;
use tokio::time;

use crate::AppState;

pub async fn user_cleanup_loop(state: AppState) {
    time::sleep(Duration::from_secs(300)).await;

    loop {
        if let Err(e) = cleanup_inactive_users(&state).await {
            tracing::warn!("user cleanup error: {}", e);
        }
        time::sleep(Duration::from_secs(86400)).await;
    }
}

async fn cleanup_inactive_users(state: &AppState) -> anyhow::Result<()> {
    let result = sqlx::query("DELETE FROM users WHERE last_seen < now() - interval '90 days'")
        .execute(&state.pool)
        .await?;

    if result.rows_affected() > 0 {
        tracing::info!("deleted {} inactive user accounts", result.rows_affected());
    }

    Ok(())
}
