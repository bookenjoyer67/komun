# Porting the Komun agentic quality gate

How does a fork move this gate onto another codebase?

## The system

What does a fork inherit?

Seven governed roles run behind one launcher (`agentic.config.json:70` `"valid": ["orchestrator", "planner", "implementer", "tester", "reviewer", "project-manager", "researcher"]`).
Four MCP servers enforce the boundaries (`agentic.config.json:93` `"storage_server": "mcp/storage/server.py"`, `agentic.config.json:95` `"gate_server": "mcp/gate/server.py"`, `ls mcp/coursetools_server.py` -> `mcp/coursetools_server.py`).
A deterministic gate core runs three commands from the config (`agentic.config.json:19` `"test": {`, `agentic.config.json:24` `"clippy": {`, `agentic.config.json:34` `"fmt": {`).
Five CI jobs carry the pipeline (`python3 -c "import yaml;print(list(yaml.safe_load(open('.github/workflows/ci.yml'))['jobs']))"` -> `['change-type-check', 'policy-gate', 'eval-gate', 'advisory-review', 'audit-trail']`).
A conversion loop replaces stable agent steps with scripts (`docs/step-classification.md:3` `Which agentic steps run in this repository, and which of them earn deterministic conversion?`).

## The fork checklist

Which files must a fork copy, and which config values must it change?

1. Copy the tree and keep `agentic.config.json` at its root, because every consumer resolves paths from there (`agentic.config.json:2` `"schema_version": 1`).
2. Replace the toolchain commands, because the three gates are Rust and cargo today (`agentic.config.json:20` `"argv": ["cargo", "test", "--workspace"]`).
3. Rewrite the clippy guard, because its marker names this repository's crate (`agentic.config.json:28` `"marker": "Checking komun-server"`, `agentic.config.json:30` `"touch_file": "crates/server/src/main.rs"`).
4. Rebuild both images under the fork's own tags (`agentic.config.json:43` `"base_image": "agent-sandbox:komun"`, `agentic.config.json:44` `"tools_image": "agent-sandbox:komun-m3"`).
5. Rename the cargo registry volume, because the tester mounts it read-write (`agentic.config.json:48` `"registry_volume": "komun-cargo-registry"`).
6. Rename both networks, because the launcher creates one and requires the other (`agentic.config.json:50` `"internal": "agent-internal"`, `agentic.config.json:51` `"broker": "agent-net"`).
7. Rename the credential broker and its port (`agentic.config.json:54` `"name": "rev-broker"`, `agentic.config.json:55` `"port": 4000`).
8. Replace the project key, because storage entries are scoped by it (`agentic.config.json:99` `"project_key": "proj-komun"`).
9. Point the style-rules path at the fork's own rule set (`agentic.config.json:90` `"style_rules": "docs/DOC-STYLE.md"`).
10. Rewrite the classification globs, because the classifier keys both CI flags off them (`agentic.config.json:102` `"governed_globs": [`, `agentic.config.json:122` `"policy_globs": [`).
11. Record the fork's values under `port.komun_defaults`, because fork mode compares against them (`agentic.config.json:130` `"komun_defaults": {`).
12. Run the self-test after the edits and read its verdict (`scripts/port-self-test.sh:5` `--fork     as above, and fail on every seam that still`).

Which values does `port.komun_defaults` record today?

| Group | `port.komun_defaults` key | Value today |
|---|---|---|
| Toolchain | `toolchain.commands.clippy.guard.marker` | `Checking komun-server` (`agentic.config.json:132` `"toolchain.commands.clippy.guard.marker": "Checking komun-server",`) |
| Toolchain | `toolchain.commands.clippy.guard.marker_regex` | the clippy marker regex (`agentic.config.json:133` `"toolchain.commands.clippy.guard.marker_regex": "\\bChecking\\b\\s+(?P<marker>komun-server)\\b",`) |
| Toolchain | `toolchain.commands.clippy.guard.touch_file` | `crates/server/src/main.rs` (`agentic.config.json:134` `"toolchain.commands.clippy.guard.touch_file": "crates/server/src/main.rs",`) |
| Images | `containers.base_image` | `agent-sandbox:komun` (`agentic.config.json:135` `"containers.base_image": "agent-sandbox:komun",`) |
| Images | `containers.tools_image` | `agent-sandbox:komun-m3` (`agentic.config.json:136` `"containers.tools_image": "agent-sandbox:komun-m3",`) |
| Caches | `containers.registry_volume` | `komun-cargo-registry` (`agentic.config.json:137` `"containers.registry_volume": "komun-cargo-registry",`) |
| Networks | `containers.networks.internal` | `agent-internal` (`agentic.config.json:138` `"containers.networks.internal": "agent-internal",`) |
| Networks | `containers.networks.broker` | `agent-net` (`agentic.config.json:139` `"containers.networks.broker": "agent-net",`) |
| Broker | `containers.broker.name` | `rev-broker` (`agentic.config.json:140` `"containers.broker.name": "rev-broker",`) |
| Project | `project.name` | `komun` (`agentic.config.json:131` `"project.name": "komun",`) |
| Project | `artifacts.project_key` | `proj-komun` (`agentic.config.json:141` `"artifacts.project_key": "proj-komun",`) |
| Docs | `artifacts.style_rules` | `docs/DOC-STYLE.md` (`agentic.config.json:142` `"artifacts.style_rules": "docs/DOC-STYLE.md"`) |

