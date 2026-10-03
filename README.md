<div align="center">

# 🫱🏾‍🫲🏼 Komun

**A marketplace and mutual aid hub — needs meet resources, conversations stay encrypted.**

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://rust-lang.org)
[![SvelteKit](https://img.shields.io/badge/SvelteKit-5-ff3e00.svg)](https://svelte.dev)

</div>

---

Komun is a single-server marketplace and mutual aid hub. People post what they need, what they can give,
and what they have to offer, then match and negotiate over end-to-end encrypted conversations.
The server is the community — there is no multi-tenant layer and no federation.

## Features

- **Email + password accounts.** Verified at signup, reset by emailed link. No key ceremony and
  no passphrase — users never see key material.
- **Encrypted conversations.** Message plaintext is encrypted in the browser; the database
  stores ciphertext only. See the honest limitation below.
- **Flat posts.** One server-wide feed at `/api/posts`, with search, categories and TTLs.
- **Marketplace.** Post a `listing` or a `want` with a price and condition, negotiate on the same
  encrypted thread (`offer` → `counter` → `accept`), complete the deal, and leave a star review.
  Money changes hands in person; there are no payment rails.
  - **Reviews follow completed deals.** A review is writable only against a `completed` deal, once
    per participant per deal, and a profile shows the average of a user's ratings to one decimal.
  - **Categories are data, not an enum.** The taxonomy is the seeded, runtime-editable `categories`
    table served by `GET /api/categories?scope=…`, so an admin can add, rename, reorder or retire
    one without a release.
  - **Paginated lists.** `GET /api/posts` (and a user's reviews) take `limit` — default **100**,
    capped at **200** — with `offset`, rejecting an out-of-range value with a 400 rather than
    silently clamping it.
- **OSM map.** Leaflet with click-to-place coordinates on new posts; `/map` plots located posts.
- **Optional public directory.** Advertise your server, and accept peer registrations, only when
  you opt in.

## Architecture

```
Browser (SvelteKit 5 SPA + komun-wasm)  ──HTTPS──▶  Axum server :3000  ──▶  PostgreSQL 16
        x25519 · XChaCha20Poly1305 · Argon2
```

| Crate | Role |
|---|---|
| `crates/core` | Shared models + the `db_enum!` macro |
| `crates/server` | Axum API, sessions, sqlx queries, tasks, REPL |
| `crates/wasm` | Client crypto: x25519, XChaCha20Poly1305, Argon2, recovery codes |

See `docs/ARCHITECTURE.md`, `docs/CRYPTO.md`, `docs/DATABASE.md`.

## Quickstart

Prerequisites: Rust (stable), Node 22 + npm 10, PostgreSQL 16, and `wasm-pack`.

```bash
git clone <your-fork> komun && cd komun

# 1. Database (see docs/DEVELOPMENT.md for the exact provisioning order — 001_schema.sql
#    is applied by hand once, then the migrator takes over)
createdb komun

# 2. Config
cp config.example.toml config.toml     # point [database] url at your Postgres

# 3. Build (wasm FIRST — the frontend depends on crates/wasm/pkg)
wasm-pack build crates/wasm --target web
cd web && npm install && npm run build && cd ..

# 4. Run
cargo run --bin komun-server
```

Open `http://localhost:3000`.

### Docker Compose

```bash
cp config.example.toml config.toml
docker compose up --build
```

## Development

```bash
cargo build --workspace --all-targets
cargo test --workspace
cargo clippy --release -- -D warnings

cd web && npm run check && npm run build && npx vitest run
```

`docs/DEVELOPMENT.md` has the provisioning order, the migration rules, a runtime-gate recipe,
and what a sandbox cannot verify (tiles, live SMTP, the wasm build).

The agent harness has its own guide. `docs/AGENT-HARNESS.md` covers the sandbox image, the internal
network, the MCP servers and the per-role launcher, with a prerequisite check and the failure modes
worth expecting. Nothing in it is needed to build or run the application.

## Security model

- **Never leaves the client:** the x25519 secret key, the password-derived key, the recovery code, and
  message plaintext.
- **The server stores:** the email, the Argon2id password verifier, wrapped key bundles, hashed
  session tokens, and ciphertext.
- **Sessions** are opaque 256-bit tokens stored only as a hash; the role is loaded from the
  database on every request, so revocations and demotions take effect immediately.
- **No JWT and no ed25519** anywhere.

### Honest limitation

Browser-delivered end-to-end encryption **cannot** protect against a malicious server that
serves modified JavaScript. This design protects against database theft, passive disk reads, an
operator reading message content, and admin snooping. It does not protect against a hostile
operator who ships modified client code.

## Deploying

See `docs/DEPLOY.md` for a self-hosting walkthrough (release binary, service, database, TLS in
front). `deploy/` holds starting-point assets.

## License

**AGPL-3.0-or-later** — [LICENSE](LICENSE)

<div align="center">
<br>
<b>Solidarity, not charity.</b>
</div>
