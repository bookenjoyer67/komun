#!/usr/bin/env bash
# Module 2.4, drill 2 (scope leak): start a one-shot agent container whose /workspace/.memory is
# ANOTHER project's memory, replicating the lesson's "ran docker from the wrong directory" mistake.
#
#   ./run-badmount.sh "$HOME/project-b/.memory"   # the mistake
#   ./run-badmount.sh "$HOME/rev/.memory"         # the corrected rerun
#
# The workspace bind mount is always ~/rev; only the memory mount changes. The container reuses the
# broker network and the credential-free setup from sandbox/run-agent.sh: no keys, internal network,
# a dummy token, and the trust/model cache copied from the long-lived agent container so a headless
# run does not stall on onboarding.
set -euo pipefail

MOUNT="${1:?usage: run-badmount.sh <memory-dir-to-mount>}"
NAME="${NAME:-agent-rev-badmount}"
SOURCE_CTR="${SOURCE_CTR:-agent-rev}"

[ -d "$MOUNT" ] || { echo "no such memory directory: $MOUNT"; exit 1; }

docker rm -f "$NAME" >/dev/null 2>&1 || true

docker run -dit --name "$NAME" \
  --network agent-net \
  -e ANTHROPIC_BASE_URL=http://rev-broker:4000 \
  -e ANTHROPIC_AUTH_TOKEN=sandbox-dummy-token \
  -e CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
  -v "$HOME/rev:/workspace" \
  -v "$MOUNT:/workspace/.memory" \
  -v rev-cargo-target:/workspace/target \
  -v komun-cargo-registry:/usr/local/cargo/registry \
  agent-sandbox:komun >/dev/null

# trust + model entitlement cache, copied from the long-lived agent container (no tokens in it)
docker exec "$SOURCE_CTR" cat /root/.claude.json > /tmp/badmount-claude.json 2>/dev/null || echo '{}' > /tmp/badmount-claude.json
docker cp /tmp/badmount-claude.json "$NAME:/root/.claude.json" >/dev/null
rm -f /tmp/badmount-claude.json
docker exec "$SOURCE_CTR" cat /root/.claude/settings.json > /tmp/badmount-settings.json 2>/dev/null || true
[ -s /tmp/badmount-settings.json ] && docker cp /tmp/badmount-settings.json "$NAME:/root/.claude/settings.json" >/dev/null
rm -f /tmp/badmount-settings.json

echo "container : $NAME"
echo "workspace : $HOME/rev -> /workspace (Komun)"
echo "memory    : $MOUNT -> /workspace/.memory"
docker inspect "$NAME" --format '{{range .Mounts}}  mount {{.Type}} {{.Source}} -> {{.Destination}}{{println}}{{end}}'

# ---
# Provenance: copied verbatim from ~/komun-agent-exercise-2-4/run-badmount.sh on 2026-10-07.
# Nothing above this note was changed; the original path is outside the repository, so this
# copy exists to keep the citation at docs/iteration-log.md:542 reachable.
