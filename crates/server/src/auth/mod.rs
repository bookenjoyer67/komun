//! Authentication (A2a / SPEC Part 1.5).
//!
//! What this replaces, and why:
//!
//! * **JWTs are gone.** A signed token is valid until it expires, so "sign out" was a client-side
//!   gesture, a stolen token could not be revoked, and a demoted admin kept their powers for the
//!   rest of the token's lifetime. Sessions are now opaque rows: revocation is an UPDATE, and the
//!   role is re-read from `users` on every single request.
//! * **The ed25519 challenge dance is gone.** It proved possession of a key the browser had just
//!   generated, which is not an authentication factor. Accounts are email + password.
//! * **The recovery-id lookup is gone.** It derived an identifier from the passphrase under a
//!   salt hardcoded into the WASM and identical on every deployment, reachable through an
//!   unauthenticated, unthrottled endpoint — a cross-deployment dictionary oracle.
//!
//! The password never reaches this server. The client derives
//! `verifier = Argon2id(password, auth_salt)` and sends the verifier; the server puts a second,
//! independent Argon2id over it before storage (see [`password`]). The other derivation,
//! `wrap_key = Argon2id(password, bundle_salt)`, never leaves the browser — it unwraps the x25519
//! secret, so the server cannot read private messages even with full database access.

pub mod email;
mod key_change;
pub mod password;

use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{ConnectInfo, Extension, Multipart, Path, Query, Request, State},
    http::{header, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::db::sessions as session_db;
use crate::rate_limit::{client_ip, RouteClass};
use crate::sessions;
use crate::AppState;

type ApiError = (StatusCode, Json<serde_json::Value>);

fn fail(status: StatusCode, message: &str) -> ApiError {
    (status, Json(serde_json::json!({ "error": message })))
}

/// Log the real cause, return a generic one. Database errors routinely carry table names, column
/// names and occasionally values; none of that belongs in an HTTP response.
fn internal(context: &str, e: impl std::fmt::Display) -> ApiError {
    tracing::error!("{context}: {e}");
    fail(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
}

pub fn encode_b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn decode_b64(s: &str) -> Result<Vec<u8>, ApiError> {
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|_| fail(StatusCode::BAD_REQUEST, "invalid base64"))
}

fn decode_b64_opt(s: &Option<String>) -> Result<Option<Vec<u8>>, ApiError> {
    match s.as_deref() {
        None => Ok(None),
        Some("") => Ok(None),
        Some(v) => decode_b64(v).map(Some),
    }
}

/// Addresses are stored lowercase (the `users.email` CHECK enforces it), so `Ada@Example.COM` and
/// `ada@example.com` are the same account and the second signup is a duplicate.
pub fn normalize_email(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// Cheap structural check. Deliverability is proven by the verification mail, not by a regex, so
/// this only rejects what cannot possibly be an address.
fn email_looks_valid(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && email.len() <= 254
        && !email.chars().any(char::is_whitespace)
        && email.matches('@').count() == 1
}

// ---------------------------------------------------------------------------
// router
// ---------------------------------------------------------------------------

pub fn router(state: AppState) -> Router {
    let public = Router::new()
        .route("/signup", post(signup))
        .route("/signin", post(signin))
        .route("/salt", get(auth_salt))
        .route("/verify", get(verify_email_link).post(verify_email))
        .route("/resend-verification", post(resend_verification))
        .route("/password-reset", post(request_password_reset))
        .route("/password-reset/bundle", get(password_reset_bundle))
        .route("/password-reset/confirm", post(confirm_password_reset));

    let protected = Router::new()
        .route("/me", get(me).put(update_profile))
        .route("/me/avatar", post(upload_avatar))
        .route("/signout", post(signout))
        .route(
            "/sessions",
            get(list_sessions).delete(revoke_other_sessions),
        )
        .route("/sessions/{id}", axum::routing::delete(revoke_session))
        .route("/users/{id}/keys", get(get_user_keys))
        // A2b.1. Both re-authenticate with the current verifier rather than trusting the session:
        // a stolen bearer token must not be enough to take the account over permanently.
        .route("/password/change", post(change_password))
        .route("/recovery/reissue", post(reissue_recovery))
        // The /me routes deliberately use the plain session check rather than `require_auth`:
        // an unverified user must be able to see who they are and ask for another mail.
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_session,
        ));

    public.merge(protected).with_state(state)
}

// ---------------------------------------------------------------------------
// wire types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SignupRequest {
    email: String,
    display_name: String,
    /// `Argon2id(password, auth_salt)`, base64. The password itself is never sent.
    verifier: String,
    /// The public, per-user salt the client used, base64. Returned by `GET /auth/salt` on later
    /// sign-ins so the same derivation can be repeated.
    auth_salt: String,
    /// The length of the plaintext password the client derived from, so the server can enforce
    /// `min_password_length` even against a client that skipped its own check.
    #[serde(default)]
    password_length: usize,
    encryption_public_key: Option<String>,
    /// x25519 secret wrapped under `wrap_key = Argon2id(password, bundle_salt)`.
    encrypted_key_bundle: Option<String>,
    bundle_salt: Option<String>,
    /// The same secret wrapped under the key derived from the 12-word recovery code.
    encrypted_recovery_bundle: Option<String>,
    recovery_bundle_salt: Option<String>,
    #[serde(default)]
    invite_code: Option<String>,
}

#[derive(Deserialize)]
pub struct SigninRequest {
    email: String,
    verifier: String,
    #[serde(default)]
    device_label: Option<String>,
}

#[derive(Serialize)]
pub struct SessionResponse {
    /// The raw session token. Shown exactly once — only its SHA-256 is stored.
    token: String,
    user_id: Uuid,
    display_name: String,
    bio: Option<String>,
    avatar_url: Option<String>,
    role: String,
    email_verified: bool,
    /// Present when the account has one; the client needs it plus the password to unwrap its
    /// x25519 secret. The server cannot unwrap it.
    encrypted_key_bundle: Option<String>,
    bundle_salt: Option<String>,
}

#[derive(Serialize)]
pub struct MeResponse {
    user_id: Uuid,
    display_name: String,
    bio: Option<String>,
    avatar_url: Option<String>,
    role: String,
    email: String,
    email_verified: bool,
}

#[derive(Deserialize)]
pub struct UpdateProfileRequest {
    display_name: Option<String>,
    bio: Option<String>,
    profile_json: Option<serde_json::Value>,
    encryption_public_key: Option<String>,
    encrypted_key_bundle: Option<String>,
    bundle_salt: Option<String>,
    encrypted_recovery_bundle: Option<String>,
    recovery_bundle_salt: Option<String>,
    current_verifier: Option<String>,
}

#[derive(Deserialize)]
pub struct SaltQuery {
    email: String,
}

#[derive(Serialize)]
pub struct SaltResponse {
    auth_salt: String,
}

#[derive(Deserialize)]
pub struct TokenQuery {
    token: String,
}

