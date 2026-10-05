//! Changing an account's encryption keys through `PUT /api/auth/me`.
//!
//! The five key columns of `users` (the public key, the wrapped key bundle, its salt, the recovery
//! bundle and its salt) move together or not at all. Following the Matrix rule for
//! `keys/device_signing/upload`, a session alone may set keys on an account that has none and may
//! re-upload the keys already stored, but replacing stored keys needs proof of the account
//! password in the same request. That proof is `super::reauthenticate`, charged to the sign-in
//! rate-limit bucket exactly as `change_password` charges it.
//!
//! Nothing here logs or audits key bytes, and no type that holds key bytes derives `Debug`, so
//! none of them is one `{:?}` away from a log line.

use std::net::IpAddr;

use axum::http::StatusCode;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::{enforce_limit, fail, internal, reauthenticate, record_audit, ApiError};
use crate::rate_limit::RouteClass;
use crate::AppState;

/// How many key columns `users` has.
const KEY_COLUMNS: usize = 5;

/// The refusal for a request carrying some of the five key fields but not all of them. The auth
/// API has no error-code field, so clients match on this message.
const PARTIAL_SET: &str = "the encryption keys must be sent together: \
                           encryption_public_key, encrypted_key_bundle, bundle_salt, \
                           encrypted_recovery_bundle, recovery_bundle_salt";

/// The refusal for a replacement of stored keys that carries no `current_verifier`.
const VERIFIER_REQUIRED: &str = "changing existing encryption keys requires current_verifier";

/// The audit action a successful replacement records.
const KEYS_REPLACED: &str = "auth.keys_replaced";

/// The refusal for a key write that would leave the five key columns partly set.
const INCOMPLETE_STORED_KEYS: &str =
    "the stored encryption keys are incomplete: a key update must send all five key columns";

/// The five key columns of `users`, in schema order, as raw bytes. `None` is a column that is not
/// stored, or a field the request did not send.
///
/// Holds key material, so it deliberately derives neither `Debug` nor `Serialize`.
#[derive(Clone, Default, PartialEq, Eq)]
pub(super) struct KeyColumns {
    pub(super) encryption_public_key: Option<Vec<u8>>,
    pub(super) encrypted_key_bundle: Option<Vec<u8>>,
    pub(super) bundle_salt: Option<Vec<u8>>,
    pub(super) encrypted_recovery_bundle: Option<Vec<u8>>,
    pub(super) recovery_bundle_salt: Option<Vec<u8>>,
}

impl KeyColumns {
    fn present(&self) -> usize {
        [
            &self.encryption_public_key,
            &self.encrypted_key_bundle,
            &self.bundle_salt,
            &self.encrypted_recovery_bundle,
            &self.recovery_bundle_salt,
        ]
        .iter()
        .filter(|column| column.is_some())
        .count()
    }

    /// Refuses a partial set with 400. It reads nothing and spends no rate-limit token, so
    /// `update_profile` calls it before any query. No key field at all, or all five, passes.
    pub(super) fn check_complete(&self) -> Result<(), ApiError> {
        match self.present() {
            0 | KEY_COLUMNS => Ok(()),
            _ => Err(fail(StatusCode::BAD_REQUEST, PARTIAL_SET)),
        }
    }
}

/// What a request does to the stored key set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyChange {
    /// No key field sent: a profile-only update.
    NoKeys,
    /// A full set, and no key column is stored: allowed on the session alone.
    FirstTimeSet,
    /// A full set byte-equal to the stored one: idempotent, and nothing is written.
    Identical,
    /// A full set over stored keys that differ: needs `current_verifier`.
    Replacement,
    /// Some but not all five fields: refused with 400.
    Partial,
}

