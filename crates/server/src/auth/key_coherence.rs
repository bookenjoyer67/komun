//! The key-coherence rule for the password writers that can also write encryption keys: `signup`,
//! `confirm_password_reset` and `change_password`.
//!
//! A wrapped bundle is useless without its salt, and a new public key is useless without both
//! bundles that carry its secret. So a request may send no key field, the bundle pair, the recovery
//! pair, both pairs, or all five; every other shape is refused with 400 before any query, any invite
//! decrement, any token consumption and any write. `signup` is stricter: all five or none.
//!
//! Both checks only test whether a field is present. They read no byte of key material, log nothing,
//! and this module declares no type, so nothing here can derive `Debug`.

use axum::http::StatusCode;

use super::key_change::KeyColumns;
use super::{fail, ApiError};

/// The refusal for a new public key sent without the full set. Byte-equal to the existing reset
/// guard's message, so a reset client sees the same text as before.
const PUBLIC_KEY_NEEDS_ALL: &str =
    "a new encryption_public_key must arrive with a new key bundle and recovery bundle";

/// The refusal for one half of the key-bundle pair.
const BUNDLE_PAIR: &str = "encrypted_key_bundle and bundle_salt must be sent together";

/// The refusal for one half of the recovery-bundle pair.
const RECOVERY_PAIR: &str =
    "encrypted_recovery_bundle and recovery_bundle_salt must be sent together";

/// The general rule, used by `confirm_password_reset` and `change_password`. Pure: no database,
/// no rate limiter. Accepts exactly five of the 32 shapes — none, the bundle pair, the recovery
/// pair, both pairs, or all five — and returns the first refusal otherwise: a public key without
/// all four other fields, then a broken bundle pair, then a broken recovery pair.
pub(super) fn check_pairs(keys: &KeyColumns) -> Result<(), ApiError> {
    let bundle = keys.encrypted_key_bundle.is_some();
    let bundle_salt = keys.bundle_salt.is_some();
    let recovery_bundle = keys.encrypted_recovery_bundle.is_some();
    let recovery_salt = keys.recovery_bundle_salt.is_some();
    let rest_complete = bundle && bundle_salt && recovery_bundle && recovery_salt;

    if keys.encryption_public_key.is_some() && !rest_complete {
        return Err(fail(StatusCode::BAD_REQUEST, PUBLIC_KEY_NEEDS_ALL));
    }
    if bundle != bundle_salt {
        return Err(fail(StatusCode::BAD_REQUEST, BUNDLE_PAIR));
    }
    if recovery_bundle != recovery_salt {
        return Err(fail(StatusCode::BAD_REQUEST, RECOVERY_PAIR));
    }
    Ok(())
}

