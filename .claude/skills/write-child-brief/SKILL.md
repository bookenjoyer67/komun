---
name: write-child-brief
description: >
  Produce the brief a role receives before it is spawned, so the child starts from measured context
  instead of rediscovering it. Use when the orchestrator is about to spawn a role and the work rests
  on facts another session already established.
---

# Skill: Write Child Brief

## Why does a brief decide the run?

A role reads its brief, its own definition and the artifacts it may open, and nothing else about the
change. A role that cannot reach the artifact its brief describes plans from the brief alone, which is
the starvation near-miss recorded as NM-2.

## What must the brief carry?

- One sentence naming the change request and the project key.
- The repository path and the container name the role runs in.
- A measured-context block: each fact the child must not rediscover, labelled measured, with the file or
  command that produced it.
- The work, split by role, one block per role.
- The evidence the child must return, named as an artifact rather than as an assurance.

## What must the brief never carry?

- A restatement of a fact the brief has not itself checked.
- An instruction that needs a tool the child does not hold.
- A credential, a token or a provider key.
- A conclusion the child is expected to reach before it looks.

## How is a self-reported result checked?

Name the watermark that identifies the child's own rows. A journal stamps the caller name from
unvalidated input, so the brief names the newest pre-existing row before the first call, and a
per-invocation figure the child can match against the response it received.

## Activation Scope

Permitted for `orchestrator` alone, because the orchestrator is the role that spawns and briefs the others.
Denied for `planner`, `implementer`, `tester`, `reviewer`, `project-manager` and `researcher`, because a briefed role never writes its own brief.