#[derive(Deserialize)]
pub struct TokenBody {
    token: String,
}

#[derive(Deserialize)]
pub struct EmailBody {
    email: String,
}

#[derive(Deserialize)]
pub struct PasswordResetConfirm {
    token: String,
    verifier: String,
    auth_salt: String,
    #[serde(default)]
    password_length: usize,
    /// The x25519 secret re-wrapped under the new password. Omitting it is allowed — the account
    /// is recoverable, but old encrypted messages stay unreadable until the recovery code is used.
    encrypted_key_bundle: Option<String>,
    bundle_salt: Option<String>,
    /// A2b: the three fields below exist for the reset-*without*-the-recovery-code path.
    ///
    /// Someone who has lost both their password and their recovery code cannot recover the old
    /// x25519 secret — nobody can, which is the point. Their client generates a fresh keypair, and
    /// the new *public* key has to reach the server or every correspondent would keep encrypting
    /// to a secret the account no longer holds: the user would regain the account and still be
    /// unable to read anything, including messages sent after the reset.
    encryption_public_key: Option<String>,
    encrypted_recovery_bundle: Option<String>,
    recovery_bundle_salt: Option<String>,
}

/// `POST /auth/password/change` — a signed-in user changing a password they still know.
///
/// Distinct from the reset flow in the two ways that matter: it proves knowledge of the current
/// password, and it keeps the x25519 secret. Only the wrapping changes, so every message the
/// account could read before it can still read afterwards.
#[derive(Deserialize)]
pub struct PasswordChangeRequest {
    /// `Argon2id(current password, current auth_salt)` — the same value `/auth/signin` takes.
    current_verifier: String,
    /// `Argon2id(new password, new auth_salt)`.
    verifier: String,
    auth_salt: String,
    #[serde(default)]
    password_length: usize,
    /// The *same* x25519 secret, re-wrapped under the new password. Required when the account has
    /// a bundle: changing the password without re-wrapping would strand the secret behind a key
    /// nobody can derive any more.
    encrypted_key_bundle: Option<String>,
    bundle_salt: Option<String>,
}

/// `POST /auth/recovery/reissue` — mint a replacement 12-word recovery code.
///
/// The code itself is generated in the browser and never transmitted; what arrives here is the
/// x25519 secret wrapped under a key derived from it. Writing the new wrapping over the old one is
/// what invalidates the previous code — there is nothing else to revoke, because the server never
/// held anything derived from it.
#[derive(Deserialize)]
pub struct RecoveryReissueRequest {
    /// Re-authentication. A stolen session token must not be enough to overwrite the recovery
    /// bundle with garbage and destroy the account's last way back in.
    current_verifier: String,
    encrypted_recovery_bundle: String,
    recovery_bundle_salt: String,
}

// ---------------------------------------------------------------------------
// rows
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct SigninRow {
    id: Uuid,
    display_name: String,
    bio: Option<String>,
    avatar_path: Option<String>,
    role: String,
    password_hash: String,
    email_verified: bool,
    encrypted_key_bundle: Option<Vec<u8>>,
    bundle_salt: Option<Vec<u8>>,
}

#[derive(sqlx::FromRow)]
struct MeRow {
    id: Uuid,
    email: String,
    display_name: String,
    bio: Option<String>,
    avatar_path: Option<String>,
    role: String,
    email_verified: bool,
}

#[derive(sqlx::FromRow)]
struct UserKeysRow {
    id: Uuid,
    encryption_public_key: Option<Vec<u8>>,
}

#[derive(Serialize)]
struct UserKeysResponse {
    user_id: Uuid,
    encryption_public_key: Option<String>,
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// The IP a rate-limit bucket is keyed by. `X-Forwarded-For` is honoured only when the connecting
/// peer is a configured trusted proxy; see [`crate::rate_limit::client_ip`].
fn limit_key(state: &AppState, peer: SocketAddr, headers: &axum::http::HeaderMap) -> IpAddr {
    let forwarded = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());
    client_ip(peer.ip(), forwarded, &state.trusted_proxies)
}

fn enforce_limit(state: &AppState, class: RouteClass, ip: IpAddr) -> Result<(), ApiError> {
    state.rate_limiter.check(class, ip).map_err(|retry| {
        tracing::info!(route = class.as_str(), %ip, "rate limited");
        (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": "too many attempts, try again later",
                "retry_after_seconds": retry.as_secs(),
            })),
        )
    })
}

/// A stable but unpredictable decoy salt for an address that has no account.
///
/// `GET /auth/salt` must answer the same way for a registered and an unregistered address,
/// otherwise it is an account-enumeration endpoint. The decoy is derived from a per-deployment
/// pepper so it looks exactly like a real salt and does not change between requests.
fn decoy_salt(pepper: &[u8], email: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(pepper);
    hasher.update([0u8]);
    hasher.update(email.as_bytes());
    hasher.finalize()[..16].to_vec()
}

/// Append an `audit_events` row. `pub(crate)` because `api::admin` records role changes through
/// the same path — one implementation means one place where the failure policy below is decided.
pub(crate) async fn record_audit(
    pool: &sqlx::PgPool,
    actor: Option<Uuid>,
    action: &str,
    subject: Option<Uuid>,
    detail: serde_json::Value,
) {
    let result = sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, subject_id, detail) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(actor)
    .bind(action)
    .bind(subject)
    .bind(detail)
    .execute(pool)
    .await;
    if let Err(e) = result {
        // An audit write must never break the operation it describes.
        tracing::warn!("audit write failed for {action}: {e}");
    }
}

/// Mint a session and return the response body for a successful sign-in or signup.
#[allow(clippy::too_many_arguments)]
async fn issue_session(
    state: &AppState,
    user_id: Uuid,
    display_name: String,
    bio: Option<String>,
    avatar_path: Option<String>,
    role: String,
    email_verified: bool,
    bundle: Option<Vec<u8>>,
    bundle_salt: Option<Vec<u8>>,
    device_label: Option<String>,
    user_agent: Option<String>,
    ip: IpAddr,
) -> Result<SessionResponse, ApiError> {
    let token = sessions::generate_token();
    let label = device_label
        .filter(|l| !l.trim().is_empty())
        .or_else(|| sessions::device_label_from_user_agent(user_agent.as_deref()));

    session_db::create(
        &state.pool,
        user_id,
        &token.hash,
        i64::from(state.config.auth.token_lifetime_days).max(1),
        label.as_deref(),
        user_agent.as_deref(),
        Some(&ip.to_string()),
    )
    .await
    .map_err(|e| internal("session create failed", e))?;

    Ok(SessionResponse {
        token: token.raw,
        user_id,
        display_name,
        bio,
        avatar_url: avatar_path.map(|p| format!("/avatars/{p}")),
        role,
        email_verified,
        encrypted_key_bundle: bundle.as_deref().map(encode_b64),
        bundle_salt: bundle_salt.as_deref().map(encode_b64),
    })
}