## The seam table

Which file owns each seam, and what breaks when a fork leaves it untouched?

| File | Config key | What the value means | What breaks if a fork leaves it untouched |
|---|---|---|---|
| `scripts/run-agent.sh` | `containers.tools_image` (`scripts/run-agent.sh:26` `IMAGE="${IMAGE:-$(cfg containers.tools_image 'agent-sandbox:komun-m3')}"`) | the image the launcher starts | the launcher starts a foreign image name |
| `scripts/run-agent.sh` | `containers.broker.name` (`scripts/run-agent.sh:27` `BROKER="${BROKER:-$(cfg containers.broker.name 'rev-broker')}"`) | the broker container the launcher probes | the launcher exits 1 with a missing-broker message |
| `scripts/run-agent.sh` | `containers.networks.broker` (`scripts/run-agent.sh:36` `NET_BROKER="${NET_BROKER:-$(cfg containers.networks.broker 'agent-net')}"`) | the network that reaches the broker | the launcher exits 1 with a missing-network message |
| `scripts/run-agent.sh` | `containers.networks.internal` (`scripts/run-agent.sh:35` `NET_INTERNAL="${NET_INTERNAL:-$(cfg containers.networks.internal 'agent-internal')}"`) | the egress-free MCP network | the container joins the wrong network |
| `scripts/run-agent.sh` | `containers.workspace` (`scripts/run-agent.sh:37` `WORKSPACE="$(cfg containers.workspace '/workspace')"`) | the container mount point | mounts and execs point at a directory that does not exist |
| `scripts/run-agent.sh` | `containers.memory_dir` (`scripts/run-agent.sh:38` `MEMORY_DIR="$(cfg containers.memory_dir '/workspace/.memory')"`) | the memory bind destination | the reuse check never matches and the container is recreated every run |
| `scripts/run-agent.sh` | `containers.broker.port` (`scripts/run-agent.sh:28` `BROKER_PORT="${BROKER_PORT:-$(cfg containers.broker.port '4000')}"`) | the broker port in the base URL | the harness talks to the wrong port |
| `scripts/run-agent.sh` | `containers.registry_volume` (`scripts/run-agent.sh:30` `REGISTRY_VOL="${REGISTRY_VOL:-$(cfg containers.registry_volume 'komun-cargo-registry')}"`) | the cargo registry volume | the read-write tester mount misses the warm registry |
| `scripts/run-agent.sh` | `roles.mounts` (`scripts/run-agent.sh:104` `var="${dim#*:}"; mode="$(cfg "roles.mounts.$1.${dim%%:*}" "${!var}")"; [ -z "$mode" ] || printf -v "$var" '%s' "$mode"`) | the per-role workspace, memory and cache modes | a role receives another role's permissions |
| `.github/workflows/ci.yml` | `containers.base_image` (`.github/workflows/ci.yml:116` `echo "AGENT_IMAGE_BASE=$(cfg containers.base_image 'agent-sandbox:komun')"`) | the base image tag the pipeline builds | the pipeline builds a stale base |
| `.github/workflows/ci.yml` | `containers.tools_image` (`.github/workflows/ci.yml:172` `echo "AGENT_IMAGE_TOOLS=$(cfg containers.tools_image 'agent-sandbox:komun-m3')"`) | the tools image tag every job runs | every gate runs inside the wrong image |
| `.github/workflows/ci.yml` | `containers.workspace` (`.github/workflows/ci.yml:173` `echo "AGENT_WORKSPACE=$(cfg containers.workspace '/workspace')"`) | the container workspace for CI steps | the gates run in a directory that does not exist |
| `.github/workflows/ci.yml` | `containers.cargo_registry_cache` (`.github/workflows/ci.yml:175` `echo "AGENT_CACHE_REGISTRY=${RUNNER_TEMP}/$(cfg containers.cargo_registry_cache 'cargo-registry')"`) | the runner-side registry cache directory name | the cache restores under the old name and every build is cold |
| `mcp/gate/server.py` | `toolchain.commands.*` (`bash scripts/port-self-test.sh` -> `ok    mcp/gate/server.py follows the config`) | the three gate argv lists and the guard | the gate runs another language's commands |
| `scripts/classify-change.py` | `classification.governed_globs` (`bash scripts/port-self-test.sh` -> `ok    scripts/classify-change.py follows the config`) | the paths that need the governed check | the classifier flags nothing and `eval-gate` never runs |
| `eval/test_policy.py` | `roles.valid` and `roles.mounts` (`bash scripts/port-self-test.sh` -> `ok    eval/test_policy.py follows the config`) | the roles the policy suite polices | the suite checks a foreign role list |
| `scripts/validate_doc_conformance_deterministic.py` | `artifacts.style_rules` (`bash scripts/port-self-test.sh` -> `ok    scripts/validate_doc_conformance_deterministic.py follows the config`) | the rule set the prose validator applies | the validator reports against another standard |
| `scripts/port-self-test.sh` | `port.komun_defaults` (`scripts/port-self-test.sh:103` `defaults = cfg["port"]["komun_defaults"]`) | the recorded Komun values fork mode hunts for | fork mode cannot separate a wired seam from an unwired one |

