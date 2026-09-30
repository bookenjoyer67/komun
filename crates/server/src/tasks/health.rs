use std::time::Duration;
use tokio::time;

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

async fn check_registered_servers(state: &AppState) -> anyhow::Result<()> {
    let entries: Vec<(String,)> = sqlx::query_as("SELECT url FROM directory_entries")
        .fetch_all(&state.pool)
        .await?;

    let client = reqwest::Client::new();

    for (url,) in entries {
        let node_url = format!("{}/api/node", url);
        match client
            .get(&node_url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(res) if res.status().is_success() => {
                if let Ok(info) = res.json::<serde_json::Value>().await {
                    sqlx::query(
                        "UPDATE directory_entries SET last_seen = now(), name = $2 WHERE url = $1",
                    )
                    .bind(&url)
                    .bind(info["name"].as_str().unwrap_or(""))
                    .execute(&state.pool)
                    .await
                    .ok();
                }
            }
            _ => {
                tracing::debug!("server {} unreachable", url);
            }
        }
    }

    sqlx::query("DELETE FROM directory_entries WHERE last_seen < now() - interval '7 days'")
        .execute(&state.pool)
        .await?;

    Ok(())
}