/// Mint a verification token, store its hash, and send the mail. Failures to *send* are logged
/// and swallowed: the account already exists and the user can ask for another link, so a flaky
/// relay must not turn a successful signup into a 500 with an orphaned row.
async fn send_verification(state: &AppState, user_id: Uuid, email: &str, display_name: &str) {
    if let Err(e) =
        session_db::invalidate_one_time_tokens(&state.pool, user_id, session_db::KIND_EMAIL_VERIFY)
            .await
    {
        tracing::warn!("could not retire previous verification tokens: {e}");
    }

    let token = sessions::generate_token();
    if let Err(e) = session_db::create_one_time_token(
        &state.pool,
        user_id,
        session_db::KIND_EMAIL_VERIFY,
        &token.hash,
        sessions::EMAIL_VERIFY_TTL_MINUTES,
    )
    .await
    {
        tracing::error!("could not store verification token: {e}");
        return;
    }

    let Some(mailer) = state.mailer.as_ref() else {
        // Legal only when require_email_verification is false (startup refuses otherwise), so
        // this is the "verification is optional and unconfigured" path.
        tracing::info!("no mailer configured; verification link not sent for {user_id}");
        return;
    };

    match mailer.verification_message(email, display_name, &token.raw) {
        Ok(message) => {
            if let Err(e) = mailer.send(message).await {
                tracing::error!("verification mail to {user_id} failed: {e}");
            }
        }
        Err(e) => tracing::error!("could not compose verification mail for {user_id}: {e}"),
    }
}

// ---------------------------------------------------------------------------
// handlers
// ---------------------------------------------------------------------------

async fn signup(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(input): Json<SignupRequest>,
) -> Result<Json<SessionResponse>, ApiError> {
    let ip = limit_key(&state, peer, &headers);
    enforce_limit(&state, RouteClass::SignUp, ip)?;

    if state.config.registration.mode == "closed" {
        return Err(fail(
            StatusCode::FORBIDDEN,
            "registration is closed on this server",
        ));
    }

    let email = normalize_email(&input.email);
    if !email_looks_valid(&email) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "that is not a valid email address",
        ));
    }

    let display_name = input.display_name.trim().to_string();
    if display_name.is_empty() || display_name.chars().count() > 64 {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "display name must be 1-64 characters",
        ));
    }

    password::validate_password_length(
        input.password_length,
        state.config.registration.min_password_length,
    )
    .map_err(|m| fail(StatusCode::BAD_REQUEST, &m))?;
    password::validate_verifier_shape(&input.verifier)
        .map_err(|m| fail(StatusCode::BAD_REQUEST, &m))?;

    let auth_salt = decode_b64(&input.auth_salt)?;
    if auth_salt.len() < 16 {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "auth_salt must be at least 16 bytes",
        ));
    }
    let encryption_pk = decode_b64_opt(&input.encryption_public_key)?;
    let bundle = decode_b64_opt(&input.encrypted_key_bundle)?;
    let bundle_salt = decode_b64_opt(&input.bundle_salt)?;
    let recovery_bundle = decode_b64_opt(&input.encrypted_recovery_bundle)?;
    let recovery_salt = decode_b64_opt(&input.recovery_bundle_salt)?;

    // Invite mode: claim a use before creating anything, so a failed claim cannot leave an
    // account behind.
    if state.config.registration.mode == "invite" {
        let code = input
            .invite_code
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .ok_or_else(|| {
                fail(
                    StatusCode::FORBIDDEN,
                    "this server is invite-only; an invite code is required",
                )
            })?;

        let claimed = sqlx::query(
            "UPDATE invites SET uses_remaining = uses_remaining - 1
             WHERE code = $1
               AND (uses_remaining IS NULL OR uses_remaining > 0)
               AND (expires_at IS NULL OR expires_at > now())",
        )
        .bind(code)
        .execute(&state.pool)
        .await
        .map_err(|e| internal("invite claim failed", e))?;

        if claimed.rows_affected() == 0 {
            return Err(fail(
                StatusCode::FORBIDDEN,
                "that invite code is not valid or has been used up",
            ));
        }
    }

    let password_hash = password::hash_verifier(&input.verifier)
        .map_err(|e| internal("verifier hashing failed", e))?;

    let user_id = Uuid::now_v7();
    let insert = sqlx::query(
        r#"INSERT INTO users
           (id, email, display_name, password_hash, auth_salt, encryption_public_key,
            encrypted_key_bundle, bundle_salt, encrypted_recovery_bundle, recovery_bundle_salt)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#,
    )
    .bind(user_id)
    .bind(&email)
    .bind(&display_name)
    .bind(&password_hash)
    .bind(&auth_salt)
    .bind(&encryption_pk)
    .bind(&bundle)
    .bind(&bundle_salt)
    .bind(&recovery_bundle)
    .bind(&recovery_salt)
    .execute(&state.pool)
    .await;

    if let Err(e) = insert {
        // The UNIQUE index on the lowercased address is what actually decides this, so
        // `Ada@Example.com` colliding with `ada@example.com` is caught here even though the two
        // strings differ. Answering honestly does tell a stranger that an address is registered;
        // the alternative — pretending to succeed — leaves the real owner unable to tell a
        // failed signup from a hijack attempt, and the address is confirmable by other means
        // anyway. SPEC Part 1.5 chooses the honest error.
        if let Some(db_err) = e.as_database_error() {
            if db_err.is_unique_violation() {
                return Err(fail(
                    StatusCode::CONFLICT,
                    "an account already exists for that email address",
                ));
            }
        }
        return Err(internal("signup insert failed", e));
    }

    record_audit(
        &state.pool,
        Some(user_id),
        "auth.signup",
        Some(user_id),
        serde_json::json!({ "mode": state.config.registration.mode }),
    )
    .await;

    send_verification(&state, user_id, &email, &display_name).await;

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let response = issue_session(
        &state,
        user_id,
        display_name,
        None,
        None,
        "user".to_string(),
        false,
        bundle,
        bundle_salt,
        None,
        user_agent,
        ip,
    )
    .await?;

    Ok(Json(response))
}

