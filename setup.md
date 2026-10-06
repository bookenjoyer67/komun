# setup.md — Sandboxed agent for the Komun ("rev") Target Codebase

Agentic Engineer · Module 1 · Assignment 1.1, Exercise 2
Target Codebase: `~/komun` (Komun — marketplace and local listings hub, AGPL-3.0)
Sandbox image: `agent-sandbox:komun` (built from the `Dockerfile` in this repo root)

---

## 1. What the Target Codebase is

Komun is a Rust + SvelteKit + PostgreSQL platform: an Axum HTTP API, a SvelteKit 5 SPA,
a WASM crate holding the client-side crypto, and a WebSocket map relay.

| Item | Value (from the repo, not assumed) |
|:--|:--|
| Language | Rust (edition 2021, workspace `resolver = "2"`) + TypeScript/Svelte 5 |
| Packages | Cargo workspace — `komun-core`, `komun-server`, `komun-wasm`, `komun-relay`; npm for `web/` |
| Runtime | Rust 1.95.0, Node 22 LTS, PostgreSQL 16 |
| Test framework | `cargo test` (unit tests in `crates/core/src/tests.rs`, `crates/server/src/tests/mod.rs`, `crates/relay/src/**`) and Vitest 4 (`web/src/tests/*.test.ts`, jsdom) |
| Build commands | `wasm-pack build crates/wasm --target web` → `cd web && npm install && npm run build` → `cargo build --release --bin komun-server` |
| Local services | PostgreSQL `:5432` (`docker compose up db -d`), API `:3000`, piggPin WebSocket relay `:9001` (disabled by default in `config.example.toml`) |
| Setup files read | `README.md`, `AGENTS.md`, `Cargo.toml`, `web/package.json`, `web/vitest.config.ts`, `docker/Dockerfile`, `docker-compose.yml`, `config.example.toml`, `.env.example`, `.gitignore`, `.dockerignore`, `migrations/`, `scripts/` |

**Credentials referenced by the repo**

* `.env.example` → `DB_USER`, `DB_PASSWORD`, `DB_NAME`, `JWT_SECRET`.
* `config.example.toml` → `[database].url`, `[auth].jwt_secret`.

None of those are needed to build or test. `config.toml` and `.env` are gitignored and
**not present on this host**, so there was nothing to mount — the container is given the
example files only, and any real values would be throwaway test values. The coding agent's
own API credentials are **not in the container at all** — see §4.

**Smallest safe mount**

The repo root: `/home/computing/komun` → `/workspace`. That is one project directory —
not `$HOME`, not `~/.ssh`, not `~/.config`, not `~/Documents`.

**Does the agent need network access?**

With the final design (§4) the agent container has **no network egress at all**: it sits on an
`--internal` docker network and can reach exactly one host — the credential broker — which is
the only thing that can reach the providers. Crate/npm fetches therefore have to come from the
pre-warmed Cargo volumes, or from temporarily attaching the bridge network (§6, Q6).

---

## 2. Prerequisites (Docker)

The assignment assumes Docker Desktop. This host runs the native Docker Engine instead;
the check is the same one the assignment asks for:

```
$ docker --version
Docker version 29.3.0, build 5927d80

$ docker info --format 'server={{.ServerVersion}} driver={{.Driver}} root={{.DockerRootDir}}'
server=29.8.1 driver=overlayfs root=/var/lib/docker
```

If `docker info` fails with `failed to connect to the docker API at unix:///var/run/docker.sock`,
the daemon is not up. On this host that is:

```bash
sudo systemctl start docker
```

---

## 3. Build the image

The Dockerfile is the course starter from
`LaunchCodeEducation/LaunchCodeAgenticEngineer` → `module_1/Dockerfile`, copied into this
repo's root and adapted (the course copy was left untouched). What changed and why:

