//! Server-side password hashing (SPEC Part 1.5).
//!
//! The server never sees a password. The client derives
//! `verifier = Argon2id(password, auth_salt)` and sends *that*; this module puts a second,
//! independent Argon2id over the verifier before it is stored, so a database dump alone does not
//! yield a login credential. `auth_salt` is public (the client must fetch it before it can log in);
//! the salt inside `password_hash` is per-user and random.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params, Version,
};

/// Interactive-login parameters: 19 MiB, 2 passes, 1 lane (OWASP's second recommended option).
/// Kept in one place so [`needs_rehash`] can tell a stored hash apart from the current policy.
const MEMORY_KIB: u32 = 19 * 1024;
const ITERATIONS: u32 = 2;
const PARALLELISM: u32 = 1;

fn hasher() -> Argon2<'static> {
    let params = Params::new(MEMORY_KIB, ITERATIONS, PARALLELISM, None)
        .expect("hardcoded Argon2 params are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// Hash a client-supplied verifier for storage. Returns a PHC string
/// (`$argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>`) that carries its own salt and parameters.
pub fn hash_verifier(verifier: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    hasher()
        .hash_password(verifier.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("failed to hash password verifier: {e}"))
}

/// Constant-time check of a verifier against a stored PHC hash.
///
/// `argon2`'s [`PasswordVerifier`] compares the output with `subtle`'s constant-time equality, so
/// no timing signal distinguishes a near-miss from a wild guess. A malformed stored hash is a
/// rejection, never a panic.
pub fn verify_verifier(verifier: &str, stored: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored) else {
        return false;
    };
    hasher()
        .verify_password(verifier.as_bytes(), &parsed)
        .is_ok()
}

/// Burn the same work as a real verification against a throwaway hash.
///
/// Sign-in must cost the same whether or not the email exists, otherwise the response time is an
/// account-enumeration oracle. Called on the unknown-user path.
pub fn dummy_verify() {
    // A fixed hash of a fixed input: parsing and verifying it costs exactly one Argon2id pass with
    // the parameters above, which is the whole point.
    const DUMMY: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$\
                         DHkNPCnDoWtODzHqE5aTQCXNZAR0uKJRFWWCT8hcnBk";
    let _ = verify_verifier("komun-dummy-verifier", DUMMY);
}

/// True when a stored hash was produced with parameters weaker than today's policy, so the caller
/// should re-hash the verifier it just successfully verified. The rehash-on-login hook from
/// SPEC Part 1.5: raising the cost factors upgrades accounts as their owners sign in.
pub fn needs_rehash(stored: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored) else {
        // Unparseable: it cannot be verified against either, so replacing it is the right call.
        return true;
    };
    if parsed.algorithm.as_str() != "argon2id" {
        return true;
    }
    let Ok(params) = Params::try_from(&parsed) else {
        return true;
    };
    params.m_cost() < MEMORY_KIB || params.t_cost() < ITERATIONS || params.p_cost() != PARALLELISM
}

/// The client enforces `min_password_length` before deriving, but a crafted request could register
/// a one-character "verifier", so the server also checks that what it receives is a plausible
/// Argon2id output.
pub fn validate_verifier_shape(verifier: &str) -> Result<(), String> {
    // 32 raw bytes, base64 -> 43-44 chars. Accept a range so the client may pick a longer output.
    if verifier.len() < 43 {
        return Err("password verifier is too short; it must be a base64 Argon2id output of at least 32 bytes".into());
    }
    if verifier.len() > 512 {
        return Err("password verifier is too long".into());
    }
    if !verifier
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'-' | b'_' | b'='))
    {
        return Err("password verifier must be base64".into());
    }
    Ok(())
}

/// The password-length policy, applied to the length the client reports for the password it
/// derived the verifier from. The server never sees the password, so it cannot check that report:
/// this stops a buggy client, not a hostile one.
pub fn validate_password_length(len: usize, minimum: usize) -> Result<(), String> {
    if len < minimum {
        return Err(format!(
            "password must be at least {minimum} characters (got {len})"
        ));
    }
    Ok(())
}