async fn signin(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(input): Json<SigninRequest>,
) -> Result<Json<SessionResponse>, ApiError> {
    let ip = limit_key(&state, peer, &headers);
    enforce_limit(&state, RouteClass::SignIn, ip)?;

    let email = normalize_email(&input.email);

    let row = sqlx::query_as::<_, SigninRow>(
        r#"SELECT id, display_name, bio, avatar_path, role, password_hash,
                  (email_verified_at IS NOT NULL) AS email_verified,
                  encrypted_key_bundle, bundle_salt
           FROM users WHERE email = $1"#,
    )
    .bind(&email)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| internal("signin lookup failed", e))?;

    let Some(row) = row else {
        // Spend the same Argon2 work as a real verification. Without this, "no such account"
        // returns in microseconds and "wrong password" takes ~50ms, which is a reliable
        // enumeration oracle over the network.
        password::dummy_verify();
        return Err(fail(
            StatusCode::UNAUTHORIZED,
            "incorrect email or password",
        ));
    };

    if !password::verify_verifier(&input.verifier, &row.password_hash) {
        // Identical message and status to the unknown-account case, on purpose.
        return Err(fail(
            StatusCode::UNAUTHORIZED,
            "incorrect email or password",
        ));
    }

    // Cost factors may have been raised since this hash was written; upgrade it now that the
    // correct verifier is in hand. Failure here is not the user's problem — they are signed in
    // either way.
    if password::needs_rehash(&row.password_hash) {
        match password::hash_verifier(&input.verifier) {
            Ok(upgraded) => {
                if let Err(e) = sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
                    .bind(&upgraded)
                    .bind(row.id)
                    .execute(&state.pool)
                    .await
                {
                    tracing::warn!("password rehash for {} failed: {e}", row.id);
                }
            }
            Err(e) => tracing::warn!("password rehash for {} failed: {e}", row.id),
        }
    }

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let response = issue_session(
        &state,
        row.id,
        row.display_name,
        row.bio,
        row.avatar_path,
        row.role,
        row.email_verified,
        row.encrypted_key_bundle,
        row.bundle_salt,
        input.device_label,
        user_agent,
        ip,
    )
    .await?;

    sqlx::query("UPDATE users SET last_seen = now() WHERE id = $1")
        .bind(row.id)
        .execute(&state.pool)
        .await
        .ok();

    Ok(Json(response))
}

/// The public salt for an address, so the client can derive the verifier before signing in.
///
/// Always answers 200 with a salt-shaped value: for an unregistered address it is a decoy derived
/// from the deployment pepper, so the endpoint cannot be used to test whether an address has an
/// account here.
async fn auth_salt(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Query(query): Query<SaltQuery>,
) -> Result<Json<SaltResponse>, ApiError> {
    let ip = limit_key(&state, peer, &headers);
    enforce_limit(&state, RouteClass::SignIn, ip)?;

    let email = normalize_email(&query.email);
    let stored: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT auth_salt FROM users WHERE email = $1")
            .bind(&email)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| internal("salt lookup failed", e))?;

    let salt = stored.unwrap_or_else(|| decoy_salt(&state.salt_pepper, &email));
    Ok(Json(SaltResponse {
        auth_salt: encode_b64(&salt),
    }))
}

/// `GET /auth/verify?token=...` — what the link in the mail points at.
async fn verify_email_link(
    State(state): State<AppState>,
    Query(query): Query<TokenQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    consume_verification(&state, &query.token).await
}

/// `POST /auth/verify` with `{"token": "..."}` — what a SPA posts after reading the link.
async fn verify_email(
    State(state): State<AppState>,
    Json(body): Json<TokenBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    consume_verification(&state, &body.token).await
}

async fn consume_verification(
    state: &AppState,
    raw_token: &str,
) -> Result<Json<serde_json::Value>, ApiError> {
    let hash = sessions::hash_token(raw_token);

    // Single-use and expiry are both decided by the UPDATE ... WHERE used_at IS NULL in
    // db::sessions, so two concurrent clicks cannot both win.
    let user_id =
        session_db::consume_one_time_token(&state.pool, session_db::KIND_EMAIL_VERIFY, &hash)
            .await
            .map_err(|e| internal("verification lookup failed", e))?
            .ok_or_else(|| {
                fail(
                    StatusCode::BAD_REQUEST,
                    "this verification link is invalid, already used, or expired",
                )
            })?;

    sqlx::query(
        "UPDATE users SET email_verified_at = now() WHERE id = $1 AND email_verified_at IS NULL",
    )
    .bind(user_id)
    .execute(&state.pool)
    .await
    .map_err(|e| internal("marking address verified failed", e))?;

    // Retire the user's other verification links. Each "resend" mints a new token, so an inbox
    // can hold several; once the address is confirmed none of them should still open a door.
    if let Err(e) =
        session_db::invalidate_one_time_tokens(&state.pool, user_id, session_db::KIND_EMAIL_VERIFY)
            .await
    {
        tracing::warn!("could not retire spent verification tokens: {e}");
    }

    record_audit(
        &state.pool,
        Some(user_id),
        "auth.email_verified",
        Some(user_id),
        serde_json::json!({}),
    )
    .await;

    Ok(Json(
        serde_json::json!({ "verified": true, "user_id": user_id }),
    ))
}

/// Ask for another verification mail. Answers the same way whether or not the address exists.
async fn resend_verification(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(body): Json<EmailBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ip = limit_key(&state, peer, &headers);
    enforce_limit(&state, RouteClass::VerifyResend, ip)?;

    let email = normalize_email(&body.email);
    let row = sqlx::query_as::<_, (Uuid, String, bool)>(
        "SELECT id, display_name, (email_verified_at IS NOT NULL) FROM users WHERE email = $1",
    )
    .bind(&email)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| internal("resend lookup failed", e))?;

    if let Some((user_id, display_name, already_verified)) = row {
        if !already_verified {
            // Per-account cap on top of the per-IP bucket: an attacker on many addresses still
            // cannot use this server to flood one person's inbox.
            let recent = session_db::count_recent_one_time_tokens(
                &state.pool,
                user_id,
                session_db::KIND_EMAIL_VERIFY,
                60,
            )
            .await
            .unwrap_or(0);
            if recent < 5 {
                send_verification(&state, user_id, &email, &display_name).await;
            } else {
                tracing::info!("verification resend suppressed for {user_id}: per-account cap");
            }
        }
    }

    // Deliberately identical for unknown addresses, already-verified accounts and successful
    // sends. The response must not reveal which of the three happened.
    Ok(Json(serde_json::json!({
        "ok": true,
        "message": "if that address has an unverified account here, a new link is on its way"
    })))
}

async fn request_password_reset(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(body): Json<EmailBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ip = limit_key(&state, peer, &headers);
    enforce_limit(&state, RouteClass::PasswordReset, ip)?;

    let email = normalize_email(&body.email);
    let row =
        sqlx::query_as::<_, (Uuid, String)>("SELECT id, display_name FROM users WHERE email = $1")
            .bind(&email)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| internal("reset lookup failed", e))?;

    if let Some((user_id, display_name)) = row {
        let recent = session_db::count_recent_one_time_tokens(
            &state.pool,
            user_id,
            session_db::KIND_PASSWORD_RESET,
            60,
        )
        .await
        .unwrap_or(0);

        if recent < 5 {
            if let Err(e) = session_db::invalidate_one_time_tokens(
                &state.pool,
                user_id,
                session_db::KIND_PASSWORD_RESET,
            )
            .await
            {
                tracing::warn!("could not retire previous reset tokens: {e}");
            }

            let token = sessions::generate_token();
            match session_db::create_one_time_token(
                &state.pool,
                user_id,
                session_db::KIND_PASSWORD_RESET,
                &token.hash,
                sessions::PASSWORD_RESET_TTL_MINUTES,
            )
            .await
            {
                Ok(_) => {
                    if let Some(mailer) = state.mailer.as_ref() {
                        match mailer.password_reset_message(&email, &display_name, &token.raw) {
                            Ok(message) => {
                                if let Err(e) = mailer.send(message).await {
                                    tracing::error!("reset mail to {user_id} failed: {e}");
                                }
                            }
                            Err(e) => tracing::error!("could not compose reset mail: {e}"),
                        }
                    } else {
                        tracing::warn!(
                            "password reset requested for {user_id} but no mailer is configured"
                        );
                    }
                }
                Err(e) => tracing::error!("could not store reset token: {e}"),
            }
        }
    }

    Ok(Json(serde_json::json!({
        "ok": true,
        "message": "if that address has an account here, a reset link is on its way"
    })))
}