| Course Dockerfile | This Dockerfile | Why |
|:--|:--|:--|
| `FROM python:3.12-slim` | `FROM rust:1.95-slim-bookworm` | Komun is Rust; the base must carry cargo/rustc. Tag matches the host toolchain (1.95.0). |
| `pip install -r requirements.txt` (streamlit, anthropic, pandas…) | dropped | Komun contains no Python. Installing a Python/Streamlit stack "just in case" is exactly what the lesson says not to do. |
| apt: curl/git/bash/ca-certificates/nano/procps | kept | Needed by the agent and by cargo. |
| — | `+ pkg-config libssl-dev xz-utils postgresql-client` | `openssl-sys` link deps for the axum/jsonwebtoken chain; `xz-utils` to unpack the Node tarball (the first build failed without it); `psql`/`pg_isready` so the agent can inspect the dev DB. |
| — | `+ rustup component add clippy rustfmt` | The repo's quality gates (`cargo clippy --release -- -D warnings`, `cargo fmt --check`). |
| — | `+ Node 22.23.2 (official tarball)` | `web/` needs SvelteKit 5 / Vite 6 / Vitest 4; the distro Node is far too old. |
| — | `+ wasm-pack 0.15.0` | `web/package.json` depends on `komun-wasm` (`file:../crates/wasm/pkg`), so the frontend cannot build until the WASM crate is packed. |
| `npm i -g @anthropic-ai/claude-code` | kept | The coding agent. |
| `npm i -g opencode-ai` | kept | Second agent (used for the smoke test). |
| ngrok | dropped | Komun serves a static SPA; nothing needs a public tunnel. |
| `COPY settings.json` / `statusline.sh` / `docker-entrypoint.sh` | kept, unmodified | Course scaffolding: Claude Code status line and the credential-persistence entrypoint (now idle — the sandbox holds no Claude login). |

```bash
cd ~/komun
docker build -t agent-sandbox:komun .
```

Real output (tail; full log at `~/.hermes/profiles/dev/cache/scratch/build.log`):

```
Step 18/20 : ENV CARGO_TARGET_DIR=/workspace/target
Step 19/20 : ENTRYPOINT ["docker-entrypoint.sh"]
Step 20/20 : CMD ["/bin/bash"]
Successfully built fc9329cd83aa
Successfully tagged agent-sandbox:komun

$ docker images agent-sandbox:komun --format '{{.Repository}}:{{.Tag}} {{.ID}} {{.Size}}'
agent-sandbox:komun fc9329cd83aa 3.27GB
```

Toolchain actually present in the image (read back from inside the container):

```
rustc 1.95.0 (59807616e 2026-04-14)      node v22.23.2
cargo 1.95.0 (f2d3ce0bd 2026-03-21)      npm 10.9.8
wasm-pack 0.15.0                         psql (PostgreSQL) 15.19
2.1.280 (Claude Code)                    opencode 1.18.32          git 2.39.5
```

Files added for the sandbox:

```
Dockerfile                      the sandbox image (above)
docker-entrypoint.sh  settings.json  statusline.sh    course scaffolding, verbatim
sandbox/run-agent.sh            launcher: broker + agent, one command
sandbox/stage-secrets.sh        stages the broker's keys on the host (~/.config/komun-sandbox)
sandbox/opencode-sandbox.json   agent-side opencode config — points at the broker, holds no key
sandbox/broker/broker.py        the credential broker (stdlib Python, ~250 lines)
sandbox/broker/Dockerfile       broker image
```

---

## 4. Run the container

One command starts both halves — broker and agent:

```bash
~/komun/sandbox/run-agent.sh
```

What it does:

1. `docker network create --internal agent-net` — an internal network with **no route off the host**.
2. builds/starts `komun-sandbox-broker:local` as `rev-broker`, on `agent-net` **and** the default
   bridge. The bridge gives the broker (and only the broker) an internet route.
3. starts `rev-agent` on `agent-net` **only**, with a dummy token and no credential mounts.
4. writes the Claude Code trust/onboarding file for `/workspace` (contains no credential).

The equivalent by hand:

```bash
docker network create --internal agent-net
docker build -t komun-sandbox-broker:local ~/komun/sandbox/broker      # once

docker run -d --name rev-broker --network agent-net --user 1000:1000 \
  -v ~/.claude:/secrets/claude \
  -v ~/.config/komun-sandbox:/state \
  -e CLAUDE_CRED=/secrets/claude/.credentials.json \
  -e OPENAI_KEY_FILE=/state/deepseek.key \
  -e BACKUP_DIR=/state/backups \
  komun-sandbox-broker:local
docker network connect bridge rev-broker

docker run -dit --name rev-agent --network agent-net \
  -e ANTHROPIC_BASE_URL=http://rev-broker:4000 \
  -e ANTHROPIC_AUTH_TOKEN=sandbox-dummy-token \
  -e CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
  -v /home/computing/komun:/workspace \
  -v komun-cargo-target:/workspace/target \
  -v komun-cargo-registry:/usr/local/cargo/registry \
  -v /home/computing/komun/sandbox/opencode-sandbox.json:/root/.config/opencode/opencode.json:ro \
  agent-sandbox:komun
```

