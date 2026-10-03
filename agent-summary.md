# Komun — Repository Summary

Marketplace and mutual aid hub: people post needs, offers and resources and match via encrypted conversations. Backend is Rust (Axum) + PostgreSQL; frontend is a SvelteKit 5 SPA; client-side crypto is compiled to WASM.

## Repository structure

```
/workspace
├── Cargo.toml               # Rust workspace (core, server, wasm, relay)
├── crates/
│   ├── core/                # Shared data models (Community, Member, Post, MatchThread, User)
│   ├── server/              # Axum HTTP API, JWT auth, DB queries, REPL, tasks
│   ├── wasm/                # Client crypto (ed25519, x25519, ChaCha20Poly1305, Argon2, BIP39)
│   └── relay/               # piggPin WebSocket map relay (feature-gated MQTT/RNode/Reticulum bridges)
├── web/                     # SvelteKit 5 SPA (static adapter), Vite 6, TypeScript
├── migrations/              # SQLx migrations (001–015, additive; never edit existing)
├── docker/Dockerfile        # App image (multi-stage Rust build → debian-slim runtime)
├── Dockerfile               # Sandbox/agent image (Rust + Node 22 + wasm-pack + coding agents)
├── deploy/                  # nginx conf, initd, setup.sh, seed.sql
├── docs/                    # ARCHITECTURE, CONVENTIONS, CRYPTO, DATABASE, DEVELOPMENT
├── scripts/                 # audit.sh, sync-server-repo.sh
├── config.example.toml      # Documented runtime config template (copy to config.toml)
├── .env.example             # DB + JWT env template
└── docker-compose.yml       # db (postgres:16-alpine) + app services
```

The top-level `Dockerfile`, `settings.json`, `statusline.sh`, `docker-entrypoint.sh`, and `sandbox/` are the course/agent harness, not the Komun application.

## Stack

- **Rust** (edition 2021, stable): Axum 0.8, Tokio, SQLx 0.8 (runtime queries), JWT HS256 (`jsonwebtoken`), ed25519-dalek, wasm-bindgen. Cargo workspace with 4 crates.
- **Frontend**: Node.js 22+, npm, Svelte 5 (runes only), SvelteKit 2, Vite 6, TypeScript, Vitest (jsdom + Testing Library).
- **Client crypto**: Rust → WASM via `wasm-pack`; consumed as local npm dependency `komun-wasm` (`file:../crates/wasm/pkg`).
- **Database**: PostgreSQL 16, migrations auto-run on server startup.
- **Package managers**: Cargo (workspace root + `crates/*/Cargo.toml`), npm (`web/package.json`, `Cargo.lock` and `package-lock.json` committed).

## Build commands (order matters)

```bash
# 1. WASM crypto library (must precede frontend)
wasm-pack build crates/wasm --target web        # -> crates/wasm/pkg/

# 2. Frontend static export
cd web && npm install && npm run build && cd .. # -> web/build/

# 3. Backend
cargo build --release --bin komun-server
cargo run --bin komun-server                    # serves API + web/build on :3000
```

Config setup: `cp config.example.toml config.toml` (edit `jwt_secret`) and optionally `cp .env.example .env`.

## Test / lint / check commands

```bash
cargo test --workspace          # Rust tests (core + server; DB integration tests need PostgreSQL)
cargo check                     # type check all crates
cargo clippy                    # lint
cargo fmt --check               # formatting

cd web
npm run check                   # svelte-kit sync + svelte-check
npm run dev                     # Vite dev server (HMR, default :5173)
npx vitest                      # frontend unit tests (src/**/*.test.ts)
```

Test files: `crates/core/src/tests.rs`, `crates/server/src/tests/mod.rs`, `web/src/tests/{AidCard,auth,crypto}.test.ts`.
No ESLint/Prettier/Biome, no Tailwind.

## Local services

| Service | How to start | Address |
|---|---|---|
| PostgreSQL 16 | `docker compose up db -d` (user/pass/db: `komun`) or local PG | `localhost:5432` |
| App (API + SPA + relay) | `cargo run --bin komun-server` | `http://localhost:3000` |
| piggPin relay (WebSocket) | Built into server when `[relay] enabled = true` | `:9001` |
| Vite dev server | `cd web && npm run dev` | `http://localhost:5173` |
| Full stack (Docker) | `docker compose up --build` | `http://localhost:3000` |

Key environment variables: `DATABASE_URL`, `JWT_SECRET` (required in prod), `BIND_ADDR`, `KOMUN_CONFIG`, plus `KOMUN_*` overrides for server/node config.

## Notes

- `config.toml`, `.env`, `crates/wasm/pkg/`, and `web/build/` are gitignored — never commit them.
- UUIDv7 primary keys; JWT carries `user_id` + `role`, so auth needs no DB lookup.
- Secret keys and passphrases never leave the client; server stores only public keys, encrypted bundles, and Argon2 hashes.
- Docker app build needs no `sqlx prepare` — all queries use runtime `sqlx::query()` / `query_as()`, not the compile-time macros.