/// What the browser needs to turn a recovery code back into the account's x25519 secret.
#[derive(Serialize)]
pub struct RecoveryBundleResponse {
    encrypted_recovery_bundle: Option<String>,
    recovery_bundle_salt: Option<String>,
}

/// `GET /auth/password-reset/bundle?token=…`
///
/// Someone resetting a forgotten password has no session and no old password, so they cannot reach
/// the wrapped secret through `/auth/signin`. Without this the "reset *with* the recovery code"
/// path is impossible: the code derives a key, but the ciphertext that key opens only exists on the
/// server. Handing it to the holder of a valid reset token costs nothing — that holder can already
/// take the account, and the bundle is sealed by ~128 bits drawn from the word list, so having the
/// ciphertext does not help them read anything.
///
/// The token is looked up, **not consumed**: the actual reset still needs it, and a client that
/// crashes between the two calls must not be left with a dead link.
async fn password_reset_bundle(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Query(q): Query<TokenQuery>,
) -> Result<Json<RecoveryBundleResponse>, ApiError> {
    let ip = limit_key(&state, peer, &headers);
    enforce_limit(&state, RouteClass::PasswordReset, ip)?;

    // Same predicate as `consume_one_time_token`, minus the write. It is spelled out here rather
    // than added to `db::sessions` because A2b owns `auth/**` and not `db/**`.
    let hash = sessions::hash_token(&q.token);
    let user_id = sqlx::query_scalar::<_, Uuid>(
        r#"SELECT user_id FROM one_time_tokens
           WHERE token_hash = $1
             AND kind = $2
             AND used_at IS NULL
             AND expires_at > now()"#,
    )
    .bind(&hash)
    .bind(session_db::KIND_PASSWORD_RESET)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| internal("reset token lookup failed", e))?
    .ok_or_else(|| {
        fail(
            StatusCode::BAD_REQUEST,
            "this reset link is invalid, already used, or expired",
        )
    })?;

    let row = sqlx::query_as::<_, (Option<Vec<u8>>, Option<Vec<u8>>)>(
        "SELECT encrypted_recovery_bundle, recovery_bundle_salt FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| internal("recovery bundle lookup failed", e))?;

    Ok(Json(RecoveryBundleResponse {
        encrypted_recovery_bundle: row.0.as_deref().map(encode_b64),
        recovery_bundle_salt: row.1.as_deref().map(encode_b64),
    }))
}

async fn confirm_password_reset(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(body): Json<PasswordResetConfirm>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ip = limit_key(&state, peer, &headers);
    enforce_limit(&state, RouteClass::PasswordReset, ip)?;

    password::validate_password_length(
        body.password_length,
        state.config.registration.min_password_length,
    )
    .map_err(|m| fail(StatusCode::BAD_REQUEST, &m))?;
    password::validate_verifier_shape(&body.verifier)
        .map_err(|m| fail(StatusCode::BAD_REQUEST, &m))?;

    let auth_salt = decode_b64(&body.auth_salt)?;
    if auth_salt.len() < 16 {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "auth_salt must be at least 16 bytes",
        ));
    }
    let bundle = decode_b64_opt(&body.encrypted_key_bundle)?;
    let bundle_salt = decode_b64_opt(&body.bundle_salt)?;
    let encryption_pk = decode_b64_opt(&body.encryption_public_key)?;
    let recovery_bundle = decode_b64_opt(&body.encrypted_recovery_bundle)?;
    let recovery_salt = decode_b64_opt(&body.recovery_bundle_salt)?;

    // Rotating the identity key is all-or-nothing. A client that sends a new public key but keeps
    // the old recovery bundle leaves a code that unwraps a secret no longer matching the published
    // key: using it later would look like a successful recovery and decrypt nothing.
    if encryption_pk.is_some()
        && (bundle.is_none()
            || bundle_salt.is_none()
            || recovery_bundle.is_none()
            || recovery_salt.is_none())
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "a new encryption_public_key must arrive with a new key bundle and recovery bundle",
        ));
    }

    let hash = sessions::hash_token(&body.token);
    let user_id =
        session_db::consume_one_time_token(&state.pool, session_db::KIND_PASSWORD_RESET, &hash)
            .await
            .map_err(|e| internal("reset token lookup failed", e))?
            .ok_or_else(|| {
                fail(
                    StatusCode::BAD_REQUEST,
                    "this reset link is invalid, already used, or expired",
                )
            })?;

    let password_hash = password::hash_verifier(&body.verifier)
        .map_err(|e| internal("verifier hashing failed", e))?;

    sqlx::query(
        "UPDATE users SET password_hash = $1, auth_salt = $2,
                encrypted_key_bundle = COALESCE($3, encrypted_key_bundle),
                bundle_salt = COALESCE($4, bundle_salt),
                encryption_public_key = COALESCE($5, encryption_public_key),
                encrypted_recovery_bundle = COALESCE($6, encrypted_recovery_bundle),
                recovery_bundle_salt = COALESCE($7, recovery_bundle_salt)
         WHERE id = $8",
    )
    .bind(&password_hash)
    .bind(&auth_salt)
    .bind(&bundle)
    .bind(&bundle_salt)
    .bind(&encryption_pk)
    .bind(&recovery_bundle)
    .bind(&recovery_salt)
    .bind(user_id)
    .execute(&state.pool)
    .await
    .map_err(|e| internal("password reset failed", e))?;

    // Whoever held a session before the reset may be the reason it was requested.
    let revoked = session_db::revoke_all(&state.pool, user_id)
        .await
        .map_err(|e| internal("revoking sessions after reset failed", e))?;

    record_audit(
        &state.pool,
        Some(user_id),
        "auth.password_reset",
        Some(user_id),
        serde_json::json!({ "sessions_revoked": revoked }),
    )
    .await;

    Ok(Json(
        serde_json::json!({ "ok": true, "sessions_revoked": revoked }),
    ))
}

/// What re-authentication needs: the stored hash, and whether there is key material at risk.
#[derive(sqlx::FromRow)]
struct ReauthRow {
    password_hash: String,
    has_key_bundle: bool,
}

