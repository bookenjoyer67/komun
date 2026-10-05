# Coding Standards

Last reviewed: 2026-10-04
Maintained by: the Komun maintainers

These standards apply to all code in this project. The agent should consult this file before writing
or reviewing any code. These rules are set by humans and the agent should never modify this file.

## 1. Zero clippy warnings
`cargo clippy --release -- -D warnings` must exit clean on every change. Touch a source file before
re-running it: a silent second run is a cache hit, not a clean lint.

## 2. Tests stay green
`cargo test --workspace` must pass; the baseline measured 2026-09-25 was 158 passed, 0 failed, 0
ignored, and the frontend suite was 82 tests in 7 files. A change that breaks either gate is not done.

## 3. Migrations are append-only
`migrations/001_schema.sql` is checksum-bookmarked in every provisioned database and is never edited.
Schema changes arrive as new files: `002_...`, `003_...`.

## 4. The frontend is Svelte 5 runes only
Use `$state`, `$derived`, `$effect` and `$props`. There is no `export let`, no `on:click` and no
`$:` in this codebase, and event handlers are written `onclick={handler}`.

## 5. Never log or store secret material
No key material, key bundle, password, derived key, recovery code or message plaintext is written to a
log, a memory file or a commit. The server holds public keys and wrapped bundles only, and messages
carry `ciphertext` plus `nonce` with no plaintext column.

## 6. Rebuild the wasm package and the frontend together
After any change under `crates/wasm/`, rebuild `wasm-pack` output and the frontend, because
`web/package.json` consumes `crates/wasm/pkg` as a file dependency. The build order is
`wasm-pack build crates/wasm --target web`, then `npm install && npm run build` in `web/`, then
`cargo build --release --bin komun-server`.

## 7. Documentation claims carry their authority
Every claim in a document names the `path:line` that contains the literal text it quotes, and every
count names the search that produced it. A claim that cannot be traced is marked rather than asserted.

## 8. Never commit local state or build output
`config.toml`, `.env`, `.env.local`, `crates/wasm/pkg/`, `web/build/`, `data/avatars/` and
`data/post-images/` stay out of commits. They hold credentials or reproducible artifacts.

## 9. Comment only what the code cannot say
A comment earns its place only by stating an invariant, a constraint, a failure mode, or why the
obvious alternative is wrong. Anything that restates the code, narrates the change, or cites a line
number that will move is deleted. When in doubt, leave it out.
