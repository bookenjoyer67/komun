use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use super::outbound;
use crate::rate_limit::RouteClass;
use crate::AppState;

const URL_NOT_ALLOWED: &str = "url not allowed";
const PREVIEW_UNAVAILABLE: &str = "preview unavailable";

#[derive(Deserialize)]
pub struct LinkPreviewQuery {
    url: String,
}

#[derive(Serialize)]
pub struct LinkPreview {
    url: String,
    title: Option<String>,
    description: Option<String>,
    image: Option<String>,
}

pub async fn link_preview(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<LinkPreviewQuery>,
) -> Response {
    if let Err(limited) = super::per_ip_limit(&state, RouteClass::LinkPreview, peer, &headers) {
        return limited.into_response();
    }

    let html = match outbound::fetch_public(&query.url, outbound::LINK_PREVIEW).await {
        Ok(body) => String::from_utf8_lossy(&body).into_owned(),
        Err(e) if e.is_refused() => return error(StatusCode::BAD_REQUEST, URL_NOT_ALLOWED),
        Err(e) => {
            tracing::debug!("link preview fetch failed: {e}");
            return error(StatusCode::BAD_GATEWAY, PREVIEW_UNAVAILABLE);
        }
    };

    let title = extract_meta(&html, "og:title").or_else(|| extract_tag(&html, "title"));
    let description =
        extract_meta(&html, "og:description").or_else(|| extract_meta(&html, "description"));
    let image = extract_meta(&html, "og:image");

    Json(LinkPreview {
        url: query.url,
        title,
        description,
        image,
    })
    .into_response()
}

fn error(status: StatusCode, message: &'static str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}

fn extract_meta(html: &str, property: &str) -> Option<String> {
    let pattern = format!("property=\"{}\"", property);
    let start = html.find(&pattern)?;
    let after = &html[start + pattern.len()..];
    let content_start = after.find("content=\"")?;
    let after_content = &after[content_start + 9..];
    let content_end = after_content.find('"')?;
    let value = &after_content[..content_end];

    let decoded = value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'");

    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

fn extract_tag(html: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = html.find(&open)?;
    let after = &html[start + open.len()..];
    let end = after.find(&close)?;
    let value = &after[..end];
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}
