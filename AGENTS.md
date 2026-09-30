# AGENTS.md — Komun

Cold-start guide for an agent working in this repo. Keep it accurate: if code and this file
disagree, fix one of them.

## What this is

What is Komun, what is it built from, and what has been deliberately left out of it?

Komun is a **single-server** mutual-aid web app (`docs/ARCHITECTURE.md:5` `A single-server
mutual-aid web app.`). People post needs, offers and resources, plus marketplace listings and wants
(`crates/core/src/models/post.rs:8-12` `Resource => "resource",` through `Want => "want",`). They
search those posts (`crates/server/src/api/mod.rs:44` `.merge(search::router(state.clone()))`). They
negotiate over conversations whose `messages` rows hold `ciphertext` and `nonce` only
(`migrations/001_schema.sql:232-233` `ciphertext BYTEA NOT NULL,` / `nonce BYTEA,`; `:227`
`-- Message content is never readable by the server: ciphertext only, no plaintext body.`). That
ciphertext is produced in the browser, in WASM (`crates/wasm/src/lib.rs:49` `pub fn
encrypt_message(plaintext: &[u8], recipient_x25519_pk: &[u8])`). For a completed deal a participant
leaves a star review, and completion is checked inside the insert transaction
(`crates/server/src/api/reviews.rs:95` `// Whether the deal is completed is decided inside the
transaction, with the row locked —`). A rating is one to five stars
(`migrations/001_schema.sql:264` `CONSTRAINT chk_deal_reviews_rating CHECK (rating BETWEEN 1 AND
5),`).

The backend is Rust with Axum (`crates/server/Cargo.toml:9` `axum = { version = "0.8", features =
["ws", "multipart"] }`). Queries go through sqlx (`crates/server/Cargo.toml:21` `sqlx = { version =
"0.8"`). The database is PostgreSQL 16 (`docker-compose.yml:3` `image: postgres:16-alpine`). The
frontend is a SvelteKit 5 SPA built by the static adapter **[UNVERIFIED]** (`web/package.json:20`
`"svelte": "^5.0.0",`; `web/package.json:13` `"@sveltejs/kit": "^2.0.0",`; `web/package.json:12`
`"@sveltejs/adapter-static": "^3.0.0",`). The licence is
AGPL-3.0-or-later (`Cargo.toml:8` `license = "AGPL-3.0-or-later"`).

