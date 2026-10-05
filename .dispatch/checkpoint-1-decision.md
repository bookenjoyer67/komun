# Human checkpoint 1 — decision

**Plan: approved as written.**

**Finding 1: (B).** Rewrite `MatchStatus::is_resolved` as an exhaustive `match` that names all four
statuses, so a new status fails to compile again instead of silently taking the `false` arm. The
`resolved_at` computation keeps calling the predicate.

Note for the implementer: clippy's `match_like_matches_macro` does **not** fire on this shape — it
requires a catch-all arm — so no `#[allow]` is needed to keep `-D warnings` green. Verified on a
forced recheck of `komun-core`.

**Finding 2:** deferred. `decline_offer` setting `resolved_at` directly in SQL is correct today and
belongs in a separate change.

Proceed through the implementer, tester and reviewer, run the gates, and stop at Human checkpoint 2.