/// The signup rule: all five or none. Reuses the partial-set check, so a refusal is 400 with that
/// message. Pure: no database, no rate limiter.
pub(super) fn check_signup(keys: &KeyColumns) -> Result<(), ApiError> {
    keys.check_complete()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic full key set. Not real key material.
    fn full(tag: u8) -> KeyColumns {
        KeyColumns {
            encryption_public_key: Some(vec![tag; 32]),
            encrypted_key_bundle: Some(vec![tag.wrapping_add(1); 72]),
            bundle_salt: Some(vec![tag.wrapping_add(2); 16]),
            encrypted_recovery_bundle: Some(vec![tag.wrapping_add(3); 72]),
            recovery_bundle_salt: Some(vec![tag.wrapping_add(4); 16]),
        }
    }

    /// The fields of `full(tag)` selected by the low five bits of `mask`. Bit 0 is
    /// `encryption_public_key`, bit 1 `encrypted_key_bundle`, bit 2 `bundle_salt`, bit 3
    /// `encrypted_recovery_bundle` and bit 4 `recovery_bundle_salt`.
    fn subset(tag: u8, mask: u8) -> KeyColumns {
        let keys = full(tag);
        let pick = |bit: u8, column: Option<Vec<u8>>| column.filter(|_| mask & (1 << bit) != 0);
        KeyColumns {
            encryption_public_key: pick(0, keys.encryption_public_key),
            encrypted_key_bundle: pick(1, keys.encrypted_key_bundle),
            bundle_salt: pick(2, keys.bundle_salt),
            encrypted_recovery_bundle: pick(3, keys.encrypted_recovery_bundle),
            recovery_bundle_salt: pick(4, keys.recovery_bundle_salt),
        }
    }

    const NONE: u8 = 0b00000;
    const PUBLIC_KEY: u8 = 0b00001;
    const BUNDLE: u8 = 0b00010;
    const BUNDLE_SALT: u8 = 0b00100;
    const RECOVERY_BUNDLE: u8 = 0b01000;
    const RECOVERY_SALT: u8 = 0b10000;
    const BUNDLE_PAIR_ONLY: u8 = BUNDLE | BUNDLE_SALT;
    const RECOVERY_PAIR_ONLY: u8 = RECOVERY_BUNDLE | RECOVERY_SALT;
    const BOTH_PAIRS: u8 = BUNDLE_PAIR_ONLY | RECOVERY_PAIR_ONLY;
    const ALL: u8 = PUBLIC_KEY | BOTH_PAIRS;

    /// The refusal message a request mask must get from `check_pairs`, or `None` when it passes.
    /// Written from the rule's statement, by counting, rather than from the function body.
    fn expected_pairs(mask: u8) -> Option<&'static str> {
        let bundle_halves = (mask & BUNDLE_PAIR_ONLY).count_ones();
        let recovery_halves = (mask & RECOVERY_PAIR_ONLY).count_ones();
        if mask & PUBLIC_KEY != 0 && mask != ALL {
            Some(PUBLIC_KEY_NEEDS_ALL)
        } else if bundle_halves == 1 {
            Some(BUNDLE_PAIR)
        } else if recovery_halves == 1 {
            Some(RECOVERY_PAIR)
        } else {
            None
        }
    }

    /// Asserts that `result` is a 400 carrying `message` as its `error` field.
    fn assert_refused(result: Result<(), ApiError>, message: &str, mask: u8) {
        let (status, body) = result.expect_err("expected a refusal");
        assert_eq!(status, StatusCode::BAD_REQUEST, "mask {mask:#07b}");
        assert_eq!(body.0["error"], message, "mask {mask:#07b}");
    }

    // Of the 32 subsets, check_pairs accepts exactly the five coherent shapes.
    #[test]
    fn check_pairs_accepts_exactly_five_of_the_32_subsets() {
        let accepted: Vec<u8> = (0..=ALL)
            .filter(|&mask| check_pairs(&subset(1, mask)).is_ok())
            .collect();
        assert_eq!(
            accepted,
            vec![NONE, BUNDLE_PAIR_ONLY, RECOVERY_PAIR_ONLY, BOTH_PAIRS, ALL]
        );
    }

    // Every one of the 32 subsets gets the outcome the rule's statement gives it, and every
    // refusal is a 400.
    #[test]
    fn check_pairs_gives_every_subset_its_expected_outcome() {
        let mut refused = 0;
        for mask in 0..=ALL {
            let result = check_pairs(&subset(2, mask));
            match expected_pairs(mask) {
                None => assert!(result.is_ok(), "mask {mask:#07b} must pass"),
                Some(message) => {
                    assert_refused(result, message, mask);
                    refused += 1;
                }
            }
        }
        assert_eq!(refused, 27);
    }

    // The refusal message, case by case.
    #[test]
    fn check_pairs_names_the_broken_rule() {
        for mask in [
            PUBLIC_KEY,
            PUBLIC_KEY | BUNDLE_PAIR_ONLY,
            PUBLIC_KEY | RECOVERY_PAIR_ONLY,
            PUBLIC_KEY | BUNDLE,
            ALL & !RECOVERY_SALT,
        ] {
            assert_refused(check_pairs(&subset(3, mask)), PUBLIC_KEY_NEEDS_ALL, mask);
        }
        for mask in [BUNDLE, BUNDLE_SALT, BUNDLE | RECOVERY_PAIR_ONLY] {
            assert_refused(check_pairs(&subset(3, mask)), BUNDLE_PAIR, mask);
        }
        for mask in [
            RECOVERY_BUNDLE,
            RECOVERY_SALT,
            BUNDLE_PAIR_ONLY | RECOVERY_SALT,
        ] {
            assert_refused(check_pairs(&subset(3, mask)), RECOVERY_PAIR, mask);
        }
    }

    // The rule tests presence, not content: an empty decoded value still counts as sent.
    #[test]
    fn an_empty_value_counts_as_present() {
        let keys = KeyColumns {
            encrypted_key_bundle: Some(Vec::new()),
            ..KeyColumns::default()
        };
        assert_refused(check_pairs(&keys), BUNDLE_PAIR, BUNDLE);
    }

    // The three literals are pinned because clients match on them; the first must stay byte-equal
    // to the reset guard's message.
    #[test]
    fn the_refusal_messages_are_fixed() {
        assert_eq!(
            PUBLIC_KEY_NEEDS_ALL,
            "a new encryption_public_key must arrive with a new key bundle and recovery bundle"
        );
        assert_eq!(
            BUNDLE_PAIR,
            "encrypted_key_bundle and bundle_salt must be sent together"
        );
        assert_eq!(
            RECOVERY_PAIR,
            "encrypted_recovery_bundle and recovery_bundle_salt must be sent together"
        );
    }

    // check_signup accepts none and all five only, and refuses the other 30 with the partial-set text.
    #[test]
    fn check_signup_accepts_only_none_or_all_five() {
        const PARTIAL_SET: &str = "the encryption keys must be sent together: \
                                   encryption_public_key, encrypted_key_bundle, bundle_salt, \
                                   encrypted_recovery_bundle, recovery_bundle_salt";
        let mut accepted = Vec::new();
        for mask in 0..=ALL {
            let result = check_signup(&subset(4, mask));
            if mask == NONE || mask == ALL {
                assert!(result.is_ok(), "mask {mask:#07b} must pass");
                accepted.push(mask);
            } else {
                assert_refused(result, PARTIAL_SET, mask);
            }
        }
        assert_eq!(accepted, vec![NONE, ALL]);
    }

    // Every shape the web client sends passes the check at its site.
    #[test]
    fn the_client_shapes_pass_their_sites() {
        // signup sends all five.
        assert!(check_signup(&full(5)).is_ok());
        // reset with a recovery code sends the bundle pair.
        assert!(check_pairs(&subset(5, BUNDLE_PAIR_ONLY)).is_ok());
        // reset without a code sends all five.
        assert!(check_pairs(&full(5)).is_ok());
        // change_password sends the bundle pair, or neither when no secret is held.
        let keys = full(5);
        let change = KeyColumns {
            encrypted_key_bundle: keys.encrypted_key_bundle,
            bundle_salt: keys.bundle_salt,
            ..KeyColumns::default()
        };
        assert!(check_pairs(&change).is_ok());
        assert!(check_pairs(&KeyColumns::default()).is_ok());
        // reissue sends the recovery pair. No span routes it here, but the rule would pass it.
        assert!(check_pairs(&subset(5, RECOVERY_PAIR_ONLY)).is_ok());
    }
}
