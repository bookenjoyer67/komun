//! Passwords, timing, single-use tokens and session revocation.

mod auth_hardening;
mod badges;
mod deal_and_moderation;
mod key_change;
mod key_coherence;
mod location_privacy;
/// The marketplace foundation: `[market]` config, category scopes, market filters.
#[cfg(test)]
mod market;
mod ops_admin;
mod outbound_routes;
mod resource_limits;
mod saved_searches;
mod security_headers;
mod support;

/// The server-side half of the password path.
#[cfg(test)]
mod password_tests {
    use crate::auth::password::*;

    // A base64 Argon2id output is what the client actually sends.
    const VERIFIER: &str = "Hk3bQmVyaWZpZXItb3V0cHV0LTMyLWJ5dGVzLWV4YW1wbGU";
    const OTHER: &str = "Zm9vYmFyLXdyb25nLXZlcmlmaWVyLW91dHB1dC0zMi1ieXRlcw";

    #[test]
    fn the_correct_verifier_is_accepted_and_a_wrong_one_is_not() {
        let stored = hash_verifier(VERIFIER).expect("hash");
        assert!(
            verify_verifier(VERIFIER, &stored),
            "correct verifier must pass"
        );
        assert!(!verify_verifier(OTHER, &stored), "wrong verifier must fail");
        assert!(!verify_verifier("", &stored));
    }

    #[test]
    fn the_stored_hash_is_argon2id_with_a_random_salt() {
        let a = hash_verifier(VERIFIER).expect("hash");
        let b = hash_verifier(VERIFIER).expect("hash");
        assert!(a.starts_with("$argon2id$"), "must be Argon2id, got {a}");
        assert_ne!(
            a, b,
            "the same verifier must hash differently each time, or the salt is not random"
        );
        assert!(
            !a.contains(VERIFIER),
            "the verifier must not appear in its own hash"
        );
        // Both verify: a per-hash salt does not break verification.
        assert!(verify_verifier(VERIFIER, &a));
        assert!(verify_verifier(VERIFIER, &b));
    }

    // A coarse check, but it catches the failure that matters: an early-exit comparison is
    // orders of magnitude faster than a full Argon2 verify.
    #[test]
    fn verification_time_does_not_leak_how_wrong_the_guess_was() {
        use std::time::Instant;

        let stored = hash_verifier(VERIFIER).expect("hash");
        // Differs in the last character only: an early-exit compare would return instantly.
        let near_miss = format!("{}X", &VERIFIER[..VERIFIER.len() - 1]);
        let far_miss = OTHER;

        let mut near_total = std::time::Duration::ZERO;
        let mut far_total = std::time::Duration::ZERO;
        let mut good_total = std::time::Duration::ZERO;

        const ROUNDS: u32 = 5;
        for _ in 0..ROUNDS {
            let t = Instant::now();
            assert!(!verify_verifier(&near_miss, &stored));
            near_total += t.elapsed();

            let t = Instant::now();
            assert!(!verify_verifier(far_miss, &stored));
            far_total += t.elapsed();

            let t = Instant::now();
            assert!(verify_verifier(VERIFIER, &stored));
            good_total += t.elapsed();
        }

        let near = near_total / ROUNDS;
        let far = far_total / ROUNDS;
        let good = good_total / ROUNDS;

        // Ratios rather than absolute times, which depend on the machine.
        let ratio = |a: std::time::Duration, b: std::time::Duration| {
            a.as_secs_f64().max(b.as_secs_f64()) / a.as_secs_f64().min(b.as_secs_f64()).max(1e-9)
        };
        assert!(
            ratio(near, far) < 3.0,
            "near-miss {near:?} and far-miss {far:?} must cost the same"
        );
        assert!(
            ratio(good, far) < 3.0,
            "success {good:?} and failure {far:?} must cost the same"
        );
    }

    #[test]
    fn the_unknown_account_path_costs_the_same_as_a_real_check() {
        use std::time::Instant;

        let stored = hash_verifier(VERIFIER).expect("hash");

        let t = Instant::now();
        for _ in 0..3 {
            let _ = verify_verifier(OTHER, &stored);
        }
        let real = t.elapsed();

        let t = Instant::now();
        for _ in 0..3 {
            dummy_verify();
        }
        let dummy = t.elapsed();

        let ratio = real.as_secs_f64().max(dummy.as_secs_f64())
            / real.as_secs_f64().min(dummy.as_secs_f64()).max(1e-9);
        assert!(
            ratio < 3.0,
            "dummy_verify {dummy:?} must cost about what a real check costs {real:?}, \
             otherwise sign-in response time reveals whether the account exists"
        );
    }

