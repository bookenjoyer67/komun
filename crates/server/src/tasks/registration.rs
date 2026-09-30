use std::time::Duration;
use tokio::time;

use crate::AppState;

pub async fn registration_loop(state: AppState) {
    loop {
        if let Err(e) = register_with_directory(&state).await {
            tracing::warn!("directory registration failed: {}", e);
        }
        time::sleep(Duration::from_secs(3600)).await;
    }
}

async fn register_with_directory(state: &AppState) -> anyhow::Result<()> {
    let config = &state.config;
    let directory_url = config
        .discovery
        .directory_url
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no directory_url configured"))?;

    let payload = serde_json::json!({
        "url": config.public_url(),
        "name": config.node.name,
        "description": config.node.description,
        "location_name": config.node.location_name,
        "location_lat": config.node.location_lat,
        "location_lon": config.node.location_lon,
        "version": env!("CARGO_PKG_VERSION"),
        "open_registration": config.open_registration(),
    });

    let client = reqwest::Client::new();
    let url = format!(
        "{}/api/directory/register",
        directory_url.trim_end_matches('/')
    );

    let res = client
        .post(&url)
        .json(&payload)
        .timeout(Duration::from_secs(10))
        .send()
        .await?;

    if res.status().is_success() {
        tracing::info!("registered with directory at {}", directory_url);
    } else {
        tracing::warn!("directory registration returned {}", res.status());
    }

    Ok(())
}