### The credential broker

A mounted key is a key the agent can read and leak; `:ro` only stops it being *changed*. So the
final design puts **no credential in the agent container** and gives it **no route to leak one
over**. The agent talks to `http://rev-broker:4000` with the literal string
`sandbox-dummy-token`; the broker swaps in the real credential upstream.

| | agent container (`rev-agent`) | broker container (`rev-broker`) |
|:--|:--|:--|
| Networks | `agent-net` (internal — no egress) | `agent-net` + `bridge` |
| Credentials | none — dummy token in env only | `~/.claude` (rw) and `~/.config/komun-sandbox` (rw) |
| Mounts | `/workspace`, 2 Cargo volumes, opencode config | 2 secret mounts, no project files |
| Runs as | root | uid 1000 (so written-back credentials stay yours) |
| Forwards to | nothing | `api.anthropic.com`, `api.deepseek.com` only |

Broker routes: `/v1/messages*` → Anthropic (Claude Code); `/v1/chat/completions`, `/v1/models`,
`/v1/embeddings` → the OpenAI-compatible upstream; anything else is refused with 404. `/health`
reports status and never any secret material. `POST /refresh` is a maintenance endpoint that
forces the OAuth refresh now, so the write-back path can be proven before a token actually
expires.

Claude Code's OAuth access token expires hourly, so the broker refreshes it and **writes the
refreshed credential back to the host file** (atomic replace, timestamped backup kept in
`~/.config/komun-sandbox/backups/`). That matters: refresh tokens rotate, so two independent
copies of the credential would invalidate each other — one file, one lineage, host `claude` and
sandbox `claude` both keep working. The refresh uses the same values Claude Code itself uses,
read out of the installed binary rather than guessed: token URL
`https://platform.claude.com/v1/oauth/token`, public client id
`9d1c250a-e61b-44d9-88ed-5944d1962f5e`, body `grant_type=refresh_token`, plus the
`oauth-2025-04-20` beta header on API calls.

Use it with:

```bash
docker exec -it rev-agent claude --model sonnet                              # Claude Code
docker exec -it rev-agent opencode run -m sandbox/deepseek-v4-flash "..."    # opencode
docker exec -it rev-agent bash                                               # plain shell
docker logs -f rev-broker                                                    # one line per request
```

### Model access inside the sandbox

Claude Code learns which models the account may use from `~/.claude.json`
(`modelAccessCache` + `additionalModelOptionsCache`). With no egress the container cannot fetch
that list, and a fresh container resolves aliases (`--model opus`) to a default the account is
**not** entitled to — `claude-opus-5-5`, which returns 404 and surfaces as
"There's an issue with the selected model". `run-agent.sh` therefore copies just those cache
fields (plus trust for `/workspace`) out of the host's `~/.claude.json` into the container's —
no tokens. After that, aliases and explicit ids both work:

```
$ for m in opus sonnet haiku claude-fable-5-1; do claude --model "$m" -p 'ok'; done
opus                 ok
sonnet               ok
haiku                ok
claude-fable-5-1     ok
```

On this account only `claude-opus-5-5` is unentitled; the 13 others (opus-5, opus-4-5…4-8,
sonnet-4-5/4-6/5, haiku-4-5, fable-5/5-1, opus-3) are fine.

opencode is the other half of the story: it lists its bundled `opencode/*` free models plus
`sandbox/deepseek-v4-flash`, but only the broker-routed one can actually reach a provider — the
rest would need egress the sandbox does not have. More upstreams can be added by giving the
broker an OpenAI-compatible URL and key (`OPENAI_UPSTREAM` / `OPENAI_KEY_FILE`) and adding a
provider block to `sandbox/opencode-sandbox.json`.

### Verify the boundaries

Network — the agent can reach the broker and nothing else:

```
$ docker exec rev-agent curl -s http://rev-broker:4000/health
{"ok": true, "anthropic_mode": "oauth", "openai_upstream": "https://api.deepseek.com", "openai_key_present": true}

$ docker exec rev-agent curl -s -m 8 -o /dev/null -w '%{http_code}\n' https://api.anthropic.com/v1/messages
000
$ docker exec rev-agent curl -s -m 8 -o /dev/null -w '%{http_code}\n' https://crates.io
000
$ docker exec rev-agent getent hosts api.anthropic.com
(no output — DNS does not resolve outside the internal network)
```

Filesystem:

```
$ docker inspect rev-agent --format '{{range .Mounts}}{{.Type}} {{.Source}} -> {{.Destination}} rw={{.RW}}{{println}}{{end}}nets={{range $k,$v := .NetworkSettings.Networks}}{{$k}} {{end}}'
bind   /home/computing/komun -> /workspace rw=true
volume /var/lib/docker/volumes/komun-cargo-target/_data -> /workspace/target rw=true
bind   /home/computing/komun/sandbox/opencode-sandbox.json -> /root/.config/opencode/opencode.json rw=false
volume /var/lib/docker/volumes/komun-cargo-registry/_data -> /usr/local/cargo/registry rw=true
nets=agent-net
```

Secret material — there is none to find, while the broker has the real files:

```
$ docker exec rev-agent bash -c 'find / -xdev \( -name "*.credentials.json" -o -name "deepseek.key" -o -name "auth.json" \) 2>/dev/null'
(no output)
$ docker exec rev-agent bash -c 'env | grep -Ei "anthropic|deepseek|token"'
ANTHROPIC_BASE_URL=http://rev-broker:4000
ANTHROPIC_AUTH_TOKEN=<dummy>
$ docker exec rev-broker python3 -c "import os;print(os.path.exists('/secrets/claude/.credentials.json'), os.path.exists('/state/deepseek.key'))"
True True
```

The only file in the agent container that mentions the OAuth schema is
`/workspace/sandbox/broker/broker.py`, which names the JSON *field* and holds no value.

---

## 5. Smoke test

**Prompt** (inside the container, agent = opencode, model `sandbox/deepseek-v4-flash`):

```
docker exec -w /workspace rev-agent bash -c 'export HOME=/root;
  opencode run -m sandbox/deepseek-v4-flash "Inspect the repository at /workspace and write a
  concise Markdown summary of its structure, stack (languages, package managers), build/test
  commands, and local services to /workspace/agent-summary.md. Do not modify any other file.
  When finished print: WROTE /workspace/agent-summary.md"'
```

**Output:**

```
← Write agent-summary.md
Wrote file successfully.

WROTE /workspace/agent-summary.md
```

**Did it persist on the host?**

```
$ ls -l ~/komun/agent-summary.md
-rw-r--r-- 1 computing computing 4753 Sep 22 19:06 /home/computing/komun/agent-summary.md
```

Yes — the agent wrote inside `/workspace` (backed by the host bind mount) and the file is on the
host. It correctly describes the four crates, the Rust/Node/Postgres stack, the
WASM-before-frontend build order and the local services, including details that only come from
reading the files (there is no npm `test` script; Vitest is run with `npx vitest run`).

**Did it write anywhere else on the host?**

```
$ docker diff rev-agent | grep -v '^C /workspace'
A /claude-auth
C /usr/local/cargo ; A /usr/local/cargo/registry
C /tmp ; A /tmp/…so
C /root/.config/opencode ; A … opencode.json, opencode.jsonc
C /root/.local/share/opencode ; A … opencode.db, opencode.db-shm, opencode.db-wal, log/
C /root/.local/state/opencode ; A … locks/
C /root/.cache …
```

Every write outside `/workspace` is container-local state — the agent's own database, logs and
lock files, plus `/tmp` scratch. No host path outside `~/komun` received a byte.

**Both agents verified end to end, holding no credential:**

```
$ docker exec -w /workspace rev-agent bash -c 'claude --model sonnet -p "Reply with exactly: BROKER_OK"'
BROKER_OK
$ docker exec -w /workspace rev-agent bash -c 'opencode run -m sandbox/deepseek-v4-flash "Reply with exactly: BROKER_OPENCODE_OK"'
BROKER_OPENCODE_OK

$ docker logs rev-broker
[00:05:18] broker up on :4000 — anthropic=oauth openai_upstream=https://api.deepseek.com
[00:05:26] 200 POST /v1/messages?beta=true (3.1s)
[00:05:35] 200 POST /v1/chat/completions (0.5s)
```

**Credential refresh verified (the piece most likely to break silently):**