    #[test]
    fn a_malformed_stored_hash_is_a_rejection_not_a_panic() {
        assert!(!verify_verifier(VERIFIER, ""));
        assert!(!verify_verifier(VERIFIER, "not-a-phc-string"));
        assert!(!verify_verifier(VERIFIER, "$argon2id$garbage"));
    }

    #[test]
    fn rehash_is_requested_only_for_weaker_or_broken_hashes() {
        let current = hash_verifier(VERIFIER).expect("hash");
        assert!(
            !needs_rehash(&current),
            "a fresh hash must not need rehashing"
        );

        // Weaker parameters than today's policy: 8 MiB, 1 pass.
        let weak = "$argon2id$v=19$m=8192,t=1,p=1$c29tZXNhbHRzb21lc2FsdA$\
                    DHkNPCnDoWtODzHqE5aTQCXNZAR0uKJRFWWCT8hcnBk";
        assert!(
            needs_rehash(weak),
            "weaker parameters must trigger a rehash"
        );

        // A different algorithm family, and outright junk.
        let argon2i = "$argon2i$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$\
                       DHkNPCnDoWtODzHqE5aTQCXNZAR0uKJRFWWCT8hcnBk";
        assert!(needs_rehash(argon2i));
        assert!(needs_rehash("not-a-hash"));
    }

    // The server re-enforces the length policy because a client may skip its own check.
    #[test]
    fn too_short_a_password_is_refused_at_signup() {
        assert!(validate_password_length(12, 12).is_ok());
        assert!(validate_password_length(40, 12).is_ok());

        let err = validate_password_length(8, 12).expect_err("8 < 12 must be refused");
        assert!(err.contains("at least 12"), "unhelpful error: {err}");
        assert!(validate_password_length(0, 12).is_err());
        assert!(validate_password_length(11, 12).is_err());
    }

    #[test]
    fn a_hand_typed_verifier_is_refused() {
        // Without this, a request could register a plaintext verifier directly.
        assert!(validate_verifier_shape(VERIFIER).is_ok());
        assert!(validate_verifier_shape("hunter2").is_err());
        assert!(validate_verifier_shape("").is_err());
        assert!(validate_verifier_shape(&"a".repeat(513)).is_err());
        assert!(
            validate_verifier_shape(&format!("{}!!", &VERIFIER[..41])).is_err(),
            "non-base64 characters must be refused"
        );
    }
}

/// Session tokens: what is stored, what is accepted, what is thrown away.
#[cfg(test)]
mod session_token_tests {
    use crate::sessions::*;

    #[test]
    fn what_is_stored_is_never_what_is_accepted() {
        let token = generate_token();
        assert_eq!(token.hash.len(), 32, "SHA-256");
        assert_eq!(token.hash, hash_token(&token.raw));
        assert_ne!(
            token.raw.as_bytes(),
            token.hash.as_slice(),
            "a database dump must not contain a usable bearer token"
        );
        assert!(
            token.raw.len() >= 43,
            "256 bits of entropy, base64: {}",
            token.raw
        );
    }

    #[test]
    fn a_token_that_differs_by_one_character_does_not_match() {
        let token = generate_token();
        let mut tampered = token.raw.clone();
        let last = tampered.pop().expect("non-empty");
        tampered.push(if last == 'A' { 'B' } else { 'A' });
        assert_ne!(hash_token(&tampered), token.hash);
    }

    #[test]
    fn lifetimes_match_the_spec() {
        // Verification 24h, reset 30 minutes.
        assert_eq!(EMAIL_VERIFY_TTL_MINUTES, 24 * 60);
        assert_eq!(PASSWORD_RESET_TTL_MINUTES, 30);
        // A `const` assert moves a wrong TTL pair from test time to build time.
        const _: () = assert!(
            PASSWORD_RESET_TTL_MINUTES < EMAIL_VERIFY_TTL_MINUTES,
            "a reset link is more dangerous than a verification link and must live shorter"
        );
    }
}

#[cfg(test)]
mod config_tests {
    use crate::config::Config;

