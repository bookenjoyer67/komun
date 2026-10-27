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

/// Peers and the directory read `name`; `city` and `state` are additive.
#[derive(Serialize)]
struct NodeLocation {
    name: Option<String>,
    city: Option<String>,
    state: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/node", get(get_node_info))
        .with_state(state)
}

fn non_blank(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(String::from)
}

async fn get_node_info(State(state): State<AppState>) -> Json<NodeInfo> {
    let config = &state.config;
    let node = &config.node;
    let city = non_blank(node.location_city.as_deref());
    let region = non_blank(node.location_state.as_deref());
    let location = if node.location_name.is_some()
        || node.location_lat.is_some()
        || city.is_some()
        || region.is_some()
    {
        Some(NodeLocation {
            name: node.location_name.clone(),
            city,
            state: region,
            lat: node.location_lat,
            lon: node.location_lon,
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