/// Load the caller's stored verifier hash and check the one they just supplied.
///
/// Shared by `/auth/password/change` and `/auth/recovery/reissue`. Both are protected routes, so
/// this is a *second* factor in the literal sense — the session proves the browser, this proves
/// the person. The rate-limit token is refunded when the check passes: a legitimate password
/// change should not eat into the same IP's ability to sign in.
async fn reauthenticate(
    state: &AppState,
    user_id: Uuid,
    ip: IpAddr,
    current_verifier: &str,
) -> Result<ReauthRow, ApiError> {
    let row = sqlx::query_as::<_, ReauthRow>(
        "SELECT password_hash, (encrypted_key_bundle IS NOT NULL) AS has_key_bundle
         FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| internal("reauth lookup failed", e))?
    .ok_or_else(|| fail(StatusCode::NOT_FOUND, "user not found"))?;

    if !password::verify_verifier(current_verifier, &row.password_hash) {
        return Err(fail(
            StatusCode::UNAUTHORIZED,
            "current password is incorrect",
        ));
    }

    state.rate_limiter.refund(RouteClass::SignIn, ip);
    Ok(row)
}

/// `POST /auth/password/change`.
async fn change_password(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Extension(auth): Extension<AuthUser>,
    Json(body): Json<PasswordChangeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ip = limit_key(&state, peer, &headers);
    // Guessing the current password is the same attack as guessing it at the sign-in form, so it
    // belongs in the same bucket. The token is handed back below once the guess turns out right.
    enforce_limit(&state, RouteClass::SignIn, ip)?;

    password::validate_password_length(
        body.password_length,
        state.config.registration.min_password_length,
    )
    .map_err(|m| fail(StatusCode::BAD_REQUEST, &m))?;
    password::validate_verifier_shape(&body.verifier)
        .map_err(|m| fail(StatusCode::BAD_REQUEST, &m))?;

    let auth_salt = decode_b64(&body.auth_salt)?;
    if auth_salt.len() < 16 {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "auth_salt must be at least 16 bytes",
        ));
    }
    let bundle = decode_b64_opt(&body.encrypted_key_bundle)?;
    let bundle_salt = decode_b64_opt(&body.bundle_salt)?;

    let row = reauthenticate(&state, auth.user_id, ip, &body.current_verifier).await?;

    // The wrap key is `Argon2id(password, bundle_salt)`. A new password means a new wrap key, so
    // an account with key material that does not re-wrap has just locked itself out of its own
    // messages — silently, and irreversibly without the recovery code. Refuse instead.
    if row.has_key_bundle && (bundle.is_none() || bundle_salt.is_none()) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "encrypted_key_bundle and bundle_salt are required: the x25519 secret must be \
             re-wrapped under the new password",
        ));
    }

    let password_hash = password::hash_verifier(&body.verifier)
        .map_err(|e| internal("verifier hashing failed", e))?;

    // `encryption_public_key` and the recovery bundle are deliberately untouched. The identity key
    // does not change when the password does — only the wrapping around it — which is what makes
    // old messages still readable afterwards, and what keeps the existing recovery code valid.
    sqlx::query(
        "UPDATE users SET password_hash = $1, auth_salt = $2,
                encrypted_key_bundle = COALESCE($3, encrypted_key_bundle),
                bundle_salt = COALESCE($4, bundle_salt)
         WHERE id = $5",
    )
    .bind(&password_hash)
    .bind(&auth_salt)
    .bind(&bundle)
    .bind(&bundle_salt)
    .bind(auth.user_id)
    .execute(&state.pool)
    .await
    .map_err(|e| internal("password change failed", e))?;

    // Other sessions go; this one stays. The usual reason to change a password is that someone
    // else may have it, and whoever that is may be signed in right now.
    let revoked = session_db::revoke_all_except(&state.pool, auth.user_id, auth.session_id)
        .await
        .map_err(|e| internal("revoking other sessions failed", e))?;

    record_audit(
        &state.pool,
        Some(auth.user_id),
        "auth.password_change",
        Some(auth.user_id),
        serde_json::json!({ "sessions_revoked": revoked }),
    )
    .await;

    Ok(Json(
        serde_json::json!({ "ok": true, "sessions_revoked": revoked }),
    ))
}

/// `POST /auth/recovery/reissue`.
async fn reissue_recovery(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Extension(auth): Extension<AuthUser>,
    Json(body): Json<RecoveryReissueRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ip = limit_key(&state, peer, &headers);
    enforce_limit(&state, RouteClass::SignIn, ip)?;

    let recovery_bundle = decode_b64(&body.encrypted_recovery_bundle)?;
    let recovery_salt = decode_b64(&body.recovery_bundle_salt)?;
    if recovery_salt.len() < 16 {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "recovery_bundle_salt must be at least 16 bytes",
        ));
    }
    if recovery_bundle.is_empty() {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "encrypted_recovery_bundle must not be empty",
        ));
    }

    reauthenticate(&state, auth.user_id, ip, &body.current_verifier).await?;

    // Overwrite, not append. The old wrapping is gone the moment this row is written, and with it
    // the only thing the previous 12 words were good for.
    sqlx::query(
        "UPDATE users SET encrypted_recovery_bundle = $1, recovery_bundle_salt = $2 WHERE id = $3",
    )
    .bind(&recovery_bundle)
    .bind(&recovery_salt)
    .bind(auth.user_id)
    .execute(&state.pool)
    .await
    .map_err(|e| internal("recovery reissue failed", e))?;

    record_audit(
        &state.pool,
        Some(auth.user_id),
        "auth.recovery_reissued",
        Some(auth.user_id),
        serde_json::json!({}),
    )
    .await;

    // Note what is *not* here: the code. It was generated in the browser and this server has never
    // seen it, so there is no endpoint that could hand it back — which is the whole reason the
    // recovery path is safe to leave unauthenticated at the far end.
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn me(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<MeResponse>, ApiError> {
    let row = sqlx::query_as::<_, MeRow>(
        "SELECT id, email, display_name, bio, avatar_path, role,
                (email_verified_at IS NOT NULL) AS email_verified
         FROM users WHERE id = $1",
    )
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| internal("me lookup failed", e))?
    .ok_or_else(|| fail(StatusCode::NOT_FOUND, "user not found"))?;

    Ok(Json(MeResponse {
        user_id: row.id,
        display_name: row.display_name,
        bio: row.bio,
        avatar_url: row.avatar_path.map(|p| format!("/avatars/{p}")),
        role: row.role,
        email: row.email,
        email_verified: row.email_verified,
    }))
}

async fn signout(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let revoked = session_db::revoke(&state.pool, auth.user_id, auth.session_id)
        .await
        .map_err(|e| internal("signout failed", e))?;
    Ok(Json(serde_json::json!({ "ok": revoked })))
}

#[derive(Serialize)]
struct SessionSummary {
    id: Uuid,
    device_label: Option<String>,
    ip: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    last_used_at: chrono::DateTime<chrono::Utc>,
    expires_at: chrono::DateTime<chrono::Utc>,
    current: bool,
}

