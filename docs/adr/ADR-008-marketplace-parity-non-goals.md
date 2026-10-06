# ADR-008: Refuse five marketplace mechanisms, and name the artifact that enforces each

Which marketplace mechanisms does Komun refuse to copy, and what keeps each refusal in force?

## Status

What is this decision's current status?

**Accepted** for Wave 0 of the marketplace-parity plan of record (storage entry `468ce9dd` `WAVE 0 — guardrails and the refusal tests`). The rating threshold is five reviews (storage entry `468ce9dd` `D4 — Rating threshold: withhold rating_avg until rating_count >= N`). The human approved D4 at checkpoint 1 with N = 5 [UNVERIFIED]. One refusal has a built code guard today, one is policy-only, and three name planned guards.

Claims needing verification:

- The checkpoint-1 approval of D4 with N = 5. The run summary of `run-2026-10-06-parity-marketplace` settles it, because no repository file records the approval.

## Context

What prompted the refusals, and what does the code do today?

A research pass read two large classified-ad platforms against Komun (storage entry `fc8894a7` `Research synthesis: Facebook Marketplace and Craigslist, read against Komun.`). It proposed seven features and named five mechanisms to refuse (storage entry `fc8894a7` `DELIBERATE NON-GOALS — the mechanisms to refuse:`). The parity plan makes the refusals enforceable in its Wave 0 (storage entry `468ce9dd` `WAVE 0 — guardrails and the refusal tests`).

The feed orders posts by urgency before anything else, and no engagement signal enters it (`crates/server/src/db/posts.rs:96` `CASE WHEN p.urgency = 'critical' THEN 0`). Money changes hands in person (`README.md:28` `Money changes hands in person; there are no payment rails.`). A moderator hides a post through one handler, which takes the post id and no reason (`crates/server/src/api/reports.rs:90` `Path(post_id): Path<Uuid>,`).

## Decision

What does Komun refuse, and which artifact enforces each refusal?

Refuse the five mechanisms below. Each one carries its reason in one line and names its enforcing artifact as built, planned or policy-only.

### R-1: No engagement-optimised ranking

Why is engagement ranking refused, and what enforces the refusal?

Reason: an engagement-ranked surface buries the requests a mutual-aid hub exists to answer (storage entry `fc8894a7` `Burying a request is the failure mode to avoid.`). Enforcement is planned, not built. A source-scanning test refuses any migration that declares an engagement-counter column (storage entry `468ce9dd` `It fails if any migrations/*.sql file declares an engagement-counter column`). Wave 3 adds a sort allow-list test (storage entry `468ce9dd` `sort_accepts_only_recency_and_distance`).

### R-2: No public rating before a threshold

Why is an early public rating refused, and what enforces the refusal?

Reason: a mean of a few reviews is an anecdote (storage entry `fc8894a7` `An unearned rating is lasting social harm at neighbourhood scale`). Enforcement is built. The threshold is one named constant (`crates/server/src/api/users.rs:15` `pub(crate) const RATING_PUBLISH_THRESHOLD: i64 = 5;`). The profile response publishes the mean only through the gate (`crates/server/src/api/users.rs:55` `"rating_avg": publishable_rating(row.rating_avg, row.rating_count),`). The review count stays public (`crates/server/src/api/users.rs:56` `"rating_count": row.rating_count,`). Two boundary tests pin the gate (`crates/server/src/api/users.rs:68` `fn publishable_rating_is_withheld_below_the_threshold() {`; `crates/server/src/api/users.rs:76` `fn publishable_rating_is_published_at_the_threshold() {`). A web test of the withheld state is planned (storage entry `468ce9dd` `web/src/tests/userProfileRating.test.ts`).

### R-3: No shadowbanning

Why is silent hiding refused, and what enforces the refusal?

Reason: an author whose post vanishes without notice stops trusting the hub (storage entry `fc8894a7` `If a post is hidden, tell the author and say why.`). Enforcement is planned for Wave 5, and nothing enforces it today. That wave records every hide and notifies the author with the reason (storage entry `468ce9dd` `This is the executable no-shadowbanning guard.`).

### R-4: No commercial-vendor drift

Why is commercial use refused, and what enforces the refusal?

Reason: a mutual-aid hub serves neighbours, not resellers (storage entry `fc8894a7` `no resale-for-profit, no commercial vendors.`). Enforcement is policy-only: this ADR is the artifact, and the reviewer applies it (storage entry `468ce9dd` `No mechanical check exists or is built; the reviewer enforces it against ADR-008.`).

### R-5: No payments or escrow

Why are payments refused, and what enforces the refusal?

Reason: a gift economy has no fee to take and no money to hold (storage entry `fc8894a7` `A gift economy needs neither.`). The repository holds no payment rail today (`AGENTS.md:39` `There are no payment rails`). Enforcement is planned: the same migration-scanning test refuses a payment, escrow or fee table (storage entry `468ce9dd` `or a payment, escrow or fee table`).

## Alternatives considered

Which alternatives were weighed, and why was each rejected?

- Publish the mean from the first review, as before. Rejected: it publishes an anecdote as a reputation (storage entry `fc8894a7` `An unearned rating is lasting social harm at neighbourhood scale`).
- Hide `rating_count` together with the mean. Rejected: the count shows a user has been reviewed without publishing a thin mean (`crates/server/src/api/users.rs:54` `` `rating_count` stays public``).
- Withhold the mean in the profile query. Rejected: the plan keeps the computation and gates only publication (storage entry `468ce9dd` `the average is still computed; only publication is gated`).
- Guard the `sort` parameter in Wave 0. Rejected: the field arrives in Wave 3 (storage entry `468ce9dd` `the sort allow-list moves to Wave 3`).

## Consequences

What does the decision change, and what does it leave open?

Most existing profiles lose their visible mean at once, which D4 intends (storage entry `468ce9dd` `most existing profiles lose their visible star at once (intended by D4`). The profile page interpolates the mean into its rating label (`web/src/routes/users/[id]/+page.svelte:118` `` ? `Rated ${profile.rating_avg} out of 5 from ${profile.rating_count} reviews` ``). That page needs its planned withheld-state branch before the server change ships alone.

The conformance gate's file list leaves out `docs/adr/`, so the reviewer reads this ADR directly (storage entry `468ce9dd` `docs/adr/ is NOT listed`).

## Evidence

Which artifacts settle this decision?

- Read the research synthesis at storage entry `fc8894a7-fcbf-4e01-97d5-5b987b36a580`.
- Read the plan of record at storage entry `468ce9dd-2329-4986-b8f7-a5bce532f959`, section G, `THE FIVE REFUSALS — deliverables and enforcing artifacts`.
- Read the built R-2 guard at `crates/server/src/api/users.rs:15` `RATING_PUBLISH_THRESHOLD` and its tests at `crates/server/src/api/users.rs:63` `#[cfg(test)]`.
