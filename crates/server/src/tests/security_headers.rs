//! The gates never run nginx, so the nginx half is pinned by reading the sample config as text.

use std::collections::BTreeSet;

use axum::{
    body::Body,
    http::{header, HeaderName, Request},
    middleware,
    response::Response,
    routing::get,
    Router,
};
use tower::ServiceExt;

const API_CSP_DIRECTIVES: [&str; 2] = ["default-src 'none'", "frame-ancestors 'none'"];
const HSTS: &str = "max-age=31536000";
const CSP_INCLUDE: &str = "include /opt/komun/csp.conf;";
const NGINX_CONF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../deploy/nginx-komun.conf");

async fn probe() -> Response {
    let app = Router::new()
        .route("/probe", get(|| async { "ok" }))
        .layer(middleware::from_fn(
            crate::security_headers::security_headers,
        ));
    let request = Request::builder()
        .uri("/probe")
        .body(Body::empty())
        .expect("build request");
    app.oneshot(request)
        .await
        .expect("the router is infallible")
}

fn header_text(response: &Response, name: HeaderName) -> Option<&str> {
    response.headers().get(name)?.to_str().ok()
}

/// Inside `location /`, not at server level: an `add_header` in a location drops every
/// server-level one for that location.
fn nginx_spa_location() -> Vec<String> {
    let conf = std::fs::read_to_string(NGINX_CONF).expect("read the nginx sample");
    let mut lines = conf.lines().map(str::trim);
    assert!(
        lines.by_ref().any(|line| line == "location / {"),
        "no `location /` block"
    );
    lines
        .take_while(|line| *line != "}")
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn api_response_carries_frame_ancestors_none() {
    let response = probe().await;
    let csp = header_text(&response, header::CONTENT_SECURITY_POLICY);
    let directives: BTreeSet<&str> = csp
        .unwrap_or_default()
        .split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .collect();
    assert_eq!(directives, BTreeSet::from(API_CSP_DIRECTIVES), "{csp:?}");
}

#[tokio::test]
async fn api_response_carries_hsts() {
    let response = probe().await;
    assert_eq!(
        header_text(&response, header::STRICT_TRANSPORT_SECURITY),
        Some(HSTS)
    );
}

#[test]
fn nginx_spa_location_includes_per_build_csp() {
    let block = nginx_spa_location();
    assert!(block.iter().any(|line| line == CSP_INCLUDE), "{block:#?}");
}

#[test]
fn nginx_spa_location_sends_hsts() {
    let block = nginx_spa_location();
    let hsts = format!("add_header Strict-Transport-Security \"{HSTS}\" always;");
    assert!(block.contains(&hsts), "{block:#?}");
}