/// Compares the stored key set with the requested one. Pure: no database, no rate limiter.
///
/// A stored row with any key column set counts as "keys stored", so a legacy partial row can only
/// be completed through the password-checked replacement path. That fails closed.
pub(super) fn classify(stored: &KeyColumns, requested: &KeyColumns) -> KeyChange {
    match requested.present() {
        0 => KeyChange::NoKeys,
        KEY_COLUMNS => {
            if stored.present() == 0 {
                KeyChange::FirstTimeSet
            } else if stored == requested {
                KeyChange::Identical
            } else {
                KeyChange::Replacement
            }
        }
        _ => KeyChange::Partial,
    }
}

/// The columns a `COALESCE` key write leaves behind: a requested column wins, a stored column
/// survives when the request does not send it. Pure.
pub(super) fn merged(stored: &KeyColumns, requested: &KeyColumns) -> KeyColumns {
    let pick = |requested: &Option<Vec<u8>>, stored: &Option<Vec<u8>>| {
        requested.clone().or_else(|| stored.clone())
    };
    KeyColumns {
        encryption_public_key: pick(
            &requested.encryption_public_key,
            &stored.encryption_public_key,
        ),
        encrypted_key_bundle: pick(
            &requested.encrypted_key_bundle,
            &stored.encrypted_key_bundle,
        ),
        bundle_salt: pick(&requested.bundle_salt, &stored.bundle_salt),
        encrypted_recovery_bundle: pick(
            &requested.encrypted_recovery_bundle,
            &stored.encrypted_recovery_bundle,
        ),
        recovery_bundle_salt: pick(
            &requested.recovery_bundle_salt,
            &stored.recovery_bundle_salt,
        ),
    }
}

/// The rule the three writers that update part of the key set share: a wrapped bundle is useless
/// without the public key it belongs to, and a public key is useless without both bundles that
/// carry its secret, so every write must leave all five columns set or none of them. A request that
/// sends some of the columns is therefore coherent only over a row that already holds the rest.
///
/// Refused with 409 rather than 400: the request is well formed, and it is the stored row it
/// conflicts with. A request that sends no key column writes none, so a row that is already
/// incomplete is not a reason to refuse a password-only change. Pure: no database, no rate limiter.
pub(super) fn check_merge_is_coherent(
    stored: &KeyColumns,
    requested: &KeyColumns,
) -> Result<(), ApiError> {
    if requested.present() == 0 || merged(stored, requested).present() == KEY_COLUMNS {
        return Ok(());
    }
    Err(fail(StatusCode::CONFLICT, INCOMPLETE_STORED_KEYS))
}

/// The verifier a replacement needs. A missing one is refused before any rate-limit token is
/// spent, because no password was guessed.
fn require_verifier(current_verifier: Option<&str>) -> Result<&str, ApiError> {
    current_verifier.ok_or_else(|| fail(StatusCode::UNAUTHORIZED, VERIFIER_REQUIRED))
}

/// The row shape of the stored-key read, in schema order.
type StoredRow = (
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
);

