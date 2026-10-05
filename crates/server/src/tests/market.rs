//! Pure-unit tests over the marketplace and negotiation logic; anything needing live Postgres
//! belongs to the runtime gate.

#[cfg(test)]
mod market_config_tests {
    use crate::config::{is_currency_code, Config};

    /// A guessed default is indistinguishable from a configured one, so the default is unset.
    #[test]
    fn default_currency_is_unset_by_default() {
        let config: Config = toml::from_str("").expect("parse empty config");
        assert!(
            config.market.default_currency.is_none(),
            "a server must not be handed a currency it never configured"
        );
        config.validate_market().expect("an unset default is valid");
    }

    #[test]
    fn a_well_formed_default_currency_is_accepted() {
        for code in ["USD", "EUR", "GBP", "XOF"] {
            let config: Config =
                toml::from_str(&format!("[market]\ndefault_currency = \"{code}\"\n"))
                    .expect("parse config");
            assert_eq!(config.market.default_currency.as_deref(), Some(code));
            config
                .validate_market()
                .unwrap_or_else(|e| panic!("{code} must be accepted: {e}"));
        }
    }

    /// The 400 must name the key and echo the value so the operator can find the TOML line.
    #[test]
    fn a_malformed_default_currency_refuses_to_start_and_names_the_key_and_the_value() {
        for bad in ["usd", "US", "USDD", "", "U5D", "us d"] {
            let config: Config =
                toml::from_str(&format!("[market]\ndefault_currency = \"{bad}\"\n"))
                    .expect("parse config");

            let err = config
                .validate_market()
                .expect_err("a malformed currency must refuse to start")
                .to_string();

            assert!(
                err.contains("[market] default_currency"),
                "the error must name the key, got: {err}"
            );
            assert!(
                err.contains(&format!("{bad:?}")),
                "the error must quote the bad value {bad:?}, got: {err}"
            );
        }
    }

    #[test]
    fn a_listings_own_currency_always_wins_over_the_server_default() {
        let configured: Config =
            toml::from_str("[market]\ndefault_currency = \"EUR\"\n").expect("parse config");
        assert_eq!(
            configured.market.resolve_currency(Some("USD")).as_deref(),
            Some("USD"),
            "the listing's own currency must not be overwritten by the server's"
        );
        assert_eq!(
            configured.market.resolve_currency(None).as_deref(),
            Some("EUR"),
            "the default is the fallback for a post that arrives without one"
        );

        let unset: Config = toml::from_str("").expect("parse empty config");
        assert_eq!(
            unset.market.resolve_currency(Some("USD")).as_deref(),
            Some("USD")
        );
        assert_eq!(
            unset.market.resolve_currency(None),
            None,
            "with no default configured, a listing without a currency keeps none"
        );
    }

    /// Must agree with `chk_posts_currency`, or accepted values fail as a 500 on insert.
    #[test]
    fn the_currency_rule_is_the_one_the_database_enforces() {
        for good in ["USD", "EUR", "XOF", "AAA"] {
            assert!(is_currency_code(good), "{good:?} must be accepted");
        }
        for bad in ["usd", "US", "USDD", "", "U5D", "US$", "\u{20ac}UR"] {
            assert!(!is_currency_code(bad), "{bad:?} must be rejected");
        }
    }
}

#[cfg(test)]
mod categories_tests {
    use uuid::Uuid;

    use komun_core::models::{Category, CategoryScope};

    use crate::api::categories::{
        accepted_scopes, audit_detail_create, audit_detail_update, bad_request, parse_scope,
        validate_label, validate_slug, AUDIT_CREATE, AUDIT_UPDATE,
    };
    use crate::auth::AuthUser;

    const SCHEMA: &str = include_str!("../../../../migrations/001_schema.sql");

    /// Read out of the migration, so a re-scoped seed is followed rather than duplicated here.
    fn seeded_scopes() -> Vec<(String, CategoryScope)> {
        let start = SCHEMA
            .find("INSERT INTO categories")
            .expect("the migration seeds the categories table");
        let block = &SCHEMA[start..];
        let block = &block[..block.find(';').expect("the seed INSERT is terminated")];

        block
            .lines()
            .filter(|line| line.trim_start().starts_with('('))
            .map(|line| {
                // No seeded label contains an apostrophe, so splitting on `'` yields the fields.
                let fields: Vec<&str> = line.split('\'').skip(1).step_by(2).collect();
                assert_eq!(fields.len(), 3, "unexpected seed row: {line}");
                let scope = CategoryScope::parse(fields[2])
                    .unwrap_or_else(|| panic!("seed row has an unknown scope: {line}"));
                (fields[0].to_string(), scope)
            })
            .collect()
    }

    /// Must mirror the SQL predicate `scope = $1 OR scope = 'both'` in `db::categories::list`.
    fn scope_includes(requested: CategoryScope, row: CategoryScope) -> bool {
        row == requested || row == CategoryScope::Both
    }

    fn slugs_for(requested: CategoryScope) -> Vec<String> {
        seeded_scopes()
            .into_iter()
            .filter(|(_, scope)| scope_includes(requested, *scope))
            .map(|(slug, _)| slug)
            .collect()
    }

    fn category(
        slug: &str,
        label: &str,
        scope: CategoryScope,
        sort_order: i32,
        active: bool,
    ) -> Category {
        let now = chrono::Utc::now();
        Category {
            slug: slug.to_string(),
            label: label.to_string(),
            scope,
            sort_order,
            active,
            created_at: now,
            updated_at: now,
        }
    }

    fn actor(role: &str) -> AuthUser {
        AuthUser {
            user_id: Uuid::now_v7(),
            session_id: Uuid::now_v7(),
            role: role.to_string(),
            email_verified: true,
        }
    }

    /// `market` is a union with `both`, not an equality: a `both` category must reach it.
    #[test]
    fn scope_market_is_a_union_with_both_not_an_equality() {
        assert_eq!(seeded_scopes().len(), 23, "SPEC 1.6 seeds 23 categories");

        let market = slugs_for(CategoryScope::Market);
        assert_eq!(
            market.len(),
            21,
            "the market form must see all 21 market-usable categories, got {market:?}"
        );
        assert!(
            market.iter().any(|s| s == "electronics"),
            "a market-only category is missing"
        );
        assert!(
            market.iter().any(|s| s == "services"),
            "a 'both' category must be offered to the market form"
        );
        assert!(
            !market.iter().any(|s| s == "shelter"),
            "an aid-only category must not reach the market form"
        );
    }

    #[test]
    fn scope_aid_sees_its_own_rows_and_the_shared_ones() {
        let aid = slugs_for(CategoryScope::Aid);
        assert_eq!(
            aid.len(),
            8,
            "the aid form must see 8 categories, got {aid:?}"
        );
        assert!(
            aid.iter().any(|s| s == "shelter"),
            "an aid-only category is missing"
        );
        assert!(
            aid.iter().any(|s| s == "food"),
            "a 'both' category must reach the aid form"
        );
        assert!(
            !aid.iter().any(|s| s == "electronics"),
            "a market-only category must not reach the aid form"
        );
    }

    #[test]
    fn scope_both_narrows_to_the_shared_rows() {
        let both = slugs_for(CategoryScope::Both);
        assert_eq!(both.len(), 6, "6 categories are shared, got {both:?}");
    }

    #[test]
    fn a_missing_or_blank_scope_means_every_active_category() {
        assert_eq!(parse_scope(None), Ok(None));
        assert_eq!(
            parse_scope(Some("   ")),
            Ok(None),
            "an untouched form field is not a filter"
        );
    }

    #[test]
    fn every_scope_the_check_allows_parses() {
        for scope in CategoryScope::ALL {
            assert_eq!(parse_scope(Some(scope.as_str())), Ok(Some(*scope)));
        }
    }

