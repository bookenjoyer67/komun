# Pre-merge gate: MatchStatus::is_resolved

A change is committed on branch `verify/matchstatus-is-resolved` (commit `88b9e0d`):

crates/core/src/models/match_thread.rs |  8 ++++++++
 crates/core/src/tests.rs               | 15 +++++++++++++++
 crates/server/src/db/conversations.rs  | 13 ++++++++-----
 3 files changed, 31 insertions(+), 5 deletions(-)
The diff, in full:

```diff
diff --git a/crates/core/src/models/match_thread.rs b/crates/core/src/models/match_thread.rs
index 54f4ec4..bd03246 100644
--- a/crates/core/src/models/match_thread.rs
+++ b/crates/core/src/models/match_thread.rs
@@ -11,6 +11,14 @@ db_enum!(
     }
 );
 
+impl MatchStatus {
+    /// The two terminal statuses. A thread that reached either one is over: `resolved_at` is set
+    /// for it, and no further offer may be appended to it.
+    pub fn is_resolved(&self) -> bool {
+        matches!(self, MatchStatus::Completed | MatchStatus::Withdrawn)
+    }
+}
+
 db_enum!(
     /// A step in a marketplace negotiation, carried on the same thread as the messages.
     OfferKind {
diff --git a/crates/core/src/tests.rs b/crates/core/src/tests.rs
index 1cc1e3b..c90461c 100644
--- a/crates/core/src/tests.rs
+++ b/crates/core/src/tests.rs
@@ -351,6 +351,21 @@ mod schema_contract {
         }
     }
 
+    /// The set, not the predicate's own body: a fifth status lands on one side of this line by
+    /// being named here, and a status that quietly starts resolving threads fails this test.
+    #[test]
+    fn only_the_terminal_statuses_are_resolved() {
+        let resolved: Vec<MatchStatus> = MatchStatus::ALL
+            .iter()
+            .copied()
+            .filter(|status| status.is_resolved())
+            .collect();
+        assert_eq!(
+            resolved,
+            vec![MatchStatus::Completed, MatchStatus::Withdrawn]
+        );
+    }
+
     #[test]
     fn admin_roles_are_admin() {
         assert!(Role::Admin.is_admin());
diff --git a/crates/server/src/db/conversations.rs b/crates/server/src/db/conversations.rs
index c72df63..6861faa 100644
--- a/crates/server/src/db/conversations.rs
+++ b/crates/server/src/db/conversations.rs
@@ -604,8 +604,10 @@ async fn insert_offer(
 pub fn check_offer_allowed(current: MatchStatus) -> Result<(), String> {
     match current {
         MatchStatus::Proposed | MatchStatus::Accepted => Ok(()),
-        // Names the status for the same reason `check_transition` does: it is the one fact the
-        // caller does not have.
+        // Listed as the live set rather than written as `!current.is_resolved()`: a fifth status
+        // is then refused until someone decides it is live, so offers fail closed. Names the
+        // status for the same reason `check_transition` does: it is the one fact the caller
+        // does not have.
         _ => Err(format!(
             "this conversation is '{current}' and can no longer take new offers"
         )),
@@ -828,9 +830,10 @@ pub async fn update_status(pool: &PgPool, match_id: Uuid, to: MatchStatus) -> Re
     }
 
     // `resolved_at` marks the end of a thread, so only the two terminal statuses set it.
-    let resolved_at = match to {
-        MatchStatus::Completed | MatchStatus::Withdrawn => Some(Utc::now()),
-        MatchStatus::Proposed | MatchStatus::Accepted => None,
+    let resolved_at = if to.is_resolved() {
+        Some(Utc::now())
+    } else {
+        None
     };
 
     sqlx::query("UPDATE matches SET status = $2, resolved_at = $3 WHERE id = $1")
```

## The request

Run the Komun pre-merge quality gate over this change and stop at your two human checkpoints.

The change is small and self-contained: it gives the two terminal negotiation statuses a name on
the enum (`MatchStatus::is_resolved`, beside `PostKind::is_market` and `Role::is_admin`), resolves
the `resolved_at` computation through it, and pins the resolved set with a test over
`MatchStatus::ALL`. `check_offer_allowed` deliberately keeps its explicit two-variant live set, so
an unknown fifth status is refused rather than allowed; that reasoning is a comment where the
match is.

## Evidence gathered by hand, to verify rather than trust

- `scripts/classify-change.py` classes all three files as deterministic work:
  `requires_governed_check` false, `touches_policy` false.
- `cargo fmt --check` clean.
- `cargo clippy --release --all-targets --offline -- -D warnings` exits 0.
- `cargo test --workspace`: 22 passed in komun-core, 138 in komun-server.

Treat those as claims from the requester, not as gate evidence. Produce the evidence yourself.

## What to hand back

The gate evidence for this change, the reviewer's verdict, and any finding that should block the
merge — in your role's own handoff format.