```
$ docker exec rev-agent curl -s -X POST http://rev-broker:4000/refresh
{"ok": true, "refreshed": true}

before: expiresAt 2026-09-23T01:19:31+00:00
after:  expiresAt 2026-09-23T08:07:23+00:00        # broker rotated + wrote back to the host file

$ docker logs rev-broker
[00:07:21] access token expired/near expiry — refreshing
[00:07:23] credential refreshed and written back to the host file
[00:07:35] 200 POST /v1/messages?beta=true (3.8s)

$ claude --model sonnet -p "Reply with exactly: HOST_OK"           # host, after the sandbox refreshed
HOST_OK
$ docker exec -w /workspace rev-agent bash -c 'claude --model sonnet -p "Reply with exactly: SANDBOX_AFTER_REFRESH"'
SANDBOX_AFTER_REFRESH
```

So the single-lineage design holds: the sandbox refreshes the OAuth credential, the host's
`claude` keeps working with the rotated token, and the pre-refresh copy is kept at
`~/.config/komun-sandbox/backups/credentials-<timestamp>.json` (mode 0600).

**Toolchain proof (run directly in the sandbox, not by the agent):**

```
$ docker exec -w /workspace rev-agent bash -c 'export PATH=/usr/local/cargo/bin:$PATH; cargo check --workspace'
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 21.76s
warning: `komun-server` (bin "komun-server") generated 4 warnings
```

The whole workspace — `komun-core`, `komun-server`, `komun-wasm`, `komun-relay`, including sqlx,
axum, tokio and the image codecs — compiles inside the sandbox.

**One real finding.** The repo's documented test command does **not** currently compile:

```
$ cargo test --workspace
error[E0063]: missing field `image_path` in initializer of `community::Community`
   --> crates/core/src/tests.rs:135:25
error: could not compile `komun-core` (lib test) due to 1 previous error
```

This is a pre-existing defect in the working tree, not a sandbox artefact — the identical error
reproduces on the host (`cargo test -p komun-core --no-run`). `crates/core/src/tests.rs` is stale
relative to the `Community` model, which has grown an `image_path` field. Until that file is
updated, `cargo check --workspace` is the working quality gate in this sandbox.

**Build-incident notes.** The first image build failed at the Node step with
`tar (child): xz: Cannot exec: No such file or directory`; `xz-utils` was added and the build
completed. The first broker attempt failed with 400/401 from both upstreams: HTTP header names
are case-insensitive but a Python dict is not, so copying the client's `Authorization` and then
setting `authorization` sent **two** auth headers and the upstream saw the dummy token.
Normalising every header name to lowercase fixed it.

---

## 6. Security decisions

**Q1 — Why did you mount only this folder?**
The project is the only thing the agent needs. `/home/computing/komun` is mounted at `/workspace`
and nothing else host-side is: no `$HOME`, no `~/.ssh`, no `~/.config`, no `~/.hermes` (agent
memory, cron, credentials), no `~/.aws`, no browser profile — and, in the final design, no API
credentials either. Docker enforces this rather than the agent's good behaviour: `docker inspect`
shows the complete mount list, the container is on a network with no route out, and I verified
inside it that the host paths are absent and that no credential file exists anywhere in it. The
repo's `.dockerignore` independently keeps `.env`, `config.toml`, `target/` and `node_modules/`
out of the build context, so no secret is baked into a layer either.

**Q2 — What did you choose to keep ephemeral?**
Everything the container writes to its own filesystem: `/tmp` scratch, the agent's session
database, logs and lock files under `/root/.local/(share|state)/opencode`, and shell history. It
all dies with the container, which keeps "run it again from a clean state" cheap and guarantees
agent runtime state never becomes a stray file in the repo.

**Q3 — What did you choose to persist?**
Three things, deliberately. (a) Source-code changes: `/workspace` is a bind mount onto the real
repo, so the file the agent wrote inside the container (`agent-summary.md`, 4753 bytes) is on the
host. (b) The Cargo build cache and crate registry, in the named volumes `komun-cargo-target` and
`komun-cargo-registry` — they survive container exits so sessions don't re-download and
re-compile the dependency graph, and keeping them in volumes means root-owned build artefacts
never pollute the host's `target/`. (c) The provider credentials, in the broker container's two
mounts and nowhere else: one copy of the Claude credential, shared with host `claude` so OAuth
refresh stays one lineage, and one DeepSeek key staged at `~/.config/komun-sandbox/deepseek.key`
(mode 600, outside the repo).

