mod api;
pub mod auth;
mod badges;
pub mod config;
mod db;
mod media;
mod rate_limit;
mod repl;
mod security_headers;
mod sessions;
mod tasks;
#[cfg(test)]
mod tests;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use anyhow::Context;
use axum::{http::header, http::HeaderValue, middleware, Router};
use config::Config;
use sqlx::postgres::PgPoolOptions;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    services::ServeDir,
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub config: Arc<Config>,
    pub rate_limiter: Arc<rate_limit::RateLimiter>,
    /// `None` when `[email]` is unconfigured, which startup only permits while
    /// `require_email_verification` is false.
    pub mailer: Arc<Option<auth::email::Mailer>>,
    pub trusted_proxies: Arc<Vec<IpAddr>>,
    /// Per-process secret behind the decoy salts that keep `GET /auth/salt` from confirming
    /// whether an address has an account here.
    pub salt_pepper: Arc<Vec<u8>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "komun_server=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    dotenvy::dotenv().ok();

    let config = Config::load()
        .context("Failed to load configuration. Copy config.example.toml to config.toml and edit it, or set KOMUN_CONFIG to a custom path.")?;

    // Built before the listener so a misconfigured `[email]` block is a startup failure rather
    // than a surprise at the first signup.
    let mailer = auth::email::Mailer::from_config(&config)
        .context("Failed to build the SMTP mailer from [email]")?;
    if mailer.is_none() {
        tracing::warn!(
            "[email] is not configured: no verification or password-reset mail will be sent"
        );
    }

    let pool = PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .connect(&config.database.url)
        .await
        .with_context(|| {
            // Not logged: a database URL can carry the password.
            "Failed to connect to PostgreSQL. Is PostgreSQL running? Check config.toml [database] url or DATABASE_URL env var."
        })?;

    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .context("Failed to run database migrations. Is the migrations/ directory present and accessible from the working directory?")?;

    let state = AppState {
        pool: pool.clone(),
        config: Arc::new(config.clone()),
        rate_limiter: Arc::new(rate_limit::RateLimiter::new()),
        mailer: Arc::new(mailer),
        trusted_proxies: Arc::new(config.security.trusted_proxy_ips()),
        salt_pepper: Arc::new(sessions::generate_pepper()),
    };

    tasks::spawn_background_tasks(state.clone());
    tokio::spawn(sessions::cleanup_loop(pool.clone()));

    let allowed_origins = &state.config.security.allowed_origins;
    let cors_headers = [header::AUTHORIZATION, header::CONTENT_TYPE];
    let cors = if allowed_origins == "*" {
        CorsLayer::new()
            .allow_origin(AllowOrigin::any())
            .allow_methods(tower_http::cors::Any)
            .allow_headers(cors_headers)
    } else {
        let origins: Vec<HeaderValue> = allowed_origins
            .split(',')
            .filter_map(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    HeaderValue::from_str(trimmed).ok()
                }
            })
            .collect();
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods(tower_http::cors::Any)
            .allow_headers(cors_headers)
    };

    let avatar_dir = std::path::absolute(&state.config.media.avatar_dir)
        .unwrap_or_else(|_| std::path::PathBuf::from(&state.config.media.avatar_dir));
    std::fs::create_dir_all(&avatar_dir).ok();
    let post_img_dir = std::path::absolute(&state.config.media.post_images_dir)
        .unwrap_or_else(|_| std::path::PathBuf::from(&state.config.media.post_images_dir));
    std::fs::create_dir_all(&post_img_dir).ok();
    let app = Router::new()
        .nest("/api", api::router(state.clone()))
        .nest_service("/avatars", ServeDir::new(&avatar_dir))
        .nest_service("/post-images", ServeDir::new(&post_img_dir))
        .layer(cors)
        .layer(middleware::from_fn(security_headers::security_headers))
        .layer(TraceLayer::new_for_http());

    if api::geocode::contact_is_missing() {
        tracing::warn!(
            "no geocode contact: set [geocode] contact or KOMUN_GEOCODE_CONTACT for Nominatim"
        );
    }

    let bind = config.bind_addr();
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!(
            "Failed to bind to {}. Is another process using port {}? Check config.toml [server] port or KOMUN_PORT env var.",
            bind, config.server.port
        ))?;

    tracing::info!("komun listening on http://{}", bind);

    // ConnectInfo is required, not optional: the auth rate limiter keys its buckets on the peer
    // address, and a limiter that cannot tell callers apart is not a limiter.
    let server = tokio::spawn(async move {
        let service = app.into_make_service_with_connect_info::<SocketAddr>();
        if let Err(e) = axum::serve(listener, service).await {
            tracing::error!("Server error: {}", e);
        }
    });

    if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        repl::run_repl(state).await;
    } else {
        server.await.context("Server task panicked")?;
    }

    Ok(())
}
