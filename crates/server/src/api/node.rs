use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;

use crate::AppState;

#[derive(Serialize)]
struct NodeInfo {
    name: String,
    description: String,
    version: String,
    domain: Option<String>,
    location: Option<NodeLocation>,
    listed: bool,
    /// The resolved `[discovery] open_registration`: explicit value, else `[registration] mode`.
    open_registration: bool,
}

#[derive(Serialize)]
struct NodeLocation {
    name: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/node", get(get_node_info))
        .with_state(state)
}

async fn get_node_info(State(state): State<AppState>) -> Json<NodeInfo> {
    let config = &state.config;
    let location = if config.node.location_name.is_some() || config.node.location_lat.is_some() {
        Some(NodeLocation {
            name: config.node.location_name.clone(),
            lat: config.node.location_lat,
            lon: config.node.location_lon,
        })
    } else {
        None
    };

    let domain = config
        .node
        .public_url
        .as_ref()
        .and_then(|url| {
            url.strip_prefix("https://")
                .or_else(|| url.strip_prefix("http://"))
        })
        .and_then(|rest| rest.split('/').next())
        .and_then(|host| host.split(':').next())
        .map(String::from);

    Json(NodeInfo {
        name: config.node.name.clone(),
        description: config.node.description.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        domain,
        location,
        listed: config.discovery.listed,
        open_registration: config.open_registration(),
    })
}
