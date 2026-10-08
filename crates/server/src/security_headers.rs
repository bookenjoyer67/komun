use axum::{
    body::Body,
    http::{header, HeaderValue, Request},
    middleware::Next,
    response::Response,
};

// This server returns JSON and uploaded images, never a document, so its policy grants nothing.
// The SPA's policy, with the build's script hash, is nginx's (`/opt/komun/csp.conf`); copying it
// here would only add script permissions to responses that run none.
pub(crate) const API_CSP: &str = "default-src 'none'; frame-ancestors 'none'";
pub(crate) const HSTS: &str = "max-age=31536000";

pub async fn security_headers(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
    headers.insert("Referrer-Policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "Permissions-Policy",
        HeaderValue::from_static("geolocation=(), microphone=(), camera=()"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(API_CSP),
    );
    headers.insert(
        header::STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static(HSTS),
    );

    response
}
