#!/usr/bin/env bash
# scripts/run-agent-servers.sh — start the MCP servers for one role box in its sidecar container:
#   ./scripts/run-agent-servers.sh <role>
#   ./scripts/run-agent-servers.sh --print-mounts <role> [--unbound]
#   ./scripts/run-agent-servers.sh orchestrator --unbound
#
# The sidecar mounts .memory read-write and no overlays because it hosts the store's only writer; no
# agent runs in it. A role box mounts .memory read-only, so the servers never run inside one.
#
# Every sidecar binds the role it serves with AGENT_ROLE="$ROLE" unless it is started with --unbound.
# Unbound, any caller on the network may claim a granted role, run_fix included, and gate rows read
# "bound": false. The orchestrator's sidecar is the single exception that may run
# unbound, because its in-process subagents call the servers as several roles and a bound server
# refuses all but one. Every other role refuses --unbound, and an unbound sidecar refuses BETA_BASE_URL.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "${REPO:-$SCRIPT_DIR/..}" && pwd)"
cfg() { local v=""; [ -f "$SCRIPT_DIR/agentic_config.py" ] && v="$(python3 "$SCRIPT_DIR/agentic_config.py" --get "$1" --default "$2" 2>/dev/null || true)"; printf '%s' "${v:-$2}"; }
IMAGE="${IMAGE:-$(cfg containers.tools_image 'agent-sandbox:komun-m3')}"
TARGET_VOL="${TARGET_VOL:-rev-cargo-target}"
REGISTRY_VOL="${REGISTRY_VOL:-$(cfg containers.registry_volume 'komun-cargo-registry')}"
NET_INTERNAL="${NET_INTERNAL:-$(cfg containers.networks.internal 'agent-internal')}"
WORKSPACE="$(cfg containers.workspace '/workspace')"
MEMORY_DIR="$(cfg containers.memory_dir '/workspace/.memory')"
VALID_ROLES="orchestrator planner implementer tester reviewer project-manager researcher beta-tester"

usage() {
  printf 'usage: ./scripts/run-agent-servers.sh <role> [--unbound]\n'
  printf '       ./scripts/run-agent-servers.sh --print-mounts <role> [--unbound]\n\n'
  printf '  <role>          %s\n' "$VALID_ROLES"
  printf '  --print-mounts  print the -v arguments and the AGENT_ROLE a start passes, one per line;\n'
  printf '                  no docker call\n'
  printf '  --unbound       start the servers with AGENT_ROLE empty. Accepted for the orchestrator only,\n'
  printf '                  whose in-process subagents call the servers as several roles.\n'
  printf '                  EXPOSURE: an unbound gate server lets any caller on the network claim\n'
  printf '                  any granted role, and so reach run_fix, which writes this tree. Its\n'
  printf '                  rows read "bound": false (mcp/gate/server.py:13-14).\n'
  printf '                  Without this flag the sidecar binds AGENT_ROLE to <role>.\n\n'
  printf 'environment: NAME (container, default agent-rev-m4-<role>-servers), BOX_NAME (the role box,\n'
  printf '             default agent-rev-m4-<role>), BETA_BASE_URL (starts the browser; refused with --unbound).\n'
}

valid_role() { local r; for r in $VALID_ROLES; do [ "$r" = "$1" ] && return 0; done; return 1; }

PRINT=no; UNBOUND=no; ROLE=""
while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --print-mounts) PRINT=yes ;;
    --unbound) UNBOUND=yes ;;
    -*) printf 'error: unknown flag "%s"\n\n' "$1" >&2; usage >&2; exit 2 ;;
    *)
      if [ -n "$ROLE" ]; then
        printf 'error: one role per sidecar, got "%s" and "%s"\n\n' "$ROLE" "$1" >&2; usage >&2; exit 2
      fi
      ROLE="$1"
      ;;
  esac
  shift
done

if ! valid_role "$ROLE"; then
  printf 'error: unknown role "%s"\n\n' "$ROLE" >&2
  usage >&2
  exit 2
fi
if [ "$UNBOUND" = yes ] && [ "$ROLE" != orchestrator ]; then
  printf 'error: --unbound is accepted for the orchestrator sidecar only; a sidecar that serves one role binds it\n' >&2
  exit 2
fi
BOUND_ROLE="$ROLE"; [ "$UNBOUND" = no ] || BOUND_ROLE=""
[ "$UNBOUND" = no ] || [ -z "${BETA_BASE_URL:-}" ] || { printf 'error: --unbound refuses BETA_BASE_URL: no orchestrated role holds a browser grant, and an unbound browser server serves any caller that declares one\n' >&2; exit 2; }

# The gate server runs the gates here, so the build cache is writable in the sidecar.
declare -a MOUNTS=(
  -v "$REPO:$WORKSPACE"
  -v "$REPO/.memory:$MEMORY_DIR"
  -v "$TARGET_VOL:$WORKSPACE/target"
  -v "$REGISTRY_VOL:/usr/local/cargo/registry"
)
declare -a ENVS=(-e "AGENT_ROLE=$BOUND_ROLE")
[ -z "${BETA_BASE_URL:-}" ] || ENVS+=(-e "BETA_BASE_URL=$BETA_BASE_URL")