## The half-wired fork

Why does a fork that edits only the config still look green?

The red team still passes and the suites still pass while the gates silently test nothing (`scripts/port-self-test.sh:9` `The red team still passes, the suites still pass, and the gates silently`).
A consumer that ignores the config keeps its old value and never fails (`scripts/port-self-test.sh:12` `whose output does not move is not reading the config, whatever its comments claim.`).
Guard against that failure with the self-test (`scripts/port-self-test.sh:14` `Exit 0 when every check passes, 1 when any check fails, 2 on a usage error.`).
Read a non-zero exit as one named consumer still ignoring the config (`scripts/port-self-test.sh:156` `A consumer that ignores the config is the failure this kit exists to prevent: the suites would`).

- Run the wiring check on this repository, which prints one line per consumer (`scripts/port-self-test.sh:25` `pass() { printf '  ok    %s\n' "$1"; }`).
- Read a `FAIL` line as the named consumer still ignoring the config (`scripts/port-self-test.sh:26` `fail() { printf '  FAIL  %s\n' "$1"; FAIL=1; }`).
- Run the fork check to list every seam still carrying a Komun default (`scripts/port-self-test.sh:5` `--fork     as above, and fail on every seam that still`).

## What the config cannot carry

Which parts of this gate are content and code rather than values?

- Keep the retrieval corpus as data, because the retrieval server reads Markdown at startup (`mcp/retrieval/server.py:589` `reference corpus not found at {reference_dir}`).
- Keep the style-rules content as prose, because the config carries only its path (`docs/DOC-STYLE.md:26` `A section opens with the question it answers`).
- Replace the three gate commands for a non-Rust toolchain, because cargo argv is Rust-specific (`agentic.config.json:20` `"argv": ["cargo", "test", "--workspace"]`).
- Replace or delete the clippy guard, because a cached linter prints nothing and exits 0 (`agentic.config.json:31` `a cached clippy run prints nothing and exits 0, which is indistinguishable from a clean lint`).
- Delete the guard when your linter prints a cache hit, because a visible cache-hit line is already distinguishable from a clean lint (`agentic.config.json:31` `the touched file forces a real check`).

## The two path deviations

Which path names differ from the course material, and are they deliberate?

- Keep `mcp/` at the repository root instead of the course's `mcp-servers/` (`docs/ci-step-design.md:421` `The lesson names the course repository's paths and this repository carries the same classes at different paths`).
- Read the course-to-here mapping as a path translation rather than a new rule (`docs/ci-step-design.md:424` `| Lesson path | Path here | Flag |`).
- Keep `.memory/` for the memory layer where the course writes `logs/` [UNVERIFIED] (`ls -d .memory/knowledge .memory/reference` -> `.memory/knowledge  .memory/reference`).

Claims needing verification:

- The course material's `logs/` directory name has no artifact in this repository, so `ls` cannot settle it. Settle it by quoting the course lesson that names `logs/`.

## Running every check

Which command proves which layer still works?

- Run the policy suite, the 75-check permission regression (`python3 -m pytest eval/test_policy.py --collect-only -q` -> `75 tests collected in 0.07s`).
- Run the doc-conformance step test, the 15-check validator regression (`python3 -m pytest eval/test_deterministic_step.py --collect-only -q` -> `15 tests collected in 0.04s`).
- Run the gate server's own self-test against a live server (`mcp/gate/selftest.py:13` `python3 mcp/gate/selftest.py --url http://localhost:8003/mcp`).
- Run the launcher's matrix, one row per role (`bash scripts/run-agent.sh --matrix` -> seven rows).
- Run the launcher's config report, the keys it consumes (`bash scripts/run-agent.sh --print-config` -> `"containers.tools_image": "agent-sandbox:komun-m3"`).
- Override one value for a dev run, because an environment variable beats the config (`scripts/run-agent.sh:26` `IMAGE="${IMAGE:-$(cfg containers.tools_image 'agent-sandbox:komun-m3')}"`).
- Override the broker, its port, the registry volume and both networks the same way (`scripts/run-agent.sh:36` `NET_BROKER="${NET_BROKER:-$(cfg containers.networks.broker 'agent-net')}"`).