**Q4 — What dependencies did you include in your extended Docker image?**
Only what this project needs, installed at build time rather than container start: the
`rust:1.95-slim` base (cargo, rustc) plus `clippy` and `rustfmt` for the quality gates,
`pkg-config` and `libssl-dev` because `openssl-sys` must link, `postgresql-client` so the agent
can talk to the dev database without hosting a server, Node.js 22.23.2 from the official tarball
for the SvelteKit frontend and Vitest, and `wasm-pack` 0.15.0 for `crates/wasm`, which the
frontend depends on. Plus the course scaffolding — the Claude Code and opencode CLIs,
`settings.json`, the status line and the entrypoint. Deliberately **excluded**: Python and the
Streamlit/Anthropic/pandas stack from the course base, ngrok, any observability agent, the Docker
CLI/socket, and any project secret. The broker is a **separate minimal image** —
`python:3.12-slim` plus stdlib-only code, running as uid 1000 — and it never sees the project.

**Q5 — What did your smoke test prove?**
It proved the loop works end to end with no broad access and no credentials: the agent ran inside
the container, read the mounted repo, wrote one new file into `/workspace`, and that file
persisted to the host (4753 bytes). It proved containment: `docker diff` shows the only writes
outside `/workspace` were container-local agent state and `/tmp`. It proved the credential
boundary: both `claude` and `opencode` completed real model calls (BROKER_OK, BROKER_OPENCODE_OK)
while the container held only `sandbox-dummy-token` and could not resolve or connect to
`api.anthropic.com`, `crates.io`, or anywhere else. And `cargo check --workspace` proved the
toolchain compiles the whole workspace inside the sandbox — which also surfaced a genuine
pre-existing compile error in the repo's test code rather than a sandbox problem.

**Q6 — What risks remain?**
* The broker is **trusted code holding the real keys**. It reads `~/.claude` (which contains
  session history, not just the credential) and can write there. It is small, stdlib-only and
  unreachable from the agent, but it is now the piece that must stay correct — a bug there, or a
  compromise of the host, still exposes the credentials.
* The agent can still **use the broker as an oracle**: it can spend account tokens on any request
  the broker forwards. Paths are restricted to the two provider APIs, but there is no model
  allowlist, spend cap, or request-size limit yet.
* **No egress means no new dependencies.** `cargo add` / `npm install` cannot fetch anything
  inside the agent container; the Cargo caches are pre-warmed, but a genuinely new crate fails
  until the bridge is temporarily attached (`docker network connect bridge rev-agent`).
* The container runs as **root**, so files the agent creates in `/workspace` are root-owned on the
  host (`agent-summary.md` was; I `chown`ed it back). Running as `--user 1000:1000` with a
  container-local `HOME` is the clean fix, and a prerequisite for mounting the workspace
  read-only — the next hardening step for pure-review tasks.
* **The workspace is mounted read-write**, so the agent can rewrite `Cargo.lock` or `migrations/`
  (the repo's rule is that migrations are append-only). Git worktrees — one per session, as the
  1.2 exercise does — are the intended containment.
* Claude Code still receives a real, refreshed credential **over the wire** from the broker on
  every call; the dummy token protects the container, not the docker-bridge hop to the broker.
* The image pins tool versions (Rust 1.95.0, Node 22.23.2, wasm-pack 0.15.0) that will drift from
  the host.

All of the above are provisional first-pass choices for this project and are expected to change
as the course progresses.

---

## 7. Version history of these choices

* **v1 (first pass).** The agent's credentials were mounted into the agent container: a read-only
  bind of `~/.local/share/opencode/auth.json` plus a `claude-auth` named volume for the Claude
  Code login. It worked, and `:ro` prevented modification — but the agent process could still
  *read* the key, so a prompt-injected agent could exfiltrate a long-lived credential.
* **v2 (current).** No credential in the agent container at all: an `--internal` network, a broker
  sidecar holding the keys and refreshing the OAuth credential in place, and a dummy token in the
  agent's environment. Verified in §4 and §5.
* **Next.** Model allowlist and request caps in the broker; run the agent as a non-root uid;
  read-only workspace mounts for review-only tasks; one Git worktree per parallel session (1.2).
