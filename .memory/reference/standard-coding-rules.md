---
classification: internal
project: proj-komun
doc_type: standard
---

# Standard: the eight coding rules

Which rules must every change in this repository satisfy?

The eight rules live in one human-maintained file the agent consults and never edits. Each rule below cites that file by line (`AGENTS.md:149` `Keep the prose docs in sync with the code`).

- Hold zero clippy warnings on the release profile, and touch a source file before re-measuring, because a silent second run is a cache hit (`.memory/knowledge/coding-standards.md:10` `must exit clean on every change. Touch a source file before`).
- Keep both test suites green; the recorded baseline is 158 passed, 0 failed, 0 ignored, plus 82 frontend tests in 7 files (`.memory/knowledge/coding-standards.md:13` `## 2. Tests stay green`).
- Treat migrations as append-only, because `001_schema.sql` is checksum-bookmarked in every provisioned database (`.memory/knowledge/coding-standards.md:17` `## 3. Migrations are append-only`).
- Write the frontend in Svelte 5 runes only, with no `export let`, no `on:click` and no `$:` (`.memory/knowledge/coding-standards.md:21` `## 4. The frontend is Svelte 5 runes only`).
- Never log or store secret material: no key, bundle, password, derived key, recovery code or plaintext message (`.memory/knowledge/coding-standards.md:25` `## 5. Never log or store secret material`).
- Rebuild the wasm package and the frontend together after any change under `crates/wasm/` (`.memory/knowledge/coding-standards.md:30` `## 6. Rebuild the wasm package and the frontend together`).
- Give every documentation claim its authority, and mark an untraceable claim instead of asserting it (`.memory/knowledge/coding-standards.md:36` `## 7. Documentation claims carry their authority`).
- Keep local state and build output out of commits, because those paths hold credentials or reproducible artifacts (`.memory/knowledge/coding-standards.md:40` `## 8. Never commit local state or build output`).
