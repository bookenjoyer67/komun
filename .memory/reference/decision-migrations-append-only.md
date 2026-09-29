---
classification: internal
project: proj-komun
doc_type: decision
---

# Decision: migrations are append-only

Which files may change under `migrations/`, and what breaks when one is edited?

`migrations/001_schema.sql` is the schema baseline and is frozen (`docs/DEVELOPMENT.md:115` `It is checksum-bookmarked in every existing`). It is applied by hand exactly once, and its `sha384` is inserted as the bookmark (`docs/DEVELOPMENT.md:80` `CHECKSUM=$(sha384sum migrations/001_schema.sql | cut -d' ' -f1)`). Editing one byte makes every existing server refuse to boot (`docs/DEVELOPMENT.md:116` `changing one byte makes every server refuse to boot with a checksum mismatch.`). The file therefore is never edited, not even to fix a typo.

## How does new schema arrive instead?

Schema changes arrive as new numbered files, and an applied migration is never edited (`docs/DATABASE.md:94` `Schema changes are additive files: `002_*.sql`, `003_*.sql`, …`). The migrator runs the whole directory at boot (`crates/server/src/main.rs:78` `sqlx::migrate!("../../migrations")`).

## Which rules bind a migration author?

- Add a new file rather than editing an applied migration.
- Write no down migration, because the migrator only runs pending files forward (`docs/DATABASE.md:96` `No down migrations; the migrator runs pending files on startup.`).
- Pair every new `CHECK` list with a matching Rust enum, because `crates/core` parses `001_schema.sql` at test time (`docs/DEVELOPMENT.md:119` `a new CHECK without a matching enum fails the test by design`).
- Expect a failed migration to roll its own bookkeeping row back with it.

Nothing else may change `001`, because the checksum is the reason existing deployments still boot (`docs/DEVELOPMENT.md:95` `A wrong or missing checksum makes every later boot fail.`).