if [ "$PRINT" = yes ]; then
  for ((i = 1; i < ${#MOUNTS[@]}; i += 2)); do printf -- '-v %s\n' "${MOUNTS[$i]}"; done
  printf -- '-e AGENT_ROLE=%s\n' "$BOUND_ROLE"
  exit 0
fi

NAME="${NAME:-agent-rev-m4-$ROLE-servers}"
BOX_NAME="${BOX_NAME:-agent-rev-m4-$ROLE}"
ALIAS="$ROLE-servers"

command -v docker >/dev/null || { printf 'docker is not on PATH\n' >&2; exit 1; }
docker info >/dev/null 2>&1 || { printf 'docker daemon is not running (sudo systemctl start docker)\n' >&2; exit 1; }
[ -d "$REPO/.memory" ] || { printf 'no memory layer at %s/.memory\n' "$REPO" >&2; exit 1; }
docker network inspect "$NET_INTERNAL" >/dev/null 2>&1 || docker network create --internal "$NET_INTERNAL" >/dev/null

# Two containers answering one alias share its DNS name, so a role box could reach either one,
# bound or not.
for other in $(docker ps -q --filter "network=$NET_INTERNAL"); do
  other_name="$(docker inspect -f '{{.Name}}' "$other")"
  other_name="${other_name#/}"
  [ "$other_name" != "$NAME" ] || continue
  aliases="$(docker inspect -f '{{range .NetworkSettings.Networks}}{{range .Aliases}}{{println .}}{{end}}{{end}}' "$other")"
  case $'\n'"$other_name"$'\n'"$aliases"$'\n' in
    *$'\n'"$ALIAS"$'\n'*)
      printf 'error: container %s already answers as %s on %s; stop it first: docker rm -f %s\n' \
        "$other_name" "$ALIAS" "$NET_INTERNAL" "$other_name" >&2
      exit 1
      ;;
  esac
done

# Reused only when running with .memory and the workspace read-write and the same AGENT_ROLE, so a
# hand-built sidecar with no AGENT_ROLE, or with another one, is replaced rather than kept.
RECREATE=yes
if docker inspect "$NAME" >/dev/null 2>&1; then
  running_now="$(docker inspect -f '{{.State.Running}}' "$NAME")"
  ws_rw="$(docker inspect -f "{{range .Mounts}}{{if eq .Destination \"$WORKSPACE\"}}{{.RW}}{{end}}{{end}}" "$NAME")"
  mem_rw="$(docker inspect -f "{{range .Mounts}}{{if eq .Destination \"$MEMORY_DIR\"}}{{.RW}}{{end}}{{end}}" "$NAME")"
  env_lines="$(docker inspect -f '{{range .Config.Env}}{{println .}}{{end}}' "$NAME")"
  have_role_set=no; have_role=""
  while IFS= read -r line; do
    case "$line" in AGENT_ROLE=*) have_role_set=yes; have_role="${line#AGENT_ROLE=}" ;; esac
  done <<< "$env_lines"
  if [ "$running_now" = true ] && [ "$ws_rw" = true ] && [ "$mem_rw" = true ] && \
     [ "$have_role_set" = yes ] && [ "$have_role" = "$BOUND_ROLE" ]; then
    printf 'container %s is already running for %s — reusing it\n' "$NAME" "$ROLE"
    RECREATE=no
  else
    docker rm -f "$NAME" >/dev/null 2>&1 || true
  fi
fi

if [ "$RECREATE" = yes ]; then
  docker run -d --name "$NAME" \
    --network "$NET_INTERNAL" --network-alias "$ALIAS" \
    "${ENVS[@]}" \
    "${MOUNTS[@]}" \
    -w "$WORKSPACE" \
    --entrypoint bash \
    "$IMAGE" \
    -c 'python3 "$1/mcp/gate/server.py" --port 8003 --host 0.0.0.0 & exec bash "$1/scripts/start-mcp-servers.sh"' \
    sidecar "$WORKSPACE" >/dev/null
fi

sleep 2
if [ "$(docker inspect -f '{{.State.Running}}' "$NAME")" != true ]; then
  printf 'error: sidecar %s exited; its output follows\n' "$NAME" >&2
  docker logs "$NAME" >&2 || true
  exit 1
fi

eff_role="$(docker inspect -f '{{range .Config.Env}}{{println .}}{{end}}' "$NAME" | sed -n 's/^AGENT_ROLE=//p')"
eff_mem="$(docker inspect -f "{{range .Mounts}}{{if eq .Destination \"$MEMORY_DIR\"}}{{if .RW}}read-write{{else}}read-only{{end}}{{end}}{{end}}" "$NAME")"

printf 'sidecar   : %s (alias %s on %s, no egress)\n' "$NAME" "$ALIAS" "$NET_INTERNAL"
printf 'serves    : %s\n' "$BOX_NAME"
if [ -n "$eff_role" ]; then
  printf 'identity  : AGENT_ROLE=%s (bound)\n' "$eff_role"
else
  printf 'identity  : AGENT_ROLE empty (UNBOUND: any caller on %s may claim a granted role, run_fix included; rows read "bound": false)\n' "$NET_INTERNAL"
fi
printf 'memory    : %s (%s)\n' "$MEMORY_DIR" "$eff_mem"
printf 'servers   : storage 8001, retrieval 8002, gate 8003%s\n' "${BETA_BASE_URL:+, browser 8004}"
printf '\n'
printf 'Wire the role box once it is running. run-agent.sh reseeds /root/.claude.json whenever it\n'
printf 'recreates the box, so repeat these after a recreate:\n'
for pair in storage:8001 retrieval:8002 gate:8003 ${BETA_BASE_URL:+browser:8004}; do
  printf '  docker exec %s claude mcp add --scope local --transport http %s http://%s:%s/mcp\n' \
    "$BOX_NAME" "${pair%%:*}" "$ALIAS" "${pair#*:}"
done
printf '\nStop it: docker rm -f %s\n' "$NAME"
