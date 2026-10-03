# Agent harness

How does a fresh clone stand up the development environment for the agentic quality gate?

This file is the entry point. It puts the existing commands in order and says what each one proves. The
depth stays in `setup.md`, `sandbox/README-m3.md` and `scripts/README.md`, which this file links rather
than repeats.

## What does the environment consist of?

Which parts have to be standing before a role can run?

- An image carrying the toolchain and the agent CLIs (`sandbox/Dockerfile.m3:11-12` `docker build -f sandbox/Dockerfile.m3 -t agent-sandbox:komun-m3 .`).
- One internal docker network with no route off the host (`sandbox/run-agent-m3.sh:31` `docker network inspect agent-internal >/dev/null 2>&1 || { docker network create --internal agent-internal; }`).
- A credential broker holding the provider keys, so no agent container holds one (`sandbox/run-agent.sh:91` `-e ANTHROPIC_AUTH_TOKEN=sandbox-dummy-token \`).
- Four MCP servers: gate, storage, retrieval and coursetools (`agentic.config.json:200` `"storage_server": "mcp/storage/server.py",`).
- Seven role definitions under `.claude/agents/`, launched one container per role (`scripts/run-agent.sh:33` `VALID_ROLES="orchestrator planner implementer tester reviewer project-manager researcher"`).

## What must be installed first?

Which commands prove the host is ready?

```bash
docker --version
docker info --format '{{.ServerVersion}}'   # the daemon must answer, not just the CLI
cargo --version
python3 --version
node --version
wasm-pack --version
```

- Treat `docker info` as the real check. A present CLI with an absent daemon fails every later step.
- Expect the host to drift from the versions the image pins (`setup.md:18` `Rust 1.95.0, Node 22 LTS, PostgreSQL 16`).
- Measured on this host on 2026-10-03: cargo 1.95.0, Python 3.14.7, Node v26.7.0, wasm-pack 0.14.0, Docker Server 29.8.2.
- Read the drift as expected, not as a fault. The container carries the pinned Node 22 and `wasm-pack` 0.15.0 (`setup.md:439` `wasm-pack` 0.15.0 for `crates/wasm`).

## How is it built?

Which commands create the image and the network?

Run both from the repository root, because the Dockerfiles copy repo-relative paths
(`sandbox/Dockerfile.m3:47` `COPY sandbox/requirements-m3.txt /tmp/requirements-m3.txt`).

```bash
docker build -t agent-sandbox:komun .                              # Module 1 image
docker build -f sandbox/Dockerfile.m3 -t agent-sandbox:komun-m3 .  # Module 3 image
docker network create --internal agent-internal                    # once; it persists
```

## How is it verified before anything relies on it?

Which checks settle that isolation is real?

```bash
docker network inspect agent-internal --format '{{.Name}} {{.Driver}} internal={{.Internal}}'
# expected: agent-internal bridge internal=true
```

```bash
# inside the container: loopback must work, egress must not
python3 -m http.server 8000 &
curl http://localhost:8000     # must succeed, printing a directory listing
curl https://example.com       # must fail, "Could not resolve host" or a timeout
kill %1
```

- Stop the test server before the MCP work; storage and retrieval take 8001 and 8002 (`sandbox/README-m3.md:97` `python3 mcp/storage/server.py --port 8001 --host 0.0.0.0 &`).
- Start the MCP servers from inside the container (`sandbox/README-m3.md:111` `bash /workspace/scripts/start-mcp-servers.sh`).

## How is a role launched?

Which commands start one governed role, and how is it inspected?

```bash
./scripts/run-agent.sh --matrix        # the permission matrix, no daemon needed
./scripts/run-agent.sh reviewer bash   # one container, read-only workspace
```

- Name the role and the command from the two lists the launcher validates (`scripts/run-agent.sh:34` `VALID_CMDS="bash sh claude opencode"`).
- Read the matrix before the first real run (`scripts/run-agent.sh:115` `--matrix)  print_matrix; exit 0 ;;`).
- Prove a role's boundary with the launcher's own five recorded proofs (`scripts/README.md` sections a to f).

## What goes wrong?

Which failures are expected, and what does each mean?

- The daemon does not answer. `docker info` reports a missing socket at `unix:///var/run/docker.sock`, and every later command fails the same way. Start Docker Engine and re-run the check.
- A new dependency cannot be fetched. `cargo add` and `npm install` reach nothing inside the agent container, because the network is internal (`setup.md:465` `cannot fetch anything`). Use the pre-warmed Cargo volumes, or attach the bridge network for that build only.
- A published port never arrives. On an internal network `-p` produced no host mapping at all (`sandbox/README-m3.md:74` `"8000/tcp":null`). Reach the servers from inside the container instead.
- A file the agent wrote is root-owned on the host. The container runs as root, so `/workspace` writes land as root (`setup.md:468` `The container runs as **root**`). Change ownership back before the next host-side command.
- `fmt` is red. That is pre-existing at `HEAD` and unattributed to any change (`docs/calibration-log.md:170` `pre-existing at` `HEAD`).
- The database is empty or half-migrated. Follow the provisioning order rather than re-running a migration (`docs/DEVELOPMENT.md:52` `## Provisioning a database (the exact order, and why)`).

## Where is the rest written down?

Which document holds what?

| Question | Document |
|:--|:--|
| How was this environment designed, and what was rejected? | `setup.md` sections 4 to 6 |
| How is the sandbox image built and the network made? | `sandbox/README-m3.md` |
| Who may call what, and how is each grant proven? | `scripts/README.md` |
| How is the application itself built and provisioned? | `docs/DEVELOPMENT.md` |
| Who may call which MCP tool? | `docs/routing-and-tool-grant-map.md` |
| What does each role hold? | `docs/governance-policy.md` |