/// Reads `user_id`'s stored key columns inside `tx`, `FOR UPDATE`, so the decision below and the
/// write it authorises cannot be split by a concurrent key upload.
pub(super) async fn read_stored(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<KeyColumns, ApiError> {
    let row = sqlx::query_as::<_, StoredRow>(
        "SELECT encryption_public_key, encrypted_key_bundle, bundle_salt,
                encrypted_recovery_bundle, recovery_bundle_salt
         FROM users WHERE id = $1
         FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| internal("key read failed", e))?
    .ok_or_else(|| fail(StatusCode::NOT_FOUND, "user not found"))?;
    Ok(KeyColumns {
        encryption_public_key: row.0,
        encrypted_key_bundle: row.1,
        bundle_salt: row.2,
        encrypted_recovery_bundle: row.3,
        recovery_bundle_salt: row.4,
    })
}

/// Reads the stored key columns and refuses a request that would leave them partly set. Every
/// writer that updates some of the five columns and not all of them calls this in the same
/// transaction as its `UPDATE`.
pub(super) async fn check_stored(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    requested: &KeyColumns,
) -> Result<(), ApiError> {
    let stored = read_stored(tx, user_id).await?;
    check_merge_is_coherent(&stored, requested)
}

/// Applies the requested key set to `user_id`'s row inside `tx`, and returns what it did.
///
/// The stored keys are read `FOR UPDATE`, so the row stays locked until the caller commits or
/// drops `tx`, and two requests cannot both take the first-time path. Every refusal returns before
/// any write. `NoKeys` and `Identical` write nothing and never evaluate `current_verifier`;
/// `Partial` is 400; `Replacement` needs `current_verifier` and charges a `SignIn` token, refunded
/// by `reauthenticate` when the password is right. The two writing outcomes write all five columns
/// in one statement.
pub(super) async fn apply(
    tx: &mut Transaction<'_, Postgres>,
    state: &AppState,
    user_id: Uuid,
    ip: IpAddr,
    keys: &KeyColumns,
    current_verifier: Option<&str>,
) -> Result<KeyChange, ApiError> {
    // A profile-only update touches no key column, so it takes no lock.
    if keys.present() == 0 {
        return Ok(KeyChange::NoKeys);
    }
    keys.check_complete()?;

    let stored = read_stored(tx, user_id).await?;

    let outcome = classify(&stored, keys);
    match outcome {
        KeyChange::NoKeys | KeyChange::Identical => {}
        KeyChange::Partial => return Err(fail(StatusCode::BAD_REQUEST, PARTIAL_SET)),
        KeyChange::FirstTimeSet => write(tx, user_id, keys).await?,
        KeyChange::Replacement => {
            let verifier = require_verifier(current_verifier)?;
            // Guessing the password here is the same attack as guessing it at the sign-in form,
            // so it is charged to the same bucket, exactly as change_password charges it.
            enforce_limit(state, RouteClass::SignIn, ip)?;
            reauthenticate(state, user_id, ip, verifier).await?;
            write(tx, user_id, keys).await?;
        }
    }
    Ok(outcome)
}

/// Only `apply` calls it, and only with a complete set.
async fn write(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    keys: &KeyColumns,
) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE users SET encryption_public_key = $1,
                          encrypted_key_bundle = $2,
                          bundle_salt = $3,
                          encrypted_recovery_bundle = $4,
                          recovery_bundle_salt = $5
         WHERE id = $6",
    )
    .bind(&keys.encryption_public_key)
    .bind(&keys.encrypted_key_bundle)
    .bind(&keys.bundle_salt)
    .bind(&keys.encrypted_recovery_bundle)
    .bind(&keys.recovery_bundle_salt)
    .bind(user_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| internal("key update failed", e))?;
    Ok(())
}

