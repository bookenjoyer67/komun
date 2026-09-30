use axum::{extract::Query, Json};
use serde::{Deserialize, Serialize};

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
    Query(query): Query<LinkPreviewQuery>,
) -> Result<Json<LinkPreview>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let client = reqwest::Client::new();
    let url = query.url;

    let resp = client.get(&url).send().await.map_err(|e| {
        (
            axum::http::StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("fetch failed: {}", e)})),
        )
    })?;

    if !resp.status().is_success() {
        return Err((
            axum::http::StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": "upstream returned error"})),
        ));
    }

    let html = resp.text().await.map_err(|e| {
        (
            axum::http::StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("read failed: {}", e)})),
        )
    })?;

    let title = extract_meta(&html, "og:title").or_else(|| extract_tag(&html, "title"));
    let description =
        extract_meta(&html, "og:description").or_else(|| extract_meta(&html, "description"));
    let image = extract_meta(&html, "og:image");

    Ok(Json(LinkPreview {
        url,
        title,
        description,
        image,
    }))
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
