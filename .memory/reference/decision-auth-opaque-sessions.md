---
classification: internal
project: proj-komun
doc_type: decision
---

# Decision: opaque database sessions, no JWT

How is a Komun request authenticated, and what is deliberately absent from that design?

Auth is email plus a password-derived verifier, and sessions are opaque database rows (`AGENTS.md:135` `There is no ed25519 key and no JWT`). The schema stores only a token hash (`migrations/001_schema.sql:38` `token_hash BYTEA NOT NULL UNIQUE,`). A client holds a 256-bit token, and the database holds `SHA-256(raw token)`, so a dump contains no replayable credential (`crates/server/src/db/sessions.rs:4` `which is `SHA-256(raw token)`. A database dump therefore contains no`).

## When does a session stop working?

Every lookup filters on both expiry and non-revocation (`crates/server/src/db/sessions.rs:84` `AND s.expires_at > now()`), and cleanup deletes rows expired more than 30 days ago (`crates/server/src/db/sessions.rs:160` `DELETE FROM sessions WHERE expires_at < now() - interval '30 days'`). The default lifetime is 30 days (`crates/server/src/sessions.rs:19` `pub const DEFAULT_LIFETIME_DAYS: i64 = 30;`), and the config default reads that one constant (`crates/server/src/config.rs:271` `token_lifetime_days: crate::sessions::DEFAULT_LIFETIME_DAYS as u32,`).

## Why load the role on every request instead of signing it into a token?

The session row carries the user id and the role, so a demotion takes effect immediately (`` `docs/ARCHITECTURE.md:114` `Protected routes run `require_auth`, which loads the session row (user id **and** role, so a` ``). Middleware names four levels (`AGENTS.md:173` `require_session` / `require_auth` / `require_admin` / `require_superadmin`). Passwords are stored as an Argon2id verifier with a second independent Argon2id over it (`AGENTS.md:215` `a second, independent Argon2id over the verifier before storage`).