    #[test]
    fn test_config_defaults() {
        let toml = r#"
[server]
bind_address = "127.0.0.1"
port = 3000

[database]
url = "postgres://localhost/test"
max_connections = 5

[node]
name = "test-node"

[discovery]
directory_enabled = false

[auth]
token_lifetime_days = 7

[security]
allowed_origins = "*"

[posts]
max_kind_length = 64

[admin]
superadmin_public_keys = []
"#;
        let config: Config = toml::from_str(toml).expect("parse config");
        assert_eq!(config.server.port, 3000);
        assert_eq!(config.database.max_connections, 5);
    }

    // Pure: setting DATABASE_URL in-process would race the parallel test threads.
    #[test]
    fn startup_without_a_database_url_names_where_to_set_it() {
        let err = Config::default()
            .validate_database()
            .expect_err("an unset database url must refuse to start")
            .to_string();
        assert!(err.contains("[database] url"), "unhelpful error: {err}");
        assert!(err.contains("DATABASE_URL"), "unhelpful error: {err}");
    }

    #[test]
    fn a_configured_database_url_passes_validation() {
        let toml = r#"
[database]
url = "postgres://db.test.invalid/komun"
"#;
        let config: Config = toml::from_str(toml).expect("parse config");
        config
            .validate_database()
            .expect("a configured database url must start");
    }

    // The messages never print the URL: a database URL can carry a password.
    #[test]
    fn the_default_config_carries_no_database_url() {
        let config = Config::default();
        assert!(
            config.database.url.is_empty(),
            "the default config must carry no database URL"
        );
    }

    #[test]
    fn a_database_section_without_a_url_leaves_it_empty() {
        let toml = r#"
[database]
max_connections = 5
"#;
        let config: Config = toml::from_str(toml).expect("parse config");
        assert!(
            config.database.url.is_empty(),
            "an unset [database] url must stay empty"
        );
        assert_eq!(config.database.max_connections, 5);
    }

    // A server that cannot send email must not be allowed to demand verification.
    #[test]
    fn verification_required_without_smtp_is_a_startup_error() {
        let toml = r#"
[registration]
require_email_verification = true
"#;
        let config: Config = toml::from_str(toml).expect("parse config");
        assert!(config.registration.require_email_verification);
        assert!(!config.email.is_configured());

        let err = config
            .validate_registration()
            .expect_err("a server that cannot send mail must not demand verification")
            .to_string();
        assert!(
            err.contains("require_email_verification"),
            "unhelpful error: {err}"
        );
        assert!(err.contains("[email]"), "unhelpful error: {err}");
    }

    #[test]
    fn verification_required_with_smtp_configured_starts() {
        let toml = r#"
[registration]
require_email_verification = true

[email]
smtp_host = "smtp.example.org"
smtp_port = 587
from = "Komun <noreply@example.org>"
starttls = true
"#;
        let config: Config = toml::from_str(toml).expect("parse config");
        assert!(config.email.is_configured());
        assert_eq!(config.email.port(), 587);
        config
            .validate_registration()
            .expect("configured SMTP must start");
    }

    // A blank host or a blank from address is not configuration, it is a typo.
    #[test]
    fn blank_smtp_fields_do_not_count_as_configured() {
        let toml = r#"
[registration]
require_email_verification = true

[email]
smtp_host = "   "
from = ""
"#;
        let config: Config = toml::from_str(toml).expect("parse config");
        assert!(!config.email.is_configured());
        assert!(config.validate_registration().is_err());
    }

    // open | invite | closed, defaulting to open.
    #[test]
    fn registration_mode_defaults_to_open_and_rejects_nonsense() {
        let default: Config = toml::from_str("").expect("parse empty config");
        assert_eq!(default.registration.mode, "open");
        assert_eq!(default.registration.min_password_length, 12);

        for mode in ["open", "invite", "closed"] {
            let toml =
                format!("[registration]\nmode = \"{mode}\"\nrequire_email_verification = false\n");
            let config: Config = toml::from_str(&toml).expect("parse config");
            assert!(
                config.validate_registration().is_ok(),
                "{mode} must be accepted"
            );
        }

        let bogus: Config = toml::from_str(
            "[registration]\nmode = \"invitation-only\"\nrequire_email_verification = false\n",
        )
        .expect("parse config");
        let err = bogus
            .validate_registration()
            .expect_err("bad mode must fail")
            .to_string();
        assert!(
            err.contains("open, invite, closed"),
            "unhelpful error: {err}"
        );
    }
}