/// Records a committed replacement in `audit_events`. The detail names how many columns moved and
/// carries no key material. Every other outcome records nothing. Call it after the commit, so a
/// rolled-back change leaves no audit row.
pub(super) async fn audit(state: &AppState, user_id: Uuid, outcome: KeyChange) {
    if outcome == KeyChange::Replacement {
        record_audit(
            &state.pool,
            Some(user_id),
            KEYS_REPLACED,
            Some(user_id),
            serde_json::json!({ "fields": KEY_COLUMNS }),
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic full key set. Two different tags differ in every column. Not real key material.
    fn full(tag: u8) -> KeyColumns {
        KeyColumns {
            encryption_public_key: Some(vec![tag; 32]),
            encrypted_key_bundle: Some(vec![tag.wrapping_add(1); 72]),
            bundle_salt: Some(vec![tag.wrapping_add(2); 16]),
            encrypted_recovery_bundle: Some(vec![tag.wrapping_add(3); 72]),
            recovery_bundle_salt: Some(vec![tag.wrapping_add(4); 16]),
        }
    }

    /// The columns of `full(tag)` selected by the low five bits of `mask`, bit 0 being
    /// `encryption_public_key` and bit 4 `recovery_bundle_salt`.
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

    /// Request masks over the five fields, bit 0 being `encryption_public_key`.
    const ALL: u8 = 0b11111;
    const PUBLIC_KEY_ALONE: u8 = 0b00001;
    const BUNDLES_ALONE: u8 = 0b01010;
    const MISSING_BUNDLE_SALT: u8 = 0b11011;
    const MISSING_RECOVERY_SALT: u8 = 0b01111;

    fn nothing_stored() -> KeyColumns {
        KeyColumns::default()
    }

    #[test]
    fn subset_helper_selects_by_mask() {
        assert!(subset(1, ALL) == full(1));
        assert!(subset(1, 0) == nothing_stored());
        assert_eq!(subset(1, BUNDLES_ALONE).present(), 2);
    }

    #[test]
    fn no_key_field_is_a_profile_only_update() {
        assert_eq!(
            classify(&nothing_stored(), &nothing_stored()),
            KeyChange::NoKeys
        );
        assert_eq!(classify(&full(1), &nothing_stored()), KeyChange::NoKeys);
    }

    #[test]
    fn full_set_with_nothing_stored_is_a_first_time_set() {
        assert_eq!(
            classify(&nothing_stored(), &full(1)),
            KeyChange::FirstTimeSet
        );
    }

    #[test]
    fn full_set_equal_to_the_stored_one_is_an_identical_reupload() {
        assert_eq!(classify(&full(1), &full(1)), KeyChange::Identical);
    }

    #[test]
    fn full_set_over_different_stored_keys_is_a_replacement() {
        assert_eq!(classify(&full(1), &full(9)), KeyChange::Replacement);
    }

    #[test]
    fn a_single_differing_column_makes_a_replacement() {
        let stored = full(1);
        let mut requested = full(1);
        requested.bundle_salt = Some(vec![0xee; 16]);
        assert_eq!(classify(&stored, &requested), KeyChange::Replacement);
    }

    #[test]
    fn full_set_over_a_partially_stored_row_is_a_replacement() {
        // Fails closed: a legacy row with only some columns set is treated as keys stored, even
        // when the request matches every column the row does hold.
        for mask in 1..ALL {
            assert_eq!(
                classify(&subset(1, mask), &full(1)),
                KeyChange::Replacement,
                "stored mask {mask:#07b}"
            );
        }
    }

    #[test]
    fn named_partial_cases_are_refused_whatever_is_stored() {
        for mask in [
            PUBLIC_KEY_ALONE,
            BUNDLES_ALONE,
            MISSING_BUNDLE_SALT,
            MISSING_RECOVERY_SALT,
        ] {
            let requested = subset(2, mask);
            for stored in [nothing_stored(), full(1), full(2)] {
                assert_eq!(
                    classify(&stored, &requested),
                    KeyChange::Partial,
                    "requested mask {mask:#07b}"
                );
            }
        }
    }

    #[test]
    fn of_the_31_non_empty_subsets_only_the_full_set_is_accepted() {
        let mut accepted = 0;
        for mask in 1..=ALL {
            let requested = subset(3, mask);
            let outcome = classify(&nothing_stored(), &requested);
            if mask == ALL {
                assert_eq!(outcome, KeyChange::FirstTimeSet);
                assert!(requested.check_complete().is_ok());
                accepted += 1;
            } else {
                assert_eq!(outcome, KeyChange::Partial, "requested mask {mask:#07b}");
                assert!(
                    requested.check_complete().is_err(),
                    "requested mask {mask:#07b}"
                );
            }
        }
        assert_eq!(accepted, 1);
    }

    #[test]
    fn check_complete_refuses_a_partial_set_with_400_and_the_fixed_message() {
        let (status, body) = subset(4, PUBLIC_KEY_ALONE)
            .check_complete()
            .expect_err("a public key alone is a partial set");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.0["error"], PARTIAL_SET);
        assert_eq!(
            PARTIAL_SET,
            "the encryption keys must be sent together: encryption_public_key, \
             encrypted_key_bundle, bundle_salt, encrypted_recovery_bundle, recovery_bundle_salt"
        );
    }

    #[test]
    fn check_complete_passes_no_keys_and_a_full_set() {
        assert!(nothing_stored().check_complete().is_ok());
        assert!(full(5).check_complete().is_ok());
    }

    #[test]
    fn a_replacement_without_a_verifier_is_refused_with_401() {
        let (status, body) = require_verifier(None).expect_err("no verifier sent");
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body.0["error"], VERIFIER_REQUIRED);
        assert_eq!(
            VERIFIER_REQUIRED,
            "changing existing encryption keys requires current_verifier"
        );
    }

    #[test]
    fn a_sent_verifier_is_passed_through_unchanged() {
        assert!(matches!(require_verifier(Some("v")), Ok("v")));
    }

    /// The recovery pair: the two columns `reissue_recovery` writes.
    const RECOVERY_PAIR_ALONE: u8 = 0b11000;

    #[test]
    fn merged_keeps_a_stored_column_the_request_does_not_send() {
        let requested = subset(2, BUNDLES_ALONE);
        let left = merged(&full(1), &requested);
        assert_eq!(left.encryption_public_key, full(1).encryption_public_key);
        assert_eq!(left.encrypted_key_bundle, requested.encrypted_key_bundle);
        assert_eq!(left.recovery_bundle_salt, full(1).recovery_bundle_salt);
        assert_eq!(merged(&nothing_stored(), &requested).present(), 2);
    }

    #[test]
    fn a_request_that_sends_no_key_column_is_coherent_over_any_stored_row() {
        for stored in 0..=ALL {
            assert!(
                check_merge_is_coherent(&subset(1, stored), &nothing_stored()).is_ok(),
                "stored mask {stored:#07b}"
            );
        }
    }

    #[test]
    fn a_full_set_is_coherent_over_any_stored_row() {
        for stored in 0..=ALL {
            assert!(
                check_merge_is_coherent(&subset(1, stored), &full(2)).is_ok(),
                "stored mask {stored:#07b}"
            );
        }
    }

    // The two shapes a partial writer sends are coherent exactly when they leave no column unset:
    // a pair over a complete row, or a pair that happens to complete a legacy row.
    #[test]
    fn a_pair_is_coherent_exactly_when_it_fills_the_row() {
        for requested_mask in [BUNDLES_ALONE, RECOVERY_PAIR_ALONE] {
            let requested = subset(2, requested_mask);
            for stored_mask in 0..=ALL {
                let expected = (stored_mask | requested_mask).count_ones() == KEY_COLUMNS as u32;
                assert_eq!(
                    check_merge_is_coherent(&subset(1, stored_mask), &requested).is_ok(),
                    expected,
                    "stored {stored_mask:#07b} requested {requested_mask:#07b}"
                );
            }
        }
    }

    #[test]
    fn a_bundle_pair_over_a_keyless_row_is_refused_with_409() {
        let (status, body) = check_merge_is_coherent(&nothing_stored(), &subset(2, BUNDLES_ALONE))
            .expect_err("two of five columns is not a key set");
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body.0["error"], INCOMPLETE_STORED_KEYS);
        assert_eq!(
            INCOMPLETE_STORED_KEYS,
            "the stored encryption keys are incomplete: a key update must send all five key columns"
        );
    }

    #[test]
    fn a_pair_over_a_complete_row_stays_coherent() {
        assert!(check_merge_is_coherent(&full(1), &subset(2, BUNDLES_ALONE)).is_ok());
        assert!(check_merge_is_coherent(&full(1), &subset(2, RECOVERY_PAIR_ALONE)).is_ok());
    }
}
