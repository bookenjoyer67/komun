//! Opaque session tokens (A2a / SPEC Part 1.5).
//!
//! A session token is 256 bits from the OS CSPRNG, handed to the client once and never stored.
//! What the database holds is `SHA-256(token)`. That is deliberately *not* a slow hash: the token
//! is full-entropy random, so there is no dictionary to run against it and no reason to pay
//! Argon2's cost on every single request. Passwords get Argon2 (see [`crate::auth::password`]);
//! random tokens get SHA-256.
//!
//! The replaced JWT scheme could not do any of this: a signed token is valid until it expires, so
//! sign-out was a client-side gesture and a demoted admin kept their powers for up to a week.

use rand::RngCore;
use sha2::{Digest, Sha256};

/// 32 bytes = 256 bits of entropy. Guessing one is not a threat model.
const TOKEN_BYTES: usize = 32;

/// How long a session lives without being renewed.
pub const DEFAULT_LIFETIME_DAYS: i64 = 30;

/// Email-verification links last a day: long enough to survive a night's sleep, short enough that
/// a link left in an inbox archive is not a standing key to the account.
pub const EMAIL_VERIFY_TTL_MINUTES: i64 = 24 * 60;

/// Password-reset links last 30 minutes. They are strictly more dangerous than a verification
/// link, so they get the shorter window (SPEC Part 1.5).
pub const PASSWORD_RESET_TTL_MINUTES: i64 = 30;

/// A freshly minted token: the raw secret to return to the client exactly once, and the hash to
/// store. Holding them together for the moment between generation and use makes it hard to store
/// the wrong one by accident.
pub struct NewToken {
    pub raw: String,
    pub hash: Vec<u8>,
}

/// Generate a URL-safe, unpadded base64 token and its hash.
pub fn generate_token() -> NewToken {
    let mut bytes = [0u8; TOKEN_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let raw = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes);
    let hash = hash_token(&raw);
    NewToken { raw, hash }
}

/// `SHA-256(token)`. The only form of a token that is ever written down.
pub fn hash_token(raw: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hasher.finalize().to_vec()
}

/// Pull the bearer token out of an `Authorization` header value.
///
/// The scheme is matched case-insensitively (RFC 7235 says it is case-insensitive, and real
/// clients send `bearer`), but the token itself is taken verbatim.
pub fn bearer_token(header_value: &str) -> Option<&str> {
    let (scheme, token) = header_value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

/// Only write `last_used_at` if the stored value is at least this old. One write per minute per
/// active session is plenty of resolution for "last seen" and keeps a polling client from turning
/// every authenticated GET into an UPDATE.
pub const TOUCH_THROTTLE_SECONDS: i64 = 60;

/// Whether this request should refresh `last_used_at`.
pub fn should_touch(last_used_at: chrono::DateTime<chrono::Utc>) -> bool {
    (chrono::Utc::now() - last_used_at).num_seconds() >= TOUCH_THROTTLE_SECONDS
}

/// Best-effort device label from a User-Agent, for the session list. Truncated because the header
/// is attacker-controlled and there is no reason to store a kilobyte of it.
pub fn device_label_from_user_agent(user_agent: Option<&str>) -> Option<String> {
    let ua = user_agent?.trim();
    if ua.is_empty() {
        return None;
    }
    Some(ua.chars().take(120).collect())
}

/// How often the cleanup loop runs. Nothing depends on promptness here — expired rows are
/// already rejected by the `lookup` query — so this is housekeeping, not enforcement.
const CLEANUP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(6 * 3600);

/// Periodically delete sessions and one-time tokens that can never authenticate again.
///
/// Spawned from `main` rather than from `tasks::spawn_background_tasks` because A2a owns this
/// file and `main.rs`, and adding a line to `tasks/mod.rs` would mean editing a file this card
/// does not own for no behavioural gain.
pub async fn cleanup_loop(pool: sqlx::PgPool) {
    loop {
        match crate::db::sessions::delete_stale(&pool).await {
            Ok(n) if n > 0 => tracing::info!("session cleanup removed {n} dead sessions"),
            Ok(_) => {}
            Err(e) => tracing::warn!("session cleanup failed: {e}"),
        }
        match crate::db::sessions::delete_stale_one_time_tokens(&pool).await {
            Ok(n) if n > 0 => tracing::info!("session cleanup removed {n} spent one-time tokens"),
            Ok(_) => {}
            Err(e) => tracing::warn!("one-time token cleanup failed: {e}"),
        }
        tokio::time::sleep(CLEANUP_INTERVAL).await;
    }
}

/// A per-deployment pepper, generated at startup.
///
/// Used to derive decoy values that must look real but must not be computable by anyone reading
/// this source (see `auth::decoy_salt`). It lives in memory only: it protects nothing that
/// survives a restart, so there is nothing to persist.
pub fn generate_pepper() -> Vec<u8> {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique_and_hash_stably() {
        let a = generate_token();
        let b = generate_token();
        assert_ne!(a.raw, b.raw, "two tokens must never collide");
        assert_eq!(a.hash, hash_token(&a.raw));
        assert_eq!(a.hash.len(), 32, "SHA-256 is 32 bytes");
        assert_ne!(a.hash, b.hash);
    }

    #[test]
    fn raw_token_is_not_recoverable_from_the_hash() {
        // Not a proof of preimage resistance — just the property that matters operationally:
        // what we store is not what we accept.
        let t = generate_token();
        let stored =
            base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &t.hash);
        assert_ne!(stored, t.raw);
    }

    #[test]
    fn bearer_parsing_accepts_any_case_and_rejects_junk() {
        assert_eq!(bearer_token("Bearer abc123"), Some("abc123"));
        assert_eq!(bearer_token("bearer abc123"), Some("abc123"));
        assert_eq!(bearer_token("BEARER abc123"), Some("abc123"));
        assert_eq!(bearer_token("Basic abc123"), None);
        assert_eq!(bearer_token("abc123"), None);
        assert_eq!(bearer_token("Bearer "), None);
        assert_eq!(bearer_token(""), None);
    }

    #[test]
    fn touch_is_throttled() {
        let now = chrono::Utc::now();
        assert!(
            !should_touch(now),
            "a just-used session must not be rewritten"
        );
        assert!(!should_touch(now - chrono::Duration::seconds(30)));
        assert!(should_touch(now - chrono::Duration::seconds(61)));
        assert!(should_touch(now - chrono::Duration::hours(2)));
    }

    #[test]
    fn peppers_are_full_length_and_unpredictable() {
        let a = generate_pepper();
        let b = generate_pepper();
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }

    #[test]
    fn device_label_is_bounded() {
        assert_eq!(device_label_from_user_agent(None), None);
        assert_eq!(device_label_from_user_agent(Some("   ")), None);
        let long = "x".repeat(5000);
        assert_eq!(
            device_label_from_user_agent(Some(&long)).unwrap().len(),
            120
        );
    }
}
