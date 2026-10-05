use std::time::Duration;
use tokio::time;

use crate::api::outbound;
use crate::AppState;

pub async fn health_check_loop(state: AppState) {
    time::sleep(Duration::from_secs(60)).await;

    loop {
        if let Err(e) = check_registered_servers(&state).await {
            tracing::warn!("health check error: {}", e);
        }
        time::sleep(Duration::from_secs(900)).await;
    }
}

/// A stored URL the guard refuses is treated as unreachable, so a private or loopback entry ages
/// out through the 7-day sweep below instead of being probed.
async fn check_registered_servers(state: &AppState) -> anyhow::Result<()> {
    let entries: Vec<(String,)> = sqlx::query_as("SELECT url FROM directory_entries")
        .fetch_all(&state.pool)
        .await?;

    for (url,) in entries {
        let node_url = format!("{}/api/node", url);
        let info = match outbound::fetch_public(&node_url, outbound::NODE_INFO).await {
            Ok(body) => serde_json::from_slice::<serde_json::Value>(&body).ok(),
            Err(e) => {
                tracing::debug!("directory peer unreachable: {e}");
                None
            }
        };

        if let Some(info) = info {
            sqlx::query("UPDATE directory_entries SET last_seen = now(), name = $2 WHERE url = $1")
                .bind(&url)
                .bind(info["name"].as_str().unwrap_or(""))
                .execute(&state.pool)
                .await
                .ok();
        }
    }

    sqlx::query("DELETE FROM directory_entries WHERE last_seen < now() - interval '7 days'")
        .execute(&state.pool)
        .await?;

    Ok(())
}