    /// A `200 []` would be indistinguishable from an empty taxonomy and hide the typo.
    #[test]
    fn an_unknown_scope_is_rejected_and_the_message_lists_what_is_accepted() {
        let err = parse_scope(Some("nope")).expect_err("an unknown scope must not be accepted");
        for scope in CategoryScope::ALL {
            assert!(
                err.contains(scope.as_str()),
                "the 400 must list {scope}, got: {err}"
            );
        }
        assert!(
            err.contains("nope"),
            "the 400 must echo the bad value, got: {err}"
        );
        assert_eq!(accepted_scopes(), "aid, market, both");
    }

    /// Pins the 400 mapping every message above goes through on its way out of a handler.
    #[test]
    fn a_rejected_parameter_leaves_as_a_400() {
        use axum::response::IntoResponse;

        let response = bad_request("scope must be one of aid, market, both").into_response();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    /// The routes allow admin or superadmin, hence their own `require_admin` layer.
    #[test]
    fn only_admins_and_superadmins_may_edit_the_taxonomy() {
        for (role, allowed) in [
            ("user", false),
            ("admin", true),
            ("superadmin", true),
            ("", false),
            ("Admin", false),
            ("administrator", false),
        ] {
            assert_eq!(
                actor(role).is_admin(),
                allowed,
                "role {role:?} must {} edit categories",
                if allowed {
                    "be able to"
                } else {
                    "not be able to"
                }
            );
        }
    }

    /// A slug cannot fit the UUID `subject_id`, so it has to travel in `detail`.
    #[test]
    fn a_category_audit_row_carries_the_slug_the_subject_column_cannot_hold() {
        let created = category(
            "bicycle-parts",
            "Bicycle Parts",
            CategoryScope::Market,
            15,
            true,
        );
        let detail = audit_detail_create(&created);

        assert_eq!(detail["slug"], "bicycle-parts");
        assert_eq!(detail["label"], "Bicycle Parts");
        assert_eq!(detail["scope"], "market");
        assert_eq!(detail["sort_order"], 15);
        assert_eq!(detail["active"], true);
    }

    #[test]
    fn an_update_audit_row_records_both_sides_and_the_reindex() {
        let before = category(
            "household",
            "Household Goods",
            CategoryScope::Market,
            140,
            true,
        );
        let after = category("household", "Home Goods", CategoryScope::Market, 140, false);
        let detail = audit_detail_update("household", &before, &after, 7);

        assert_eq!(detail["slug"], "household");
        assert_eq!(detail["from"]["label"], "Household Goods");
        assert_eq!(detail["to"]["label"], "Home Goods");
        assert_eq!(detail["from"]["active"], true);
        assert_eq!(detail["to"]["active"], false);
        // The FTS trigger indexes the label, so the reindex count exposes a skipped rename.
        assert_eq!(detail["posts_reindexed"], 7);
    }

    #[test]
    fn the_audit_actions_are_namespaced_like_the_other_admin_actions() {
        assert_eq!(AUDIT_CREATE, "admin.category_create");
        assert_eq!(AUDIT_UPDATE, "admin.category_update");
        assert!(
            AUDIT_CREATE.starts_with("admin."),
            "one prefix must find every admin action"
        );
        assert!(AUDIT_UPDATE.starts_with("admin."));
    }

    #[test]
    fn slugs_are_lowercase_kebab() {
        for good in ["electronics", "bikes-vehicles", "baby-kids", "web3", "a"] {
            validate_slug(good).unwrap_or_else(|e| panic!("{good:?} must be accepted: {e}"));
        }
        for bad in [
            "",
            "Electronics",
            "bikes vehicles",
            "bikes_vehicles",
            "-leading",
            "trailing-",
            "double--dash",
            "caf\u{e9}",
            "a/b",
        ] {
            assert!(validate_slug(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    /// Every seeded slug must satisfy the editor's rule, or the seed could not have come from it.
    #[test]
    fn every_seeded_slug_passes_the_rule_the_admin_editor_enforces() {
        for (slug, _) in seeded_scopes() {
            validate_slug(&slug)
                .unwrap_or_else(|e| panic!("seeded slug {slug:?} is rejected: {e}"));
        }
    }

    #[test]
    fn labels_must_not_be_blank() {
        validate_label("Electronics & Computers").expect("a seeded label must be accepted");
        assert!(validate_label("").is_err());
        assert!(validate_label("   ").is_err(), "whitespace is not a label");
        validate_label(&"x".repeat(80)).expect("80 characters is the limit, not past it");
        assert!(validate_label(&"x".repeat(81)).is_err());
    }
}

/// The `scope` in an admin body, not a query string. A real router and request are needed
/// because the defect only appears through axum's `Json` extractor. The pool is deliberately
/// unreachable: the 400 is decided before the first query.
#[cfg(test)]
mod category_body_scope_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use axum::{
        body::Body,
        extract::Extension,
        http::{header, Request, StatusCode},
        routing::post,
        Router,
    };
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;
    use uuid::Uuid;

    use komun_core::models::CategoryScope;

    use crate::api::categories::{accepted_scopes, create_category, parse_body_scope};
    use crate::auth::AuthUser;
    use crate::{config::Config, rate_limit, sessions, AppState};

    /// `create_category` mounted without `require_admin`; the admin is injected as the middleware
    /// would have.
    fn app() -> Router {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            // Port 1 refuses instantly; a short deadline avoids the 30-second default retry.
            .acquire_timeout(Duration::from_millis(300))
            .connect_lazy("postgres://komun:komun@127.0.0.1:1/komun_unreachable")
            .expect("a lazy pool does not connect");

        let config: Config = toml::from_str("").expect("parse empty config");
        let state = AppState {
            pool,
            config: Arc::new(config),
            rate_limiter: Arc::new(rate_limit::RateLimiter::new()),
            mailer: Arc::new(None),
            trusted_proxies: Arc::new(Vec::new()),
            salt_pepper: Arc::new(sessions::generate_pepper()),
        };

        Router::new()
            .route("/api/admin/categories", post(create_category))
            .layer(Extension(AuthUser {
                user_id: Uuid::now_v7(),
                session_id: Uuid::now_v7(),
                role: "admin".to_string(),
                email_verified: true,
            }))
            .with_state(state)
    }

    async fn create(body: &str) -> (StatusCode, String) {
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/admin/categories")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("build request"),
            )
            .await
            .expect("the router answers");

        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("read the body");
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    /// A bad scope must be this endpoint's 400, not serde's 422; a valid one must still pass.
    #[tokio::test]
    async fn an_unknown_body_scope_is_a_400_naming_the_accepted_values() {
        let (status, body) = create(r#"{"slug":"polish-probe","label":"P","scope":"nope"}"#).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "an unknown body scope must be a 400, not the extractor's 422: {body}"
        );
        for scope in CategoryScope::ALL {
            assert!(
                body.contains(scope.as_str()),
                "the 400 must list {scope}, got: {body}"
            );
        }
        assert!(
            body.contains("nope"),
            "the 400 must echo the bad value, got: {body}"
        );
        assert!(
            !body.contains("unknown variant") && !body.contains("Failed to deserialize"),
            "serde's wording must not reach the caller, got: {body}"
        );

        // A valid scope must fail only at the absent database, never on validation.
        let (status, body) =
            create(r#"{"slug":"polish-probe","label":"P","scope":"market"}"#).await;
        assert_eq!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "a valid create must pass validation and fail only at the absent database: {body}"
        );
        assert!(
            !body.contains("scope"),
            "a valid scope must not be mentioned in the failure at all, got: {body}"
        );

        assert_eq!(parse_body_scope("market"), Ok(CategoryScope::Market));
    }

    /// Missing or malformed JSON stays a 400; a well-formed body of the wrong shape stays serde's
    /// 422.
    #[tokio::test]
    async fn a_missing_or_malformed_body_is_a_400() {
        for body in ["", "   ", "{", "not json at all", r#"{"slug":"x","#] {
            let (status, text) = create(body).await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "body {body:?} must be a 400, got {status}: {text}"
            );
            assert!(
                !text.contains("panicked"),
                "body {body:?} must not panic, got: {text}"
            );
        }

        for body in ["[]", r#"{"slug":"x"}"#] {
            let (status, _) = create(body).await;
            assert_eq!(
                status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "body {body:?} is a shape error, which P1 leaves exactly where it was"
            );
        }
    }

    /// Both paths must refuse a bad scope in the same words.
    #[test]
    fn the_body_and_the_query_refuse_a_bad_scope_in_the_same_words() {
        use crate::api::categories::parse_scope;

        let from_query = parse_scope(Some("nope")).expect_err("the query path rejects it");
        let from_body = parse_body_scope("nope").expect_err("the body path rejects it");
        assert_eq!(from_query, from_body);
        assert_eq!(accepted_scopes(), "aid, market, both");

        // A blank body scope is a mistake, unlike a blank query scope.
        assert!(
            parse_body_scope("  ").is_err(),
            "a blank body scope is not a scope"
        );
        assert_eq!(parse_scope(Some("  ")), Ok(None));

        for scope in CategoryScope::ALL {
            assert_eq!(parse_body_scope(scope.as_str()), Ok(*scope));
            assert_eq!(parse_body_scope(&format!(" {scope} ")), Ok(*scope));
        }
    }
}

#[cfg(test)]
mod post_filter_tests {
    use komun_core::models::{ItemCondition, PostKind, PostStatus};

    use crate::api::posts::{validate_filters, PostFilters};
    use crate::db::posts::{DEFAULT_LIMIT, MAX_LIMIT};

    #[test]
    fn an_empty_query_string_is_the_default_page_of_everything() {
        let filter = validate_filters(&PostFilters::default()).expect("no filters is legal");
        assert_eq!(filter.kind, None);
        assert_eq!(filter.category, None);
        assert_eq!(filter.currency, None);
        assert_eq!(
            filter.limit, DEFAULT_LIMIT,
            "an unbounded feed is not an option"
        );
        assert_eq!(filter.offset, 0);
    }

    /// Every one of these is what an untouched `<select>` or `<input>` submits.
    #[test]
    fn a_blank_parameter_is_no_filter_rather_than_a_rejection() {
        let raw = PostFilters {
            kind: Some(String::new()),
            currency: Some("  ".to_string()),
            min_price_cents: Some(String::new()),
            item_condition: Some(String::new()),
            limit: Some(String::new()),
            ..Default::default()
        };

        let filter = validate_filters(&raw).expect("blank parameters must not be errors");
        assert_eq!(filter.kind, None);
        assert_eq!(filter.currency, None);
        assert_eq!(filter.min_price_cents, None);
        assert_eq!(filter.item_condition, None);
        assert_eq!(filter.limit, DEFAULT_LIMIT);
    }

    #[test]
    fn the_kind_filter_accepts_the_market_kinds_and_names_itself_when_it_cannot() {
        for kind in [PostKind::Listing, PostKind::Want] {
            let raw = PostFilters {
                kind: Some(kind.as_str().to_string()),
                ..Default::default()
            };
            assert_eq!(
                validate_filters(&raw).expect("a market kind is legal").kind,
                Some(kind)
            );
        }

        let raw = PostFilters {
            kind: Some("listings".to_string()),
            ..Default::default()
        };
        let err =
            validate_filters(&raw).expect_err("an unknown kind must be a 400, not an empty list");
        assert!(
            err.starts_with("kind "),
            "the 400 must name the parameter, got: {err}"
        );
        assert!(
            err.contains("listing"),
            "the 400 must list the accepted values, got: {err}"
        );
        assert!(
            err.contains("listings"),
            "the 400 must echo the bad value, got: {err}"
        );
    }

    /// Shape only: a well-formed slug may match nothing, a pasted label never will.
    #[test]
    fn the_category_filter_takes_a_slug_and_rejects_what_could_not_be_one() {
        let raw = PostFilters {
            category: Some("bikes-vehicles".to_string()),
            ..Default::default()
        };
        assert_eq!(
            validate_filters(&raw)
                .expect("a slug is legal")
                .category
                .as_deref(),
            Some("bikes-vehicles")
        );

        let raw = PostFilters {
            category: Some("Bikes & Vehicles".to_string()),
            ..Default::default()
        };
        let err = validate_filters(&raw).expect_err("a label is not a slug");
        assert!(
            err.starts_with("category "),
            "the 400 must name the parameter, got: {err}"
        );
    }

    #[test]
    fn the_price_filters_take_whole_cents_and_reject_anything_else() {
        let raw = PostFilters {
            min_price_cents: Some("100".to_string()),
            max_price_cents: Some("25000".to_string()),
            ..Default::default()
        };
        let filter = validate_filters(&raw).expect("a cent range is legal");
        assert_eq!(filter.min_price_cents, Some(100));
        assert_eq!(filter.max_price_cents, Some(25_000));

        let cases = [
            ("min_price_cents", "12.50"),
            ("min_price_cents", "cheap"),
            ("min_price_cents", "-1"),
            ("max_price_cents", "1e5"),
            ("max_price_cents", "\u{a3}20"),
        ];
        for (field, value) in cases {
            let raw = if field == "min_price_cents" {
                PostFilters {
                    min_price_cents: Some(value.to_string()),
                    ..Default::default()
                }
            } else {
                PostFilters {
                    max_price_cents: Some(value.to_string()),
                    ..Default::default()
                }
            };
            let err = validate_filters(&raw).expect_err("a malformed price must be a 400");
            assert!(
                err.starts_with(field),
                "the 400 must name {field}, got: {err}"
            );
        }
    }

    /// An inverted range matches nothing, so `200 []` would hide an obvious mistake.
    #[test]
    fn an_inverted_price_range_is_rejected_rather_than_answered_with_nothing() {
        let raw = PostFilters {
            min_price_cents: Some("5000".to_string()),
            max_price_cents: Some("100".to_string()),
            ..Default::default()
        };
        let err = validate_filters(&raw).expect_err("min above max must be a 400");
        assert!(
            err.contains("min_price_cents") && err.contains("max_price_cents"),
            "the 400 must name both parameters, got: {err}"
        );
    }

    #[test]
    fn the_currency_filter_is_iso_4217_or_a_400() {
        let raw = PostFilters {
            currency: Some("USD".to_string()),
            ..Default::default()
        };
        assert_eq!(
            validate_filters(&raw)
                .expect("USD is legal")
                .currency
                .as_deref(),
            Some("USD")
        );

        for bad in ["dollars", "usd", "US", "USDD"] {
            let raw = PostFilters {
                currency: Some(bad.to_string()),
                ..Default::default()
            };
            let err = validate_filters(&raw).expect_err("a non-ISO currency must be a 400");
            assert!(
                err.starts_with("currency "),
                "the 400 must name the parameter, got: {err}"
            );
            assert!(
                err.contains(bad),
                "the 400 must echo the bad value, got: {err}"
            );
        }
    }

    #[test]
    fn the_item_condition_filter_matches_the_schemas_check_list() {
        for condition in ItemCondition::ALL {
            let raw = PostFilters {
                item_condition: Some(condition.as_str().to_string()),
                ..Default::default()
            };
            assert_eq!(
                validate_filters(&raw)
                    .expect("a known condition is legal")
                    .item_condition,
                Some(*condition)
            );
        }

        let raw = PostFilters {
            item_condition: Some("mint".to_string()),
            ..Default::default()
        };
        let err = validate_filters(&raw).expect_err("'mint' is not a condition this schema allows");
        assert!(
            err.starts_with("item_condition "),
            "the 400 must name the parameter, got: {err}"
        );
        assert!(
            err.contains("like_new"),
            "the 400 must list the accepted values, got: {err}"
        );
    }

    /// The same trap: an unknown value must not filter to an empty marketplace.
    #[test]
    fn the_status_filter_is_validated_too_so_a_typo_is_not_an_empty_marketplace() {
        let raw = PostFilters {
            status: Some("active".to_string()),
            ..Default::default()
        };
        assert_eq!(
            validate_filters(&raw).expect("active is legal").status,
            Some(PostStatus::Active)
        );

        let raw = PostFilters {
            status: Some("sold".to_string()),
            ..Default::default()
        };
        let err = validate_filters(&raw).expect_err("'sold' is not a post status");
        assert!(
            err.starts_with("status "),
            "the 400 must name the parameter, got: {err}"
        );
    }

    /// Rejected rather than clamped, so a caller cannot silently get the wrong page.
    #[test]
    fn limit_and_offset_are_bounded_and_named_when_they_are_not() {
        let raw = PostFilters {
            limit: Some("20".to_string()),
            offset: Some("40".to_string()),
            ..Default::default()
        };
        let filter = validate_filters(&raw).expect("a page is legal");
        assert_eq!(filter.limit, 20);
        assert_eq!(filter.offset, 40);

        let raw = PostFilters {
            limit: Some(MAX_LIMIT.to_string()),
            ..Default::default()
        };
        assert_eq!(
            validate_filters(&raw).expect("the maximum is legal").limit,
            MAX_LIMIT
        );

        for bad in [
            "0".to_string(),
            "-5".to_string(),
            (MAX_LIMIT + 1).to_string(),
            "all".to_string(),
        ] {
            let raw = PostFilters {
                limit: Some(bad.clone()),
                ..Default::default()
            };
            let err = validate_filters(&raw).expect_err("a bad limit must be a 400");
            assert!(
                err.starts_with("limit "),
                "the 400 must name the parameter for {bad:?}, got: {err}"
            );
        }

        let raw = PostFilters {
            offset: Some("-1".to_string()),
            ..Default::default()
        };
        let err = validate_filters(&raw).expect_err("a negative offset must be a 400");
        assert!(
            err.starts_with("offset "),
            "the 400 must name the parameter, got: {err}"
        );
    }

    #[test]
    fn a_market_view_can_ask_for_everything_it_needs_in_one_request() {
        let raw = PostFilters {
            kind: Some("listing".to_string()),
            category: Some("electronics".to_string()),
            min_price_cents: Some("100".to_string()),
            max_price_cents: Some("50000".to_string()),
            currency: Some("USD".to_string()),
            item_condition: Some("like_new".to_string()),
            q: Some("laptop".to_string()),
            limit: Some("24".to_string()),
            offset: Some("24".to_string()),
            ..Default::default()
        };

        let filter = validate_filters(&raw).expect("the market view's own query must be legal");
        assert_eq!(filter.kind, Some(PostKind::Listing));
        assert_eq!(filter.category.as_deref(), Some("electronics"));
        assert_eq!(filter.min_price_cents, Some(100));
        assert_eq!(filter.max_price_cents, Some(50_000));
        assert_eq!(filter.currency.as_deref(), Some("USD"));
        assert_eq!(filter.item_condition, Some(ItemCondition::LikeNew));
        assert_eq!(filter.q.as_deref(), Some("laptop"));
        assert_eq!(filter.limit, 24);
        assert_eq!(filter.offset, 24);
    }
}

#[cfg(test)]
mod offer_tests {
    use uuid::Uuid;

    use komun_core::models::{MatchStatus, OfferKind, PostKind};

    use crate::api::conversations::{
        offer_kinds, offers_allowed_on, parse_status, resolve_offer_currency, validate_offer,
        OfferRequest, MAX_NOTE_CHARS,
    };
    use crate::db::conversations::{
        check_accept_actor, check_accept_allowed, check_offer_allowed, check_transition, Thread,
        NOTHING_TO_ACCEPT, SELF_ACCEPT,
    };

    fn request(kind: &str) -> OfferRequest {
        OfferRequest {
            kind: Some(kind.to_string()),
            ..Default::default()
        }
    }

    fn priced(kind: &str, amount_cents: i64) -> OfferRequest {
        OfferRequest {
            kind: Some(kind.to_string()),
            amount_cents: Some(amount_cents),
            ..Default::default()
        }
    }

    /// An aid conversation keeps its plain propose/accept flow and refuses offers.
    #[test]
    fn offers_are_for_listings_and_wanted_ads_and_an_aid_thread_says_so() {
        for kind in [PostKind::Listing, PostKind::Want] {
            offers_allowed_on(kind)
                .unwrap_or_else(|e| panic!("a {kind} thread must accept offers: {e}"));
        }

        for kind in [PostKind::Resource, PostKind::Need, PostKind::Offer] {
            let err = offers_allowed_on(kind).expect_err("an aid thread must refuse offers");
            assert_eq!(err, "offers are for listings and wanted ads");
        }
    }

    /// `PostKind::is_market` is the single source of the split, so a sixth kind lands on one side.
    #[test]
    fn every_kind_the_schema_allows_is_on_one_side_of_that_line() {
        for kind in PostKind::ALL {
            assert_eq!(
                offers_allowed_on(*kind).is_ok(),
                kind.is_market(),
                "{kind} must be allowed exactly when it is a marketplace kind"
            );
        }
    }

    /// Refused by the thread, not the body: one predicate over the two ids on the row.
    #[test]
    fn only_the_two_people_on_a_thread_are_participants() {
        let author = Uuid::now_v7();
        let responder = Uuid::now_v7();
        let stranger = Uuid::now_v7();

        let thread = Thread {
            responder_id: responder,
            author_id: author,
            post_kind: PostKind::Listing,
            post_currency: Some("USD".to_string()),
        };

        assert!(
            thread.is_participant(author),
            "the post's author is a participant"
        );
        assert!(
            thread.is_participant(responder),
            "the responder is a participant"
        );
        assert!(
            !thread.is_participant(stranger),
            "anyone else must be refused: this is the 403"
        );
    }

    #[test]
    fn every_kind_the_check_allows_is_accepted_and_nothing_else_is() {
        for kind in [OfferKind::Offer, OfferKind::Counter] {
            let valid = validate_offer(&priced(kind.as_str(), 2500))
                .unwrap_or_else(|e| panic!("{kind} must be accepted: {e}"));
            assert_eq!(valid.kind, kind);
        }
        assert_eq!(
            validate_offer(&priced("accept", 2000))
                .expect("accept is a kind")
                .kind,
            OfferKind::Accept
        );
        assert_eq!(
            validate_offer(&request("decline"))
                .expect("decline is a kind")
                .kind,
            OfferKind::Decline
        );

        // The API must reject `bid` before the database constraint does.
        let err = validate_offer(&priced("bid", 100)).expect_err("'bid' is not an offer kind");
        for kind in OfferKind::ALL {
            assert!(
                err.contains(kind.as_str()),
                "the 400 must list {kind}, got: {err}"
            );
        }
        assert!(
            err.contains("bid"),
            "the 400 must echo the bad value, got: {err}"
        );
        assert_eq!(offer_kinds(), "offer, counter, accept, decline");
    }

    #[test]
    fn a_missing_kind_is_a_400_that_says_what_the_field_takes() {
        for raw in [OfferRequest::default(), request(""), request("   ")] {
            let err = validate_offer(&raw).expect_err("an offer with no kind is not an offer");
            assert!(
                err.contains("kind"),
                "the 400 must name the field, got: {err}"
            );
            assert!(
                err.contains("counter"),
                "the 400 must list the kinds, got: {err}"
            );
        }
    }

    /// `offer`, `counter` and `accept` state a number; without one there is nothing to agree to.
    #[test]
    fn the_three_kinds_that_carry_a_number_require_one() {
        for kind in [OfferKind::Offer, OfferKind::Counter, OfferKind::Accept] {
            let err = validate_offer(&request(kind.as_str()))
                .expect_err("a priced step with no amount must be a 400");
            assert!(
                err.contains("amount_cents"),
                "the 400 must name the field, got: {err}"
            );
            assert!(
                err.contains(kind.as_str()),
                "the 400 must name the kind, got: {err}"
            );
        }
    }

    /// A decline refuses the number already on the thread, so it carries none.
    #[test]
    fn a_decline_carries_no_amount() {
        let err = validate_offer(&priced("decline", 2000))
            .expect_err("a decline with an amount must be a 400");
        assert!(
            err.contains("amount_cents"),
            "the 400 must name the field, got: {err}"
        );
        assert!(
            err.contains("decline"),
            "the 400 must name the kind, got: {err}"
        );

        let valid = validate_offer(&request("decline")).expect("a bare decline is legal");
        assert_eq!(valid.amount_cents, None);
    }

    /// Zero is a real offer; negative is refused here and by `chk_match_offers_amount`.
    #[test]
    fn an_amount_may_be_zero_but_never_negative() {
        assert_eq!(
            validate_offer(&priced("offer", 0))
                .expect("free is a price")
                .amount_cents,
            Some(0)
        );

        for cents in [-1, -2500, i64::MIN] {
            let err = validate_offer(&priced("offer", cents))
                .expect_err("a negative amount must be a 400");
            assert!(
                err.contains("amount_cents") && err.contains("negative"),
                "the 400 must name the field and the reason, got: {err}"
            );
        }
    }

    /// Same rule as `chk_match_offers_currency`, applied first so a bad code is a 400, not a 500.
    #[test]
    fn an_offers_own_currency_is_iso_4217_or_a_400() {
        let valid = validate_offer(&OfferRequest {
            currency: Some("EUR".to_string()),
            ..priced("offer", 2500)
        })
        .expect("EUR is legal");
        assert_eq!(valid.currency.as_deref(), Some("EUR"));

        for bad in ["usd", "US", "USDD", "dollars", "\u{20ac}"] {
            let err = validate_offer(&OfferRequest {
                currency: Some(bad.to_string()),
                ..priced("offer", 2500)
            })
            .expect_err("a non-ISO currency must be a 400");
            assert!(
                err.contains("currency"),
                "the 400 must name the field, got: {err}"
            );
            assert!(
                err.contains(bad),
                "the 400 must echo the bad value, got: {err}"
            );
        }

        // An untouched form field falls through to the precedence rule, not a rejection.
        let valid = validate_offer(&OfferRequest {
            currency: Some("  ".to_string()),
            ..priced("offer", 2500)
        })
        .expect("a blank currency is no currency");
        assert_eq!(valid.currency, None);
    }

    #[test]
    fn a_note_is_trimmed_bounded_and_optional() {
        let valid = validate_offer(&OfferRequest {
            note: Some("  collection only, weekends  ".to_string()),
            ..priced("offer", 2500)
        })
        .expect("a note is legal");
        assert_eq!(valid.note.as_deref(), Some("collection only, weekends"));

        for blank in ["", "   ", "\n\t"] {
            let valid = validate_offer(&OfferRequest {
                note: Some(blank.to_string()),
                ..priced("offer", 2500)
            })
            .expect("a blank note is no note");
            assert_eq!(
                valid.note, None,
                "a whitespace-only note must not be stored"
            );
        }

        let at_limit = "x".repeat(MAX_NOTE_CHARS);
        assert_eq!(
            validate_offer(&OfferRequest {
                note: Some(at_limit.clone()),
                ..priced("offer", 2500)
            })
            .expect("the limit itself is legal")
            .note,
            Some(at_limit)
        );

        let err = validate_offer(&OfferRequest {
            note: Some("x".repeat(MAX_NOTE_CHARS + 1)),
            ..priced("offer", 2500)
        })
        .expect_err("one character past the limit must be a 400");
        assert!(
            err.contains("note"),
            "the 400 must name the field, got: {err}"
        );
        assert!(
            err.contains(&MAX_NOTE_CHARS.to_string()),
            "the 400 must state the limit, got: {err}"
        );
    }

    /// Characters, not bytes: a byte limit would cut a multi-byte note short.
    #[test]
    fn the_note_limit_counts_characters_not_bytes() {
        let cyrillic = "\u{434}".repeat(MAX_NOTE_CHARS);
        assert!(
            cyrillic.len() > MAX_NOTE_CHARS,
            "the fixture must actually be longer in bytes than in characters"
        );
        validate_offer(&OfferRequest {
            note: Some(cyrillic),
            ..priced("offer", 2500)
        })
        .expect("500 characters is 500 characters in any script");
    }

    #[test]
    fn the_currency_precedence_is_offer_then_post_then_server_default() {
        assert_eq!(
            resolve_offer_currency(Some("GBP"), Some("USD"), Some("EUR")),
            Ok("GBP".to_string()),
            "the offer's own currency wins outright"
        );
        assert_eq!(
            resolve_offer_currency(None, Some("USD"), Some("EUR")),
            Ok("USD".to_string()),
            "a counter with no currency inherits the listing's"
        );
        assert_eq!(
            resolve_offer_currency(None, None, Some("EUR")),
            Ok("EUR".to_string()),
            "the server default is the last resort, not the first"
        );
    }

    /// There is no fourth step: a guessed currency looks exactly like a deliberate choice.
    #[test]
    fn with_no_currency_anywhere_the_offer_is_refused_rather_than_priced_in_a_guess() {
        let err = resolve_offer_currency(None, None, None)
            .expect_err("an amount with no currency must not be stored");
        assert!(
            err.contains("currency"),
            "the 400 must name what is missing, got: {err}"
        );
        assert!(
            err.contains("[market] default_currency"),
            "the 400 must name the key an operator would set, got: {err}"
        );
    }

    /// The happy walk as a sequence: `proposed -> accepted -> completed`.
    #[test]
    fn the_offer_counter_accept_complete_walk_is_legal_at_every_step() {
        check_accept_allowed(MatchStatus::Proposed).expect("a proposed thread may be accepted");
        check_transition(MatchStatus::Proposed, MatchStatus::Accepted).expect("accept");
        check_transition(MatchStatus::Accepted, MatchStatus::Completed).expect("complete");
    }

    /// The whole matrix, so an unconsidered transition is decided here.
    #[test]
    fn the_transition_matrix_allows_exactly_five_moves() {
        use MatchStatus::*;

        let legal = [
            (Proposed, Proposed),
            (Proposed, Accepted),
            (Proposed, Withdrawn),
            (Accepted, Completed),
            (Accepted, Withdrawn),
        ];

        for from in MatchStatus::ALL {
            for to in MatchStatus::ALL {
                let expected = legal.contains(&(*from, *to));
                let actual = check_transition(*from, *to).is_ok();
                assert_eq!(
                    actual,
                    expected,
                    "{from} -> {to} must be {}",
                    if expected { "allowed" } else { "refused" }
                );
            }
        }
    }

    /// A refusal must name the current status, the one fact the caller lacks.
    #[test]
    fn every_refusal_names_the_status_the_conversation_is_actually_in() {
        for from in MatchStatus::ALL {
            for to in MatchStatus::ALL {
                if let Err(why) = check_transition(*from, *to) {
                    assert!(
                        why.contains(from.as_str()),
                        "{from} -> {to} refused without naming the current status: {why}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_two_illegal_moves_the_card_names_are_409s_that_name_the_current_status() {
        let err = check_transition(MatchStatus::Proposed, MatchStatus::Completed)
            .expect_err("a deal cannot be completed before it is accepted");
        assert!(err.contains("proposed"), "got: {err}");

        let err = check_transition(MatchStatus::Completed, MatchStatus::Accepted)
            .expect_err("a completed deal cannot be re-accepted");
        assert!(err.contains("completed"), "got: {err}");
    }

    /// Going back to `proposed` would leave an agreed price on a live negotiation.
    #[test]
    fn an_accepted_thread_cannot_be_walked_back_to_proposed() {
        check_transition(MatchStatus::Accepted, MatchStatus::Proposed)
            .expect_err("un-accepting must not be a status change");
    }

    /// Withdraw is legal from either live state, never from a terminal one.
    #[test]
    fn withdraw_is_legal_while_a_deal_is_live_and_not_after_it_is_over() {
        check_transition(MatchStatus::Proposed, MatchStatus::Withdrawn).expect("from proposed");
        check_transition(MatchStatus::Accepted, MatchStatus::Withdrawn).expect("from accepted");
        check_transition(MatchStatus::Completed, MatchStatus::Withdrawn)
            .expect_err("a completed deal is not withdrawable");
        check_transition(MatchStatus::Withdrawn, MatchStatus::Withdrawn)
            .expect_err("withdrawing twice is a conflict, not a no-op");
    }

    /// Accepting is only legal from `proposed`; anything else refuses and names itself.
    #[test]
    fn an_accept_is_refused_on_any_thread_that_is_no_longer_proposed() {
        check_accept_allowed(MatchStatus::Proposed).expect("the only state an accept is legal in");

        for current in [
            MatchStatus::Accepted,
            MatchStatus::Completed,
            MatchStatus::Withdrawn,
        ] {
            let err = check_accept_allowed(current)
                .expect_err("an accept on a thread that is no longer proposed must be refused");
            assert!(
                err.contains(current.as_str()),
                "the 409 must say the thread is already '{current}', got: {err}"
            );
        }
    }

    /// The counterparty accepts; accepting your own offer is one person writing both halves.
    #[test]
    fn you_cannot_accept_your_own_offer() {
        let me = Uuid::now_v7();
        let them = Uuid::now_v7();

        check_accept_actor(Some(them), me).expect("accepting the other party's offer is the point");

        let err = check_accept_actor(Some(me), me).expect_err("a self-accept must be refused");
        assert_eq!(err, SELF_ACCEPT);
        assert!(
            err.contains("your own offer"),
            "the 409 must say what was wrong with it, got: {err}"
        );
    }

    /// An accept with nothing to accept would write a price one party picked alone.
    #[test]
    fn an_accept_before_any_offer_is_refused() {
        let err = check_accept_actor(None, Uuid::now_v7())
            .expect_err("there is nothing to accept on an empty thread");
        assert_eq!(err, NOTHING_TO_ACCEPT);
    }

    #[test]
    fn the_status_patch_takes_the_four_values_the_check_allows() {
        for status in MatchStatus::ALL {
            assert_eq!(parse_status(status.as_str()), Ok(*status));
        }
        assert_eq!(parse_status("  completed  "), Ok(MatchStatus::Completed));

        for bad in ["", "   ", "sold", "Accepted", "done"] {
            let err = parse_status(bad).expect_err("an unknown status must be a 400");
            assert!(
                err.contains("status"),
                "the 400 must name the field, got: {err}"
            );
            for status in MatchStatus::ALL {
                assert!(
                    err.contains(status.as_str()),
                    "the 400 must list {status}, got: {err}"
                );
            }
        }
    }

    /// Rows are append-only: this reads the only module that may touch the table and asserts no
    /// `UPDATE` or `DELETE` reaches it, which the runtime gate cannot prove.
    #[test]
    fn no_code_updates_or_deletes_an_offer_row() {
        const DB_CONVERSATIONS: &str = include_str!("../db/conversations.rs");

        let statements: Vec<&str> = DB_CONVERSATIONS
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with("//"))
            .filter(|line| {
                let upper = line.to_uppercase();
                (upper.contains("UPDATE ") || upper.contains("DELETE "))
                    && upper.contains("MATCH_OFFERS")
            })
            .collect();

        assert!(
            statements.is_empty(),
            "match_offers is append-only, but this module rewrites it: {statements:?}"
        );
        assert!(
            DB_CONVERSATIONS.contains("INSERT INTO match_offers"),
            "the fixture path must be wrong: no insert into match_offers was found at all"
        );
    }

    /// Oldest-first with a deterministic tie-break, or ordering fails whenever rows tie.
    #[test]
    fn the_offer_list_is_ordered_oldest_first_and_breaks_ties_deterministically() {
        const DB_CONVERSATIONS: &str = include_str!("../db/conversations.rs");

        assert!(
            DB_CONVERSATIONS.contains("ORDER BY created_at ASC, id ASC"),
            "list_offers must order by created_at and then by id"
        );
    }

    /// A withdrawn thread must not take a new offer: it would render as a live number with no
    /// answer coming.
    #[test]
    fn a_thread_that_is_over_does_not_take_a_new_offer() {
        check_offer_allowed(MatchStatus::Proposed).expect("a live negotiation takes offers");
        check_offer_allowed(MatchStatus::Accepted)
            .expect("a counter after an accept is renegotiation, not a reopened deal");

        for over in [MatchStatus::Withdrawn, MatchStatus::Completed] {
            let err = check_offer_allowed(over)
                .expect_err("a thread that is over must refuse a new offer");
            assert!(
                err.contains(over.as_str()),
                "the 409 must name the status the thread is in, got: {err}"
            );
            assert!(
                err.contains("offer"),
                "the 409 must say what was refused, got: {err}"
            );
        }
    }

    /// `accepted` is a legal place to counter from and not to accept from.
    #[test]
    fn appending_an_offer_and_accepting_one_are_allowed_in_different_states() {
        check_offer_allowed(MatchStatus::Accepted).expect("countering an accepted deal is legal");
        check_accept_allowed(MatchStatus::Accepted)
            .expect_err("accepting twice is not, or the agreed price is rewritten");
    }

    /// The read must go through `participant_thread`, the same guard the offer routes use, or an
    /// outsider gets a 500 instead of a 403.
    #[test]
    fn the_thread_read_refuses_an_outsider_through_the_same_guard_the_offer_routes_use() {
        const API_CONVERSATIONS: &str = include_str!("../api/conversations.rs");

        let get_conversation = API_CONVERSATIONS
            .split("async fn get_conversation")
            .nth(1)
            .expect("api::conversations must still have a get_conversation handler");
        let body = &get_conversation[..get_conversation
            .find("\nasync fn ")
            .unwrap_or(get_conversation.len())];

        assert!(
            body.contains("participant_thread("),
            "the thread read must resolve the 404/403 pair the way every other route on a \
             thread does, or a non-participant gets a 500 again"
        );
    }

    /// Only the first completion may sell a listing, or the recorded buyer is overwritten.
    #[test]
    fn only_the_first_completion_sells_a_listing() {
        const DB_CONVERSATIONS: &str = include_str!("../db/conversations.rs");

        assert!(
            DB_CONVERSATIONS.contains("this listing is already sold"),
            "completing a deal on a listing that already has sold_at must be a 409"
        );
        assert!(
            DB_CONVERSATIONS.contains("FOR UPDATE OF p"),
            "the sold check must hold the post row for the rest of the transaction, or two \
             completions racing on one listing both read NULL and both write"
        );
    }
}

/// Pure-unit tests over review rules; the status codes and aggregate arithmetic belong to the
/// runtime gate.
#[cfg(test)]
mod review_tests {
    use uuid::Uuid;

    use komun_core::models::{MatchStatus, PostKind};

    use crate::api::categories::bad_request;
    use crate::api::reviews::{
        validate_page, validate_review, ReviewPage, ReviewRequest, MAX_BODY_CHARS, MAX_RATING,
        MIN_RATING,
    };
    use crate::db::conversations::Thread;
    use crate::db::posts::{DEFAULT_LIMIT, MAX_LIMIT};
    use crate::db::reviews::{check_reviewable, ALREADY_REVIEWED};

    const DB_REVIEWS: &str = include_str!("../db/reviews.rs");
    const API_REVIEWS: &str = include_str!("../api/reviews.rs");
    const DB_USERS: &str = include_str!("../db/users.rs");
    const SCHEMA: &str = include_str!("../../../../migrations/001_schema.sql");

    fn rated(rating: serde_json::Value) -> ReviewRequest {
        ReviewRequest {
            rating: Some(rating),
            ..Default::default()
        }
    }

    #[test]
    fn every_rating_the_check_allows_is_accepted() {
        for stars in MIN_RATING..=MAX_RATING {
            let valid = validate_review(&rated(stars.into()))
                .unwrap_or_else(|e| panic!("{stars} stars must be accepted: {e}"));
            assert_eq!(valid.rating, stars as i16);
            assert_eq!(valid.body, None, "a body is optional");
        }

        let valid = validate_review(&ReviewRequest {
            rating: Some(5.into()),
            body: Some("Smooth pickup".to_string()),
        })
        .expect("a review with a note is the ordinary case");
        assert_eq!(valid.rating, 5);
        assert_eq!(valid.body.as_deref(), Some("Smooth pickup"));
    }

    /// `0` and `6` are 400s here, not constraint violations arriving as 500s.
    #[test]
    fn a_rating_outside_the_star_range_is_a_400_that_states_the_range() {
        for stars in [0, 6, -1, 100, i64::MIN, i64::MAX] {
            let err = validate_review(&rated(stars.into()))
                .unwrap_err_or_panic(&format!("{stars} is not a star rating"));
            assert!(
                err.contains("rating"),
                "the 400 must name the field, got: {err}"
            );
            assert!(
                err.contains(&MIN_RATING.to_string()) && err.contains(&MAX_RATING.to_string()),
                "the 400 must state the range, got: {err}"
            );
            assert!(
                err.contains(&stars.to_string()),
                "the 400 must echo the bad value, got: {err}"
            );
        }
    }

    /// A rating is a whole star count, so it arrives as `serde_json::Value` to yield a 400
    /// rather than serde's 422.
    #[test]
    fn a_rating_that_is_not_a_whole_number_is_a_400_rather_than_a_422() {
        for not_a_rating in [
            serde_json::json!(4.5),
            serde_json::json!("5"),
            serde_json::json!(true),
            serde_json::json!([5]),
            serde_json::json!({"stars": 5}),
        ] {
            let err = validate_review(&rated(not_a_rating.clone()))
                .unwrap_err_or_panic(&format!("{not_a_rating} is not a rating"));
            assert!(
                err.contains("rating"),
                "the 400 must name the field, got: {err}"
            );
            assert!(
                err.contains("whole number"),
                "the 400 must say what a rating is, got: {err}"
            );
        }
    }

    #[test]
    fn a_missing_rating_is_a_400_that_says_what_the_field_takes() {
        for raw in [ReviewRequest::default(), rated(serde_json::Value::Null)] {
            let err = validate_review(&raw).unwrap_err_or_panic("a review with no rating");
            assert!(
                err.contains("rating"),
                "the 400 must name the field, got: {err}"
            );
            assert!(
                err.contains(&MAX_RATING.to_string()),
                "the 400 must state the range, got: {err}"
            );
        }
    }

    #[test]
    fn a_body_is_optional_trimmed_and_bounded() {
        let valid = validate_review(&ReviewRequest {
            rating: Some(5.into()),
            body: Some("  Smooth pickup, arrived on time  ".to_string()),
        })
        .expect("a written review is legal");
        assert_eq!(
            valid.body.as_deref(),
            Some("Smooth pickup, arrived on time")
        );

        for blank in ["", "   ", "\n\t"] {
            let valid = validate_review(&ReviewRequest {
                rating: Some(4.into()),
                body: Some(blank.to_string()),
            })
            .expect("a blank body is no body");
            assert_eq!(
                valid.body, None,
                "a whitespace-only body must be stored as NULL, not as spaces"
            );
        }

        let at_limit = "x".repeat(MAX_BODY_CHARS);
        assert_eq!(
            validate_review(&ReviewRequest {
                rating: Some(5.into()),
                body: Some(at_limit.clone()),
            })
            .expect("the limit itself is legal")
            .body,
            Some(at_limit)
        );

        let err = validate_review(&ReviewRequest {
            rating: Some(5.into()),
            body: Some("x".repeat(MAX_BODY_CHARS + 1)),
        })
        .unwrap_err_or_panic("one character past the limit");
        assert!(
            err.contains("body"),
            "the 400 must name the field, got: {err}"
        );
        assert!(
            err.contains(&MAX_BODY_CHARS.to_string()),
            "the 400 must state the limit, got: {err}"
        );
    }

    /// Characters, not bytes.
    #[test]
    fn the_body_limit_counts_characters_not_bytes() {
        let cyrillic = "\u{434}".repeat(MAX_BODY_CHARS);
        assert!(
            cyrillic.len() > MAX_BODY_CHARS,
            "the fixture must actually be longer in bytes than in characters"
        );
        validate_review(&ReviewRequest {
            rating: Some(5.into()),
            body: Some(cyrillic),
        })
        .expect("2,000 characters is 2,000 characters in any script");
    }

    /// Pins the 400 mapping every message above goes through.
    #[test]
    fn a_rejected_review_body_leaves_as_a_400() {
        use axum::response::IntoResponse;

        let err = validate_review(&rated(6.into())).unwrap_err_or_panic("6 stars");
        let response = bad_request(err).into_response();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    /// Only a completed deal is reviewable; the 409 names the current status.
    #[test]
    fn a_review_is_only_writable_against_a_completed_deal() {
        check_reviewable(MatchStatus::Completed).expect("a completed deal is reviewable");

        assert_eq!(
            check_reviewable(MatchStatus::Accepted)
                .unwrap_err_or_panic("an accepted deal is not done yet"),
            "this deal is not completed (status: accepted)"
        );

        for open in [
            MatchStatus::Proposed,
            MatchStatus::Accepted,
            MatchStatus::Withdrawn,
        ] {
            let err =
                check_reviewable(open).unwrap_err_or_panic("only a completed deal may be reviewed");
            assert!(
                err.contains(open.as_str()),
                "the 409 must name the current status, got: {err}"
            );
            assert!(
                err.contains("not completed"),
                "the 409 must say what is missing, got: {err}"
            );
        }
    }

    /// Every status falls on one side, so a fifth is decided here.
    #[test]
    fn exactly_one_status_is_reviewable() {
        for status in MatchStatus::ALL {
            assert_eq!(
                check_reviewable(*status).is_ok(),
                *status == MatchStatus::Completed,
                "{status} must be reviewable exactly when it is the completed one"
            );
        }
    }

    /// The reviewee is the other participant, never client-supplied; `None` is the 403.
    #[test]
    fn the_reviewee_is_the_other_participant_and_a_stranger_has_none() {
        let author = Uuid::now_v7();
        let responder = Uuid::now_v7();
        let stranger = Uuid::now_v7();

        let thread = Thread {
            responder_id: responder,
            author_id: author,
            post_kind: PostKind::Listing,
            post_currency: Some("USD".to_string()),
        };

        assert_eq!(
            thread.other_participant(author),
            Some(responder),
            "the seller reviews the buyer"
        );
        assert_eq!(
            thread.other_participant(responder),
            Some(author),
            "and the buyer reviews the seller"
        );
        assert_eq!(
            thread.other_participant(stranger),
            None,
            "anyone else has no counterparty here: this is the 403"
        );
    }

    /// An unverified account cannot review: the write route goes through `require_auth`, not
    /// `require_session`, which authenticates without demanding verification.
    #[test]
    fn writing_a_review_goes_through_the_middleware_that_refuses_unverified_accounts() {
        assert!(
            API_REVIEWS.contains("require_auth"),
            "the review write route must be layered with require_auth"
        );
        assert!(
            !API_REVIEWS.contains("require_session"),
            "require_session authenticates without demanding a verified address: an unverified \
             account would be able to review"
        );
        assert!(
            API_REVIEWS.contains("post(create_review)"),
            "the fixture path must be wrong: no review write route was found at all"
        );
    }

    /// One review per (match, reviewer); the `23505` must map to a 409, not an "internal error".
    #[test]
    fn a_duplicate_review_is_mapped_from_the_constraint_to_a_conflict() {
        assert_eq!(ALREADY_REVIEWED, "you have already reviewed this deal");

        assert!(
            SCHEMA.contains("UNIQUE (match_id, reviewer_id)"),
            "the one-review-per-deal rule is the database's, and this is the constraint"
        );
        assert!(
            DB_REVIEWS.contains("23505"),
            "the unique violation must be mapped explicitly, or it surfaces as a 500"
        );
        assert!(
            DB_REVIEWS.contains("DealStep::Conflict(ALREADY_REVIEWED"),
            "the mapped violation must become the 409's message"
        );
    }

    /// Reviews are a record: no path may rewrite or remove one.
    #[test]
    fn no_code_updates_or_deletes_a_review_row() {
        let statements: Vec<&str> = DB_REVIEWS
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with("//"))
            .filter(|line| {
                let upper = line.to_uppercase();
                (upper.contains("UPDATE ") || upper.contains("DELETE "))
                    && upper.contains("DEAL_REVIEWS")
            })
            .collect();

        assert!(
            statements.is_empty(),
            "deal_reviews is append-only, but this module rewrites it: {statements:?}"
        );
        assert!(
            DB_REVIEWS.contains("INSERT INTO deal_reviews"),
            "the fixture path must be wrong: no insert into deal_reviews was found at all"
        );
    }

    /// The same limit/offset convention and constants as `GET /api/posts`.
    #[test]
    fn the_review_list_paginates_the_way_every_other_list_endpoint_does() {
        assert_eq!(
            validate_page(&ReviewPage::default()).expect("no parameters is a legal page"),
            (DEFAULT_LIMIT, 0),
            "an unbounded list is not an option"
        );

        assert_eq!(
            validate_page(&ReviewPage {
                limit: Some("20".to_string()),
                offset: Some("40".to_string()),
            })
            .expect("a page is legal"),
            (20, 40)
        );

        assert_eq!(
            validate_page(&ReviewPage {
                limit: Some(MAX_LIMIT.to_string()),
                offset: None,
            })
            .expect("the maximum is legal")
            .0,
            MAX_LIMIT
        );

        // A blank parameter is an untouched form field, not a rejection.
        assert_eq!(
            validate_page(&ReviewPage {
                limit: Some("  ".to_string()),
                offset: Some(String::new()),
            })
            .expect("blank parameters must not be errors"),
            (DEFAULT_LIMIT, 0)
        );
    }

    /// Rejected rather than clamped, so a caller cannot silently get the wrong page.
    #[test]
    fn a_bad_page_names_the_parameter_it_refuses() {
        for bad in [
            "0".to_string(),
            "-5".to_string(),
            (MAX_LIMIT + 1).to_string(),
            "all".to_string(),
        ] {
            let err = validate_page(&ReviewPage {
                limit: Some(bad.clone()),
                offset: None,
            })
            .unwrap_err_or_panic(&format!("limit {bad:?}"));
            assert!(
                err.starts_with("limit "),
                "the 400 must name the parameter for {bad:?}, got: {err}"
            );
        }

        let err = validate_page(&ReviewPage {
            limit: None,
            offset: Some("-1".to_string()),
        })
        .unwrap_err_or_panic("a negative offset");
        assert!(
            err.starts_with("offset "),
            "the 400 must name the parameter, got: {err}"
        );
    }

    /// Newest first with a deterministic tie-break, or a row can land on two pages or none.
    #[test]
    fn reviews_come_back_newest_first_and_carry_who_wrote_them() {
        assert!(
            DB_REVIEWS.contains("ORDER BY r.created_at DESC, r.id DESC"),
            "the list must be newest first with a deterministic tie-break"
        );
        assert!(
            DB_REVIEWS.contains("u.display_name AS reviewer_display_name"),
            "SPEC B4: reviews are attributed, never anonymous"
        );
        assert!(
            DB_REVIEWS.contains("LIMIT $2 OFFSET $3"),
            "the list must be paginated in SQL, not after fetching every row"
        );
    }

    /// The aggregate is computed from the review rows on every read; `users` carries no counter
    /// column.
    #[test]
    fn the_profile_aggregate_is_computed_from_the_reviews_not_from_a_counter() {
        assert!(
            DB_USERS.contains("FROM deal_reviews WHERE reviewee_id = u.id"),
            "rating_avg and rating_count must be computed from the review rows"
        );
        assert!(
            DB_USERS.contains("ROUND(AVG(rating), 1)"),
            "M3.4: the mean is reported to one decimal"
        );
        assert!(
            DB_USERS.contains("COUNT(*)::bigint as rating_count"),
            "the count must come from the same rows as the mean"
        );

        let users_table = SCHEMA
            .split("CREATE TABLE users")
            .nth(1)
            .expect("the schema creates a users table");
        let users_table = &users_table[..users_table.find(");").expect("the table is terminated")];
        for column in ["rating_avg", "rating_count", "rating_total", "review_count"] {
            assert!(
                !users_table.contains(column),
                "M3.4 forbids a denormalised counter, but `users` has {column}"
            );
        }
    }

    /// `null` and `0.0` differ: an unreviewed user must not read as rated zero.
    #[test]
    fn an_unreviewed_user_has_no_average_rather_than_an_average_of_zero() {
        assert!(
            !DB_USERS.contains("COALESCE(r.rating_avg"),
            "coalescing the mean would report an unreviewed user as rated 0.0"
        );
        assert!(
            DB_USERS.contains("COALESCE(r.rating_count, 0)"),
            "a count, unlike a mean, does have a right answer when there are no rows: 0"
        );

        assert!(
            DB_USERS.contains("pub rating_avg: Option<f64>"),
            "rating_avg must be nullable all the way out to the JSON"
        );
        assert!(
            DB_USERS.contains("pub rating_count: i64"),
            "rating_count is never null: zero reviews is a number"
        );
    }

    /// Helper so every negative case fails loudly rather than passing by accident.
    trait UnwrapErrOrPanic {
        fn unwrap_err_or_panic(self, what: &str) -> String;
    }

    impl<T: std::fmt::Debug> UnwrapErrOrPanic for Result<T, String> {
        fn unwrap_err_or_panic(self, what: &str) -> String {
            match self {
                Err(why) => why,
                Ok(value) => panic!("{what} must be refused, but it was accepted as {value:?}"),
            }
        }
    }
}