async fn list_sessions(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<Vec<SessionSummary>>, ApiError> {
    let rows = session_db::list_for_user(&state.pool, auth.user_id)
        .await
        .map_err(|e| internal("session list failed", e))?;

    Ok(Json(
        rows.into_iter()
            .map(|r| SessionSummary {
                current: r.id == auth.session_id,
                id: r.id,
                device_label: r.device_label,
                ip: r.ip,
                created_at: r.created_at,
                last_used_at: r.last_used_at,
                expires_at: r.expires_at,
            })
            .collect(),
    ))
}

async fn revoke_session(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // The user_id predicate lives in the query: without it, any authenticated user could sign out
    // any other by guessing a session id.
    let revoked = session_db::revoke(&state.pool, auth.user_id, id)
        .await
        .map_err(|e| internal("session revoke failed", e))?;
    if !revoked {
        return Err(fail(StatusCode::NOT_FOUND, "no such session"));
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn revoke_other_sessions(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let revoked = session_db::revoke_all_except(&state.pool, auth.user_id, auth.session_id)
        .await
        .map_err(|e| internal("bulk revoke failed", e))?;

    record_audit(
        &state.pool,
        Some(auth.user_id),
        "auth.revoke_other_sessions",
        Some(auth.user_id),
        serde_json::json!({ "count": revoked }),
    )
    .await;

    Ok(Json(serde_json::json!({ "revoked": revoked })))
}

async fn update_profile(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(input): Json<UpdateProfileRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if let Some(ref name) = input.display_name {
        if name.trim().is_empty() || name.chars().count() > 64 {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                "display name must be 1-64 characters",
            ));
        }
    }
    if let Some(ref pj) = input.profile_json {
        if serde_json::to_string(pj).unwrap_or_default().len() > 8192 {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                "profile_json must be under 8KB",
            ));
        }
    }

    let ip = limit_key(&state, peer, &headers);

    // The key fields are decided before any query: a partial set is refused here, whole, so a
    // request that also carries profile fields writes nothing (secaudit R2 / VA01).
    let keys = key_change::KeyColumns {
        encryption_public_key: decode_b64_opt(&input.encryption_public_key)?,
        encrypted_key_bundle: decode_b64_opt(&input.encrypted_key_bundle)?,
        bundle_salt: decode_b64_opt(&input.bundle_salt)?,
        encrypted_recovery_bundle: decode_b64_opt(&input.encrypted_recovery_bundle)?,
        recovery_bundle_salt: decode_b64_opt(&input.recovery_bundle_salt)?,
    };
    keys.check_complete()?;

    // The key change and the profile fields commit together. Every refusal returns before the
    // commit, and dropping the transaction rolls it back, so a refused request moves nothing.
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| internal("profile update failed", e))?;
    let outcome = key_change::apply(
        &mut tx,
        &state,
        auth.user_id,
        ip,
        &keys,
        input.current_verifier.as_deref(),
    )
    .await?;

    // A request with no profile field writes no profile column, so an identical key re-upload
    // leaves every column of the row as it was.
    if input.display_name.is_some() || input.bio.is_some() || input.profile_json.is_some() {
        sqlx::query(
            "UPDATE users SET display_name = COALESCE($1, display_name),
                              bio = COALESCE($2, bio),
                              profile_json = COALESCE($3, profile_json),
                              last_seen = now()
             WHERE id = $4",
        )
        .bind(input.display_name.as_deref().map(str::trim))
        .bind(&input.bio)
        .bind(&input.profile_json)
        .bind(auth.user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("profile update failed", e))?;
    }

    tx.commit()
        .await
        .map_err(|e| internal("profile update failed", e))?;
    key_change::audit(&state, auth.user_id, outcome).await;

    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn upload_avatar(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id = auth.user_id;

    let recent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM avatar_uploads WHERE user_id = $1 AND uploaded_at > now() - interval '1 hour'",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);
    if recent >= 5 {
        return Err(fail(
            StatusCode::TOO_MANY_REQUESTS,
            "rate limited: 5 uploads per hour",
        ));
    }

    let field = multipart
        .next_field()
        .await
        .map_err(|_| fail(StatusCode::BAD_REQUEST, "invalid multipart"))?
        .ok_or_else(|| fail(StatusCode::BAD_REQUEST, "no file"))?;

    let content_type = field.content_type().unwrap_or("").to_string();
    if !matches!(
        content_type.as_str(),
        "image/png" | "image/jpeg" | "image/webp"
    ) {
        return Err(fail(StatusCode::BAD_REQUEST, "only PNG, JPEG, WebP"));
    }

    let data = field
        .bytes()
        .await
        .map_err(|_| fail(StatusCode::BAD_REQUEST, "read error"))?;
    if data.len() > state.config.media.max_avatar_bytes as usize {
        return Err(fail(StatusCode::BAD_REQUEST, "file too large (max 1MB)"));
    }

    let img = image::load_from_memory(&data)
        .map_err(|_| fail(StatusCode::BAD_REQUEST, "invalid image"))?;
    let img = if img.width() > 512 || img.height() > 512 {
        img.resize(512, 512, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };

    let filename = format!("{}.webp", Uuid::now_v7());
    let dir = std::path::Path::new(&state.config.media.avatar_dir);
    std::fs::create_dir_all(dir).ok();
    img.save(dir.join(&filename))
        .map_err(|_| fail(StatusCode::INTERNAL_SERVER_ERROR, "save failed"))?;

    sqlx::query("INSERT INTO avatar_uploads (user_id, uploaded_at) VALUES ($1, now())")
        .bind(user_id)
        .execute(&state.pool)
        .await
        .ok();

    sqlx::query("UPDATE users SET avatar_path = $1 WHERE id = $2")
        .bind(&filename)
        .bind(user_id)
        .execute(&state.pool)
        .await
        .map_err(|e| internal("avatar update failed", e))?;

    Ok(Json(
        serde_json::json!({ "avatar_url": format!("/avatars/{filename}") }),
    ))
}

async fn get_user_keys(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<UserKeysResponse>, ApiError> {
    let row = sqlx::query_as::<_, UserKeysRow>(
        "SELECT id, encryption_public_key FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| internal("key lookup failed", e))?
    .ok_or_else(|| fail(StatusCode::NOT_FOUND, "user not found"))?;

    Ok(Json(UserKeysResponse {
        user_id: row.id,
        encryption_public_key: row.encryption_public_key.as_deref().map(encode_b64),
    }))
}

// ---------------------------------------------------------------------------
// middleware
// ---------------------------------------------------------------------------

/// The authenticated caller, as of *this* request.
///
/// `role` and `email_verified` are read from `users` on every request rather than carried in the
/// token. That is the whole point of the rewrite: revoking an admin is an UPDATE that takes effect
/// on their next call, not at the end of a token lifetime.
#[derive(Clone, Debug)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub role: String,
    pub email_verified: bool,
}

impl AuthUser {
    pub fn is_admin(&self) -> bool {
        matches!(self.role.as_str(), "admin" | "superadmin")
    }

    pub fn is_superadmin(&self) -> bool {
        self.role == "superadmin"
    }
}

fn unauthorized(message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": message })),
    )
        .into_response()
}

/// Pull the bearer token out of a request as an owned `String`.
///
/// Owned on purpose: `axum::body::Body` is not `Sync`, so a `&Request` held across an `await`
/// would make every middleware future non-`Send` and the whole router would stop compiling.
fn bearer_from_request(request: &Request) -> Option<String> {
    request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(sessions::bearer_token)
        .map(str::to_owned)
}

/// Bearer token → SHA-256 → session row → `AuthUser`. `Err` is the response to return.
async fn authenticate(state: &AppState, raw: Option<String>) -> Result<AuthUser, Response> {
    let raw = raw.ok_or_else(|| unauthorized("authentication required"))?;

    let hash = sessions::hash_token(&raw);
    let found = session_db::lookup(&state.pool, &hash).await.map_err(|e| {
        tracing::error!("session lookup failed: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "internal error" })),
        )
            .into_response()
    })?;

    // `lookup` filters revoked and expired rows in SQL, so a signed-out or stale token is
    // indistinguishable from one that never existed — which is exactly right.
    let session = found.ok_or_else(|| unauthorized("session is invalid or has expired"))?;

    if sessions::should_touch(session.last_used_at) {
        let pool = state.pool.clone();
        let session_id = session.session_id;
        tokio::spawn(async move {
            if let Err(e) = session_db::touch(&pool, session_id).await {
                tracing::warn!("session touch failed: {e}");
            }
        });
    }

    Ok(AuthUser {
        user_id: session.user_id,
        session_id: session.session_id,
        role: session.role,
        email_verified: session.email_verified,
    })
}

/// Authenticate only. Used by the `/me` routes, which an unverified user must still reach.
pub async fn require_session(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let bearer = bearer_from_request(&request);
    match authenticate(&state, bearer).await {
        Ok(user) => {
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Err(response) => response,
    }
}

/// Authenticate, and hold unverified accounts to read-only.
///
/// SPEC Part 1.5: "until verified a user can sign in but cannot post, respond, message or create
/// listings". Every one of those is a state-changing method, so the rule is enforced here by
/// method rather than by annotating each route — which also means a route added later is covered
/// by default instead of being forgotten.
pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let bearer = bearer_from_request(&request);
    let mutating = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );

    let user = match authenticate(&state, bearer).await {
        Ok(user) => user,
        Err(response) => return response,
    };

    if mutating && state.config.registration.require_email_verification && !user.email_verified {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "confirm your email address before posting, messaging or creating listings",
                "code": "email_unverified"
            })),
        )
            .into_response();
    }

    request.extensions_mut().insert(user);
    next.run(request).await
}