There is no multi-tenant community layer and no federation (`crates/server/src/db/mod.rs:1`
`` // Merged: `communities/` (A) and `alliances/`, `federation/` (B) are all deleted. ``). There is
no relay crate in the workspace (`Cargo.toml:2` `members = ["crates/server", "crates/core",
"crates/wasm"]`). Nginx proxies no WebSocket route (`deploy/nginx-komun.conf:6` `# There is no relay
and no WebSocket route to proxy — the server is plain HTTP + JSON.`). Those parts were deleted in
the reshape (`docs/ARCHITECTURE.md:45` `` `komun-relay` and the `federation/` module were deleted in
the reshape ``). There are no payment rails (`rg -i 'stripe|paypal|payment|escrow|billing' crates
migrations` -> one hit, `crates/wasm/src/lib.rs:1585` `"payment",`, a recovery-code wordlist entry).

**Claims needing verification**

- The frontend is a **SvelteKit 5** SPA **[UNVERIFIED]**. The manifest records SvelteKit at major 2
  and Svelte at major 5 (`web/package.json:13` `"@sveltejs/kit": "^2.0.0",`; `web/package.json:20`
  `"svelte": "^5.0.0",`), so no line in it settles "SvelteKit 5". A ruling on whether the intended
  term is "Svelte 5" or "SvelteKit 2" would settle it.

## Critical rules

Which constraints must an agent respect before changing anything in this repository?

### Never commit these
Which paths must never enter a commit, and why is each one kept out?

- `config.toml` — gitignored (`.gitignore:8` `config.toml`), holds the DB URL
  (`config.example.toml:12` `url = "postgres://komun:komun@localhost:5432/komun"`) and optional SMTP
  credentials (`config.example.toml:84` `# smtp_host = "smtp.example.org"`)
- `.env` / `.env.local` — gitignored (`.gitignore:6-7` `.env` / `.env.local`)
- `crates/wasm/pkg/` — build artifact, gitignored (`.gitignore:5` `crates/wasm/pkg/`)
- `web/build/` — build artifact, gitignored (`.gitignore:3` `web/build/`)
- `data/avatars/`, `data/post-images/` — runtime uploads, gitignored (`.gitignore:12-13`
  `data/avatars/` / `data/post-images/`)

### Build order
In what order do the three builds run, and which step stands alone?

The wasm package must exist before the frontend is installed (`web/package.json:33` `"komun-wasm":
"file:../crates/wasm/pkg"`). The server does **not** serve the SPA; nginx does
(`deploy/nginx-komun.conf:30` `# SvelteKit static build (adapter-static) with an SPA fallback.`;
`:33` `try_files $uri $uri/ /index.html;`). The router mounts only `/api`, `/avatars` and
`/post-images` (`crates/server/src/main.rs:124-126` `.nest("/api", api::router(state.clone()))`,
`.nest_service("/avatars", ServeDir::new(&avatar_dir))`, `.nest_service("/post-images",
ServeDir::new(&post_img_dir))`). Step 3 therefore depends on neither of the first two
(`crates/server/Cargo.toml:8` `komun-core = { path = "../core" }`):

```bash
wasm-pack build crates/wasm --target web     # 1. -> crates/wasm/pkg/
cd web && npm install && npm run build       # 2. package.json needs pkg/ to exist
cargo build --release --bin komun-server     # 3. independent of 1 and 2
```

Rebuild the wasm package **and** the frontend after any crypto change in `crates/wasm/`
(`web/package.json:33` `"komun-wasm": "file:../crates/wasm/pkg"`).

### sqlx uses runtime queries
How are the queries written here, and what does that mean for the Docker build?

All queries use `sqlx::query()` / `sqlx::query_as()`, not the compile-time macros
(`rg 'sqlx::query!|query_as!|query_scalar!|query_file!' crates` -> No matches found). There is no
`cargo sqlx prepare` step and no offline query cache (`Glob {.sqlx/**,migrations/*.sql,docker/*}`
-> 4 paths, none under `.sqlx/`). Docker builds work as-is, and that is verified rather than
assumed: `docker build -f docker/Dockerfile .` finished in 1m51s (`docs/iteration-log.md:937`
`` `docker build` → success in 1m51s ``). That image provisioned a fresh database from `001` to
`003` and answered `/api/health` (`docs/iteration-log.md:938` `` → migrations `001`→`003`,
`/api/health` 200, media directories writable. ``). The builder stage is pinned to the workspace
toolchain (`docker/Dockerfile:5` `FROM rust:1.95-slim-bookworm AS builder`). The old `1.82` cannot
parse the `edition2024` manifest of `aligned 0.4.3` (`docker/Dockerfile:3` `` # needs the
`edition2024` Cargo feature, which Cargo 1.82 cannot parse. ``).

### Frontend is Svelte 5 runes only
Which Svelte dialect does the frontend use, and which constructs are absent from it?

The frontend has no `$:`, no `export let` and no `on:click`
(`rg 'export let|on:click|^\s*\$:' web/src` -> No matches found). Use `$state`, `$derived`,
`$effect`, `$props` and `onclick={handler}` (`web/package.json:20` `"svelte": "^5.0.0",`).

### Migrations are frozen at 001
What may change in `migrations/`, and what must never change?

`migrations/001_schema.sql` is checksum-bookmarked in every provisioned database
(`docs/DEVELOPMENT.md:115` `It is checksum-bookmarked in every existing`). Editing one byte makes
every existing server refuse to boot (`docs/DEVELOPMENT.md:116` `changing one byte makes every server
refuse to boot with a checksum mismatch.`). The migrator runs the whole directory at boot
(`crates/server/src/main.rs:78` `sqlx::migrate!("../../migrations")`). Schema changes are additive
files (`Glob {.sqlx/**,migrations/*.sql,docker/*}` -> `001_schema.sql`,
`002_directory_open_registration.sql`, `003_drop_matches_message.sql`). See `docs/DEVELOPMENT.md`.

### Crypto boundaries
Which boundaries does client-side encryption rest on, and where is each one written down?

- The **x25519 secret key, the password-derived key and the recovery code never leave the client**
  (`docs/CONVENTIONS.md:80` `Keys never leave the client; the server returns no key material or
  recovery code.`). The server stores public keys and wrapped bundles only
  (`docs/CONVENTIONS.md:81` `Only public keys and wrapped key bundles are server-visible`;
  `migrations/001_schema.sql:25-26` `encryption_public_key BYTEA,` / `encrypted_key_bundle BYTEA,`).
  A password-derived *verifier* is sent, but never the password itself
  (`crates/wasm/src/lib.rs:191` `Argon2id over the account password, used for both halves of Part
  1.5's split`; more in `docs/CRYPTO.md`).
- The schema has **no plaintext message column**: `messages` carries `ciphertext` + `nonce` only
  (`migrations/001_schema.sql:227` `-- Message content is never readable by the server: ciphertext
  only, no plaintext body.`). The plaintext `matches.message` column was dropped
  (`migrations/003_drop_matches_message.sql:22` `ALTER TABLE matches DROP COLUMN message;`).
- **Never log** keys, bundles, passwords, derived keys, or message plaintext.
- There is no ed25519 key and no JWT (`rg -i 'ed25519|jwt|jsonwebtoken' crates` -> 5 hits, all
  removal notes, e.g. `crates/server/src/config.rs:59` `the signing-key setting is gone with the
  JWTs`). Sessions are opaque database rows (`migrations/001_schema.sql:38` `token_hash BYTEA NOT
  NULL UNIQUE,`).

## Code layout

| Path | What | Be careful |
|---|---|---|
| `crates/core/` | Shared models + `db_enum!` macro | Changes affect server and client expectations; the enum↔CHECK test lives in `crates/core/src/tests.rs` |
| `crates/server/` | Axum HTTP server, auth/sessions, DB queries, tasks, REPL | Bootstrap in `main.rs`, routes in `api/mod.rs`, config in `config.rs` |
| `crates/wasm/` | Client crypto → WASM (x25519, XChaCha20Poly1305, Argon2, recovery codes) | Breaking changes here break all encryption; rebuild pkg + frontend |
| `web/` | SvelteKit 5 SPA (static adapter, `ssr = false`) | Runes only. `web/src/lib/api/**` holds the shared API helpers, but most calls live in stores and routes: 37 `fetch()` calls to `/api/` in 14 files, 34 of them outside `lib/api/` (`lib/stores/auth.ts` alone holds 17) |
| `migrations/` | `001_schema.sql` (frozen) + additive migrations | Never edit `001`; add `002+` |
| `docs/` | ARCHITECTURE, CONVENTIONS, CRYPTO, DATABASE, DEVELOPMENT, DEPLOY | Plus the quality-control artifacts — `docs/prd.md`, `docs/rubric.md`, `docs/agent-rubric.md`, `docs/iteration-log.md`, `docs/clippy-report.md`, `docs/contract-audit/` — and the Module 1 lab's own copies under `docs/clippy-gate/` (`prd.md`, `rubric.md`, `iteration-log.md`). Keep the prose docs in sync with the code |
| `deploy/` | nginx/OpenRC/setup/seed starting points | Docs only; no relay/WebSocket proxy |
| `config.example.toml` | Documented config template | Keep in sync with `config.rs` defaults **except** `require_email_verification = false` here vs `true` in `config.rs` — deliberate, the example must boot without SMTP (a `true` with no `[email]` refuses to start) |
| `scripts/` | Utility scripts | |

## Quickstart (local dev)

```bash
# config
cp config.example.toml config.toml        # edit [database] url

# build (wasm first!)
wasm-pack build crates/wasm --target web
cd web && npm install && npm run build && cd ..

# run
cargo run --bin komun-server              # -> http://localhost:3000
```

## Key architecture facts

- UUIDv7 primary keys (time-sortable). One exception, per `docs/DATABASE.md`: `avatar_uploads.id`
  is `BIGSERIAL`.
- Auth: email + password verifier (Argon2id), opaque DB sessions; **no JWT**. Middleware is
  `require_session` / `require_auth` / `require_admin` / `require_superadmin`, and it loads the
  role from the DB on every request.
- API: flat `/api/posts`, `/api/search`, `/api/auth/**`, `/api/users/**`, `/api/me/*`,
  `/api/conversations/*`, `/api/categories`, `/api/admin/*`, `/api/matches/{id}/reviews`. Mounted
  too, and easy to miss: `/api/health`, `/api/node`, `/api/geocode`, `/api/link-preview`,
  `/api/search/users`, `/api/posts/{id}/report` and `/hide`, and `/api/directory*` (only when
  `[discovery] directory_enabled = true` — otherwise those routes are not mounted at all and
  404). There are no `/api/alliances` and no `/api/communities` routes.
- Marketplace: `listing` and `want` post kinds carry the price fields; the negotiation on a
  match thread is the append-only `match_offers` trail (append-only by convention — no trigger or
  revoke enforces it); a review is writable only against a `completed` deal. The category
  taxonomy is the seeded, runtime-editable `categories` table (23 rows), not an enum. See
  `docs/ARCHITECTURE.md` and `docs/DATABASE.md`.
- Config is loaded from `config.toml` (or `KOMUN_CONFIG`) with env overrides; the server runs
  migrations on startup. `[market] default_currency` is optional and unset by default.
- Background tasks live in `tasks/` (expiry, health, directory registration, bundle cleanup; two
  of the four are spawned conditionally).
- The REPL starts when stdin is a terminal (type `help`).
- The service worker caches assets and API responses; the app is a PWA with standalone display.

## Tests

```bash
cargo test --workspace     # komun-core + komun-server unit tests
cargo clippy --release -- -D warnings   # must stay at zero warnings (touch a source file first — a silent second run is a cache hit, not a clean lint)
cd web && npm run check && npm run build && npx vitest run
```

Measured 2026-09-25 in the agent sandbox (`rustc 1.95.0`, `node v22.23.2`), on the commit that
dropped the plaintext column: `cargo test --workspace` → **158 passed, 0 failed, 0 ignored**
(20 in `komun-core`, 138 in `komun-server`; three zero-test suites); `cargo clippy --release -- -D
warnings` → exit 0, no lints (the only line cargo prints is a future-incompat note about the
`sqlx-postgres` dependency); `npm run check` → **0 errors, 0 warnings**; `npm run build` → green;
`npx vitest run` → **82 tests in 7 files, all passing**. The frontend gates need
`crates/wasm/pkg/` to exist first.

The enum↔CHECK agreement test reads `migrations/001_schema.sql` at test time.

## Security model (short)

Threat model: network observer, compromised client state, XSS via user content, disk access to
the server. Out of scope: device compromise, supply-chain attacks, quantum adversaries.
Server-side crypto is Argon2id verifier hashing (a second, independent Argon2id over the verifier
before storage), SHA-256 session-token hashing, a per-process salt pepper, and TLS terminated at a
proxy.

**Honest limitation:** browser-delivered E2E cannot protect against a malicious server serving
modified JavaScript. It protects against database theft, passive disk reads, an operator reading
message content, and admin snooping — not against a hostile operator who ships modified client
code.

More detail: `docs/ARCHITECTURE.md`, `docs/CRYPTO.md`, `docs/DATABASE.md`,
`docs/DEVELOPMENT.md`, `docs/CONVENTIONS.md`, `docs/DEPLOY.md`.
