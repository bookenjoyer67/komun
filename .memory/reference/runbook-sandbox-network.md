---
classification: internal
project: proj-komun
doc_type: runbook
---

# Runbook: the internal sandbox network and the credential broker

Where does the agent container sit on the network, and where does its credential live?

The container has no network egress at all and can reach exactly one host, the credential broker (`setup.md:41` `the agent container has **no network egress at all**`). It runs on an internal docker bridge created once per host (`sandbox/README-m3.md:5` `docker network create --internal agent-internal`). The measured shape of that network on this host is an internal bridge (`sandbox/README-m3.md:43` `agent-internal bridge internal=true`).

## How do I prove loopback works and egress does not?

Run both probes inside the container, and expect the first to succeed and the second to fail at name resolution (`sandbox/README-m3.md:96` `loopback http=200` and `Could not resolve host: example.com`).

## Where does the credential live, and why not in the container?

No credential sits in the agent container, and the agent authenticates with a literal placeholder instead (`setup.md:178` `the broker swaps in the real credential upstream.`). The broker holds the secret mounts and forwards only to the two allowed upstreams (`setup.md:186` `api.anthropic.com`, `api.deepseek.com` only).

## What may an operator not do to make a denial disappear?

Never widen an allow-list to make a denial pass, because the denial is the evidence (`sandbox/README-m3.md:132` `Never widen the allow-list to make a denial pass: the denial is the evidence`). Publish no port from the internal network expecting host access: on this host the mapping is silently absent (`sandbox/README-m3.md:74` `produced no mapping at all`). Attaching the default bridge restores publishing, but it also restores egress, which the exercise forbids (`sandbox/README-m3.md:76` `it also restores egress`).