/// Admin *or* superadmin. The role comes from the database on this request, so a demotion that
/// happened a second ago is already in force.
pub async fn require_admin(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let bearer = bearer_from_request(&request);
    let user = match authenticate(&state, bearer).await {
        Ok(user) => user,
        Err(response) => return response,
    };

    if !user.is_admin() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "admin access required" })),
        )
            .into_response();
    }

    request.extensions_mut().insert(user);
    next.run(request).await
}

/// Superadmin only — role changes and anything else that can create an admin.
pub async fn require_superadmin(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let bearer = bearer_from_request(&request);
    let user = match authenticate(&state, bearer).await {
        Ok(user) => user,
        Err(response) => return response,
    };

    if !user.is_superadmin() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "superadmin access required" })),
        )
            .into_response();
    }

    request.extensions_mut().insert(user);
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_normalization_collapses_case_and_whitespace() {
        assert_eq!(normalize_email("  Ada@Example.COM "), "ada@example.com");
        assert_eq!(normalize_email("ada@example.com"), "ada@example.com");
        // The users.email CHECK requires lowercase, so normalization is what makes the UNIQUE
        // index catch a duplicate signup under different capitalisation.
        assert_eq!(
            normalize_email("ADA@EXAMPLE.COM"),
            normalize_email("ada@example.com")
        );
    }

    #[test]
    fn obvious_non_addresses_are_rejected() {
        assert!(email_looks_valid("ada@example.com"));
        assert!(email_looks_valid("a.b+c@sub.example.org"));
        assert!(!email_looks_valid("ada"));
        assert!(!email_looks_valid("@example.com"));
        assert!(!email_looks_valid("ada@"));
        assert!(!email_looks_valid("ada@example"));
        assert!(!email_looks_valid("ada@.com"));
        assert!(!email_looks_valid("ada@@example.com"));
        assert!(!email_looks_valid("ada example@test.com"));
        assert!(!email_looks_valid(&format!(
            "{}@example.com",
            "a".repeat(250)
        )));
    }

    #[test]
    fn decoy_salts_are_stable_per_address_and_differ_between_them() {
        let pepper = b"deployment-pepper";
        let a = decoy_salt(pepper, "ada@example.com");
        let b = decoy_salt(pepper, "ada@example.com");
        let c = decoy_salt(pepper, "grace@example.com");
        assert_eq!(a, b, "the same address must always get the same decoy");
        assert_ne!(a, c);
        assert_eq!(a.len(), 16, "a decoy must be salt-shaped");
    }

    #[test]
    fn decoy_salts_differ_between_deployments() {
        // Otherwise the decoy is computable by anyone who reads this source, and the endpoint
        // becomes an enumeration oracle again.
        let a = decoy_salt(b"pepper-one", "ada@example.com");
        let b = decoy_salt(b"pepper-two", "ada@example.com");
        assert_ne!(a, b);
    }

    #[test]
    fn auth_user_role_predicates() {
        let user = |role: &str| AuthUser {
            user_id: Uuid::now_v7(),
            session_id: Uuid::now_v7(),
            role: role.to_string(),
            email_verified: true,
        };
        assert!(!user("user").is_admin());
        assert!(!user("user").is_superadmin());
        assert!(user("admin").is_admin());
        assert!(!user("admin").is_superadmin());
        assert!(user("superadmin").is_admin());
        assert!(user("superadmin").is_superadmin());
        // A role string from outside the CHECK list must never be treated as privileged.
        assert!(!user("Admin").is_admin());
        assert!(!user("").is_admin());
    }

    #[test]
    fn base64_helpers_round_trip_and_reject_junk() {
        let bytes = b"komun key material".to_vec();
        assert_eq!(decode_b64(&encode_b64(&bytes)).expect("round trip"), bytes);
        assert!(decode_b64("!!!not base64!!!").is_err());
        assert_eq!(decode_b64_opt(&None).expect("none"), None);
        assert_eq!(decode_b64_opt(&Some(String::new())).expect("empty"), None);
    }
}
