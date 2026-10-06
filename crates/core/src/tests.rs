// No inner `mod tests`: it would be `clippy::module_inception`.
#[cfg(test)]
mod schema_contract {
    use crate::models::*;
    use std::collections::{BTreeMap, BTreeSet};

    // The CHECK lists are read out of the migration at test time, never copied here, so the enums and the schema cannot drift unnoticed.

    fn schema_sql() -> String {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/001_schema.sql"
        );
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"))
    }

    /// Split a SQL value list on commas that are not inside a single-quoted string.
    fn split_sql_list(list: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        for c in list.chars() {
            match c {
                '\'' => {
                    in_quotes = !in_quotes;
                    current.push(c);
                }
                ',' if !in_quotes => {
                    out.push(std::mem::take(&mut current));
                }
                _ => current.push(c),
            }
        }
        out.push(current);
        out.into_iter()
            .map(|s| s.trim().trim_matches('\'').to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Every `CONSTRAINT chk_x CHECK (col IN ('a', 'b'))` in the migration, keyed by name; ranges, regexes and compound conditions are skipped rather than half-read.
    fn check_lists() -> BTreeMap<String, BTreeSet<String>> {
        let sql = schema_sql();
        let mut out = BTreeMap::new();
        for fragment in sql.split("CONSTRAINT ").skip(1) {
            let name = fragment
                .split_whitespace()
                .next()
                .expect("constraint name")
                .to_string();
            if !name.starts_with("chk_") {
                continue;
            }
            let Some((_, body)) = fragment.split_once("CHECK (") else {
                continue;
            };
            let Some((column, tail)) = body.split_once(" IN (") else {
                continue;
            };
            if column.is_empty()
                || !column
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                continue;
            }
            let Some((values, after)) = tail.split_once(')') else {
                continue;
            };
            // the list must close the CHECK: `col IN (...) OR ...` is a compound condition
            if !after.trim_start().starts_with(')') {
                continue;
            }
            let values: BTreeSet<String> = split_sql_list(values).into_iter().collect();
            if !values.is_empty() {
                out.insert(name, values);
            }
        }
        assert!(
            !out.is_empty(),
            "no CHECK value lists found in the migration — the parser or the schema changed"
        );
        out
    }

    fn check_list(name: &str) -> BTreeSet<String> {
        check_lists()
            .remove(name)
            .unwrap_or_else(|| panic!("migration has no CONSTRAINT {name} with an IN (...) list"))
    }

    /// Assert that an enum's `as_str()` value set equals the named CHECK list exactly.
    macro_rules! assert_enum_pinned {
        ($enum:ty, $constraint:literal) => {{
            let from_rust: BTreeSet<String> = <$enum>::ALL
                .iter()
                .map(|v| v.as_str().to_string())
                .collect();
            let from_sql = check_list($constraint);
            assert_eq!(
                from_rust,
                from_sql,
                "{} does not match {} in migrations/001_schema.sql",
                stringify!($enum),
                $constraint
            );
            for value in &from_sql {
                let parsed = <$enum>::parse(value)
                    .unwrap_or_else(|| panic!("{} cannot parse {value:?}", stringify!($enum)));
                assert_eq!(parsed.as_str(), value);
            }
            assert!(<$enum>::parse("definitely-not-a-value").is_none());
        }};
    }

    #[test]
    fn post_kind_matches_schema() {
        assert_enum_pinned!(PostKind, "chk_posts_kind");
    }

    #[test]
    fn urgency_matches_schema() {
        assert_enum_pinned!(Urgency, "chk_posts_urgency");
    }

    #[test]
    fn post_status_matches_schema() {
        assert_enum_pinned!(PostStatus, "chk_posts_status");
    }

    #[test]
    fn visibility_matches_schema() {
        assert_enum_pinned!(Visibility, "chk_posts_visibility");
    }

    #[test]
    fn item_condition_matches_schema() {
        assert_enum_pinned!(ItemCondition, "chk_posts_item_condition");
    }

    #[test]
    fn role_matches_schema() {
        assert_enum_pinned!(Role, "chk_users_role");
    }

    #[test]
    fn match_status_matches_schema() {
        assert_enum_pinned!(MatchStatus, "chk_matches_status");
    }

    #[test]
    fn offer_kind_matches_schema() {
        assert_enum_pinned!(OfferKind, "chk_match_offers_kind");
    }

    #[test]
    fn category_scope_matches_schema() {
        assert_enum_pinned!(CategoryScope, "chk_categories_scope");
    }

    /// A new enum-like CHECK list in the schema must come with a Rust enum pinning it, or be listed here as owned elsewhere.
    #[test]
    fn every_check_list_is_pinned_by_an_enum() {
        let pinned = [
            "chk_posts_kind",
            "chk_posts_urgency",
            "chk_posts_status",
            "chk_posts_visibility",
            "chk_posts_item_condition",
            "chk_users_role",
            "chk_matches_status",
            "chk_match_offers_kind",
            "chk_categories_scope",
        ];
        // one_time_tokens.kind is auth-internal and has no wire representation
        let exempt = ["chk_one_time_tokens_kind"];

        let unpinned: Vec<String> = check_lists()
            .keys()
            .filter(|name| !pinned.contains(&name.as_str()) && !exempt.contains(&name.as_str()))
            .cloned()
            .collect();
        assert!(
            unpinned.is_empty(),
            "CHECK lists with no enum pinning them: {unpinned:?}"
        );
    }

    /// (slug, label, scope) for every row of the seeded INSERT.
    fn seeded_categories() -> Vec<(String, String, String)> {
        let sql = schema_sql();
        let start = sql
            .find("INSERT INTO categories")
            .expect("the migration seeds the categories table");
        let block = &sql[start..];
        let block = &block[..block.find(';').expect("unterminated INSERT")];
        let values = &block[block.find("VALUES").expect("INSERT has no VALUES") + "VALUES".len()..];

        let mut rows = Vec::new();
        let mut rest = values;
        while let Some(open) = rest.find('(') {
            let close = rest[open..].find(')').expect("unterminated row") + open;
            let fields = split_sql_list(&rest[open + 1..close]);
            assert_eq!(
                fields.len(),
                4,
                "expected (slug, label, scope, sort_order), got {fields:?}"
            );
            rows.push((fields[0].clone(), fields[1].clone(), fields[2].clone()));
            rest = &rest[close + 1..];
        }
        rows
    }

    #[test]
    fn seeded_category_scopes_are_valid() {
        let valid = check_list("chk_categories_scope");
        for (slug, _, scope) in seeded_categories() {
            assert!(
                valid.contains(&scope),
                "category {slug} has scope {scope:?}, which the CHECK does not allow"
            );
        }
    }

    #[test]
    fn seeded_category_slugs_are_unique_lowercase_kebab() {
        let mut seen = BTreeSet::new();
        for (slug, _, _) in seeded_categories() {
            assert!(seen.insert(slug.clone()), "duplicate category slug {slug}");
            assert!(!slug.is_empty(), "empty category slug");
            assert!(
                slug.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "category slug {slug} is not lowercase-kebab"
            );
            assert!(
                !slug.starts_with('-') && !slug.ends_with('-'),
                "category slug {slug} has a leading or trailing dash"
            );
        }
    }

    #[test]
    fn seeded_category_labels_are_present() {
        for (slug, label, _) in seeded_categories() {
            assert!(
                !label.trim().is_empty(),
                "category {slug} has an empty label"
            );
        }
    }

    #[test]
    fn seeded_categories_are_the_agreed_list() {
        let rows = seeded_categories();
        assert_eq!(rows.len(), 23, "expected 23 seeded categories");
        let count = |scope: &str| rows.iter().filter(|(_, _, s)| s == scope).count();
        assert_eq!(count("market"), 15);
        assert_eq!(count("both"), 6);
        assert_eq!(count("aid"), 2);
    }

    /// serde must emit exactly the database value — the wire and the column agree.
    macro_rules! assert_serde_is_db_value {
        ($enum:ty) => {{
            for variant in <$enum>::ALL {
                let json = serde_json::to_string(variant).unwrap();
                assert_eq!(json, format!("\"{}\"", variant.as_str()));
                let back: $enum = serde_json::from_str(&json).unwrap();
                assert_eq!(back, *variant);
            }
        }};
    }

    #[test]
    fn enums_serialize_as_their_database_values() {
        assert_serde_is_db_value!(PostKind);
        assert_serde_is_db_value!(Urgency);
        assert_serde_is_db_value!(PostStatus);
        assert_serde_is_db_value!(Visibility);
        assert_serde_is_db_value!(ItemCondition);
        assert_serde_is_db_value!(Role);
        assert_serde_is_db_value!(MatchStatus);
        assert_serde_is_db_value!(OfferKind);
        assert_serde_is_db_value!(CategoryScope);
    }

    /// Display must emit the database value, never the Rust variant name; a fallback to the derived name would put `LikeNew` where the column holds `like_new`.
    macro_rules! assert_display_is_db_value {
        ($enum:ty) => {{
            for variant in <$enum>::ALL {
                assert_eq!(
                    format!("{variant}"),
                    variant.as_str(),
                    "{} must Display as its database value",
                    stringify!($enum)
                );
            }
        }};
    }

    #[test]
    fn enums_display_as_their_database_values() {
        assert_display_is_db_value!(PostKind);
        assert_display_is_db_value!(Urgency);
        assert_display_is_db_value!(PostStatus);
        assert_display_is_db_value!(Visibility);
        assert_display_is_db_value!(ItemCondition);
        assert_display_is_db_value!(Role);
        assert_display_is_db_value!(MatchStatus);
        assert_display_is_db_value!(OfferKind);
        assert_display_is_db_value!(CategoryScope);
    }

    #[test]
    fn only_marketplace_kinds_are_market_kinds() {
        assert!(PostKind::Listing.is_market());
        assert!(PostKind::Want.is_market());
        for kind in [PostKind::Resource, PostKind::Need, PostKind::Offer] {
            assert!(!kind.is_market(), "{kind} must not be a marketplace kind");
        }
    }

    /// The set, not the predicate's own body: the compiler makes a fifth status pick a side, and a status that starts resolving threads fails this test.
    #[test]
    fn only_the_terminal_statuses_are_resolved() {
        let resolved: Vec<MatchStatus> = MatchStatus::ALL
            .iter()
            .copied()
            .filter(|status| status.is_resolved())
            .collect();
        assert_eq!(
            resolved,
            vec![MatchStatus::Completed, MatchStatus::Withdrawn]
        );
    }

    #[test]
    fn admin_roles_are_admin() {
        assert!(Role::Admin.is_admin());
        assert!(Role::SuperAdmin.is_admin());
        assert!(!Role::User.is_admin());
    }

    #[test]
    fn post_round_trips_without_community_fields() {
        let post = Post {
            id: uuid::Uuid::now_v7(),
            author_id: uuid::Uuid::now_v7(),
            kind: PostKind::Offer,
            category: "food".into(),
            category_label: Some("Food".into()),
            title: "Free bread".into(),
            body: Some("Fresh sourdough".into()),
            location_name: None,
            location_lat: None,
            location_lon: None,
            urgency: None,
            quantity: None,
            status: PostStatus::Active,
            visibility: Visibility::Public,
            expires_at: None,
            tags: vec![],
            contact_method: None,
            images: vec![],
            verified_by: None,
            verified_at: None,
            market_listed: false,
            price_cents: None,
            currency: None,
            price_negotiable: false,
            item_condition: None,
            sold_at: None,
            buyer_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&post).unwrap();
        assert!(!json.contains("community"));
        assert!(!json.contains("federated"));
        assert!(json.contains("\"category\":\"food\""));
        let back: Post = serde_json::from_str(&json).unwrap();
        assert_eq!(back.category, "food");
        assert_eq!(back.kind, PostKind::Offer);
    }

    #[test]
    fn message_carries_ciphertext_not_a_body() {
        let message = Message {
            id: uuid::Uuid::now_v7(),
            match_id: uuid::Uuid::now_v7(),
            sender_id: uuid::Uuid::now_v7(),
            ciphertext: "3q2+7w==".into(),
            nonce: Some("q83vEjRW".into()),
            created_at: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("ciphertext"));
        assert!(!json.contains("\"body\""));
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.ciphertext, "3q2+7w==");
    }

    #[test]
    fn user_profile_carries_no_secret_key_material() {
        let profile = UserProfile {
            id: uuid::Uuid::now_v7(),
            display_name: "Ana".into(),
            email: None,
            email_verified: true,
            bio: None,
            avatar_url: None,
            encryption_public_key: Some("cHVibGlj".into()),
            role: Role::User,
            post_count: 0,
            verified_post_count: 0,
            endorsement_count: 0,
            joined_at: chrono::Utc::now(),
            last_seen: chrono::Utc::now(),
            profile_json: serde_json::json!({}),
        };
        let json = serde_json::to_string(&profile).unwrap();
        for forbidden in ["password", "auth_salt", "encrypted_key_bundle", "recovery"] {
            assert!(
                !json.contains(forbidden),
                "UserProfile leaked {forbidden}: {json}"
            );
        }
        assert!(!json.contains("\"email\""), "email is hidden when absent");
    }
}

