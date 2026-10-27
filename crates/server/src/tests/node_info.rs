//! `GET /node` reads config only, so no database is needed.

use axum::http::{Method, StatusCode};
use serde_json::Value;

use super::support::{app, send, state_with, unreachable_pool};

async fn node_location(config_toml: &str) -> Value {
    let app = app(state_with(unreachable_pool(), config_toml));
    let (status, body) = send(&app, Method::GET, "/node", None, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["location"].clone()
}

#[tokio::test]
async fn get_node_exposes_city_and_state() {
    let location = node_location(
        r#"
[node]
name = "stl"
location_name = "St. Louis, Missouri"
location_lat = 38.627
location_lon = -90.199
location_city = "St. Louis"
location_state = "MO"
"#,
    )
    .await;
    assert_eq!(location["city"], "St. Louis", "{location}");
    assert_eq!(location["state"], "MO", "{location}");
    assert_eq!(location["name"], "St. Louis, Missouri", "{location}");
    assert_eq!(location["lat"], 38.627, "{location}");
    assert_eq!(location["lon"], -90.199, "{location}");
}

#[tokio::test]
async fn get_node_with_only_a_city_reports_a_location() {
    let location = node_location(
        r#"
[node]
location_city = "St. Louis"
location_state = "   "
"#,
    )
    .await;
    assert_eq!(location["city"], "St. Louis", "{location}");
    assert!(location["state"].is_null(), "a blank state is absent: {location}");
    assert!(location["name"].is_null(), "{location}");
}

/// The directory and peers read `location.name`; adding city and state must not displace it.
#[tokio::test]
async fn get_node_still_reports_location_name() {
    let location = node_location(
        r#"
[node]
location_name = "St. Louis, Missouri"
"#,
    )
    .await;
    assert_eq!(location["name"], "St. Louis, Missouri", "{location}");
    assert!(location["city"].is_null(), "{location}");
    assert!(location["state"].is_null(), "{location}");
}

#[tokio::test]
async fn get_node_without_location_reports_null() {
    let location = node_location("[node]\nname = \"unlocated\"\n").await;
    assert!(location.is_null(), "{location}");
}