#[cfg(test)]
mod location_grid {
    use crate::models::{coarsen_coordinate, LOCATION_PRECISION_DEGREES};

    #[test]
    fn the_precision_is_a_tenth_of_a_degree() {
        assert_eq!(LOCATION_PRECISION_DEGREES, 0.1);
    }

    /// `37.8`, not `37.800000000000004`: multiplying a step count by 0.1 lands off the grid.
    #[test]
    fn a_coordinate_snaps_to_the_grid_and_serialises_short() {
        assert_eq!(coarsen_coordinate(37.80443), 37.8);
        assert_eq!(
            serde_json::to_string(&coarsen_coordinate(37.80443)).unwrap(),
            "37.8"
        );
        assert_eq!(coarsen_coordinate(-122.27121), -122.3);
        assert_eq!(
            serde_json::to_string(&coarsen_coordinate(-122.27121)).unwrap(),
            "-122.3"
        );
    }

    #[test]
    fn the_boundaries_are_already_on_the_grid() {
        for edge in [90.0, -90.0, 180.0, -180.0] {
            assert_eq!(coarsen_coordinate(edge), edge, "{edge}");
        }
    }

    #[test]
    fn coarsening_is_idempotent() {
        for raw in [37.80443, -122.27121, 0.05, -0.04, 89.96, -179.95] {
            let once = coarsen_coordinate(raw);
            assert_eq!(coarsen_coordinate(once), once, "{raw}");
        }
    }

    /// A served `-0.0` would tell a reader which side of the line the post sits on.
    #[test]
    fn a_value_rounding_to_zero_is_positive_zero() {
        let zero = coarsen_coordinate(-0.04);
        assert_eq!(zero, 0.0);
        assert!(zero.is_sign_positive(), "got {zero:?}");
        assert_eq!(serde_json::to_string(&zero).unwrap(), "0.0");
    }

    /// `122.25` is exact in binary, so this is a true tie. `web/src/tests/geo.test.ts` pins the same
    /// two values for the browser's copy, so the runtimes cannot drift apart unnoticed.
    #[test]
    fn a_tie_rounds_away_from_zero() {
        assert_eq!(coarsen_coordinate(122.25), 122.3);
        assert_eq!(coarsen_coordinate(-122.25), -122.3);
    }
}
