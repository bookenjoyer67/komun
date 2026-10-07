use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use komun_core::models::PostStatus;

#[derive(Debug, FromRow, serde::Serialize)]
pub struct Report {
    pub id: Uuid,
    pub reporter_id: Uuid,
    pub post_id: Uuid,
    pub reason: String,
    pub status: String,
    pub admin_notes: Option<String>,
    pub resolved_by: Option<Uuid>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

pub async fn create_report(
    pool: &PgPool,
    reporter_id: Uuid,
    post_id: Uuid,
    reason: &str,
) -> Result<Report> {
    let existing = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM reports WHERE reporter_id = $1 AND post_id = $2 AND status = 'pending'"
    )
    .bind(reporter_id)
    .bind(post_id)
    .fetch_one(pool)
    .await?;

    if existing > 0 {
        return Err(anyhow!("already reported this post"));
    }

    let id = Uuid::now_v7();
    let now = Utc::now();

    sqlx::query(
        "INSERT INTO reports (id, reporter_id, post_id, reason, status, created_at) VALUES ($1, $2, $3, $4, 'pending', $5)"
    )
    .bind(id)
    .bind(reporter_id)
    .bind(post_id)
    .bind(reason)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(Report {
        id,
        reporter_id,
        post_id,
        reason: reason.to_string(),
        status: "pending".into(),
        admin_notes: None,
        resolved_by: None,
        resolved_at: None,
        created_at: now,
    })
}

pub async fn list_reports(pool: &PgPool) -> Result<Vec<Report>> {
    let rows = sqlx::query_as::<_, Report>(
        "SELECT id, reporter_id, post_id, reason, status, admin_notes, resolved_by, resolved_at, created_at FROM reports ORDER BY created_at DESC"
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn resolve_report(
    pool: &PgPool,
    report_id: Uuid,
    status: &str,
    admin_notes: Option<&str>,
    resolved_by: Uuid,
) -> Result<()> {
    let now = Utc::now();
    sqlx::query(
        "UPDATE reports SET status = $1, admin_notes = $2, resolved_by = $3, resolved_at = $4 WHERE id = $5"
    )
    .bind(status)
    .bind(admin_notes)
    .bind(resolved_by)
    .bind(now)
    .bind(report_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub const MODERATION_NOTIFICATION_KIND: &str = "moderation";

/// Counted from the removal itself, not from when the author first read the notice.
pub const APPEAL_WINDOW_DAYS: i64 = 14;

pub const NOT_HIDDEN: &str = "this post is not hidden, so there is nothing to appeal";
pub const NO_RECORDED_REMOVAL: &str =
    "this post was hidden before removals recorded a reason, so there is no removal to appeal";
pub const ALREADY_APPEALED: &str = "this removal has already been appealed";
pub const APPEAL_ALREADY_DECIDED: &str = "this appeal has already been decided";
pub const REMOVAL_SUPERSEDED: &str =
    "a later removal now holds this post, so a grant would overturn a decision nobody appealed";

/// The outcome of a step the post's or the appeal's own state may refuse. `Conflict` carries the
/// sentence the API turns into a 409, kept out of `Err` so a database failure is never reported as
/// a state error.
#[derive(Debug)]
pub enum AppealStep<T> {
    Done(T),
    NotFound,
    Conflict(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppealDecision {
    Granted,
    Denied,
}

impl AppealDecision {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "granted" => Some(AppealDecision::Granted),
            "denied" => Some(AppealDecision::Denied),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AppealDecision::Granted => "granted",
            AppealDecision::Denied => "denied",
        }
    }
}

pub fn appeal_deadline(hidden_at: DateTime<Utc>) -> DateTime<Utc> {
    hidden_at + Duration::days(APPEAL_WINDOW_DAYS)
}

/// Inclusive: an appeal sent at the deadline itself is inside the window the author was promised.
pub fn appeal_window_open(hidden_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now <= appeal_deadline(hidden_at)
}

/// Only a pending appeal is decided, and only once. A grant returns the post to the status the
/// removal took it from, and is refused when a later removal now holds the post, because restoring
/// it would overturn a decision nobody appealed. `Ok(None)` means the post is left as it is.
pub fn check_appeal_resolution(
    appeal_status: &str,
    decision: AppealDecision,
    post_held_by_this_removal: bool,
    prior_status: &str,
) -> Result<Option<PostStatus>, String> {
    if appeal_status != "pending" {
        return Err(APPEAL_ALREADY_DECIDED.to_string());
    }
    match decision {
        AppealDecision::Denied => Ok(None),
        AppealDecision::Granted if post_held_by_this_removal => {
            Ok(Some(status_after_grant(prior_status)))
        }
        AppealDecision::Granted => Err(REMOVAL_SUPERSEDED.to_string()),
    }
}

/// A grant reverses moderation, so it never restores a moderation status: a post hidden while it
/// was already hidden or flagged comes back `Active`, as does a status this build cannot read.
/// Restoring `Hidden` would leave the post removed by the very grant that overturned its removal.
pub fn status_after_grant(prior_status: &str) -> PostStatus {
    match PostStatus::parse(prior_status) {
        Some(PostStatus::Hidden | PostStatus::Flagged) | None => PostStatus::Active,
        Some(prior) => prior,
    }
}

/// A re-hide records the status the first hide replaced rather than `Hidden`, so a grant on the
/// latest removal restores the post as it was before moderation began. With no earlier removal on
/// record (a post hidden before `004`) it records `Hidden`, and the grant falls back to `Active`.
/// A post that is not hidden has had any earlier removal reversed, so that removal is ignored.
pub fn prior_status_for_hide(
    current: PostStatus,
    previous_prior: Option<PostStatus>,
) -> PostStatus {
    match current {
        PostStatus::Hidden => previous_prior.unwrap_or(PostStatus::Hidden),
        other => other,
    }
}

/// A hide and the notice telling its author why land together or not at all. The notice goes
/// through `notifications::create`, which takes the pool rather than this transaction, so it is
/// written before the hide commits: a failed notice rolls the hide back. What remains is the
/// reverse case, a notice for a hide whose commit then fails, which tells the author too much
/// rather than too little. `Ok(None)` is a post that does not exist.
///
/// The status and the latest removal are read under this row lock, which every hide takes before
/// it records a removal, so neither can change between the read and the write.
pub async fn hide_post(
    pool: &PgPool,
    post_id: Uuid,
    actor_id: Uuid,
    reason: &str,
) -> Result<Option<Uuid>> {
    let mut tx = pool.begin().await?;

    let target: Option<(Uuid, String, String)> =
        sqlx::query_as("SELECT author_id, title, status FROM posts WHERE id = $1 FOR UPDATE")
            .bind(post_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((author_id, title, status)) = target else {
        return Ok(None);
    };
    let Some(current) = PostStatus::parse(&status) else {
        return Err(anyhow!("post {post_id} has unreadable status {status}"));
    };

    let previous_prior: Option<String> = sqlx::query_scalar(
        "SELECT prior_status FROM moderation_actions
         WHERE post_id = $1 AND action = 'hidden'
         ORDER BY created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(post_id)
    .fetch_optional(&mut *tx)
    .await?;
    let previous = previous_prior.as_deref().and_then(PostStatus::parse);
    let prior_status = prior_status_for_hide(current, previous);

    sqlx::query("UPDATE posts SET status = 'hidden' WHERE id = $1")
        .bind(post_id)
        .execute(&mut *tx)
        .await?;

    let action_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO moderation_actions (id, post_id, actor_id, action, reason, prior_status)
         VALUES ($1, $2, $3, 'hidden', $4, $5)",
    )
    .bind(action_id)
    .bind(post_id)
    .bind(actor_id)
    .bind(reason)
    .bind(prior_status.as_str())
    .execute(&mut *tx)
    .await?;

    super::notifications::create(
        pool,
        author_id,
        MODERATION_NOTIFICATION_KIND,
        &format!("Your post was removed from public view: {title}"),
        Some(reason),
        Some(&format!("/p/{post_id}")),
    )
    .await?;

    tx.commit().await?;
    Ok(Some(action_id))
}

#[derive(Debug, FromRow, serde::Serialize)]
pub struct Appeal {
    pub id: Uuid,
    pub action_id: Uuid,
    pub author_id: Uuid,
    pub body: String,
    pub status: String,
    pub admin_notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

/// Anyone but the author gets `NotFound`, the answer a missing id gets, so a hidden post is
/// confirmed to nobody else. A second appeal on the same removal is refused by `UNIQUE (action_id)`
/// rather than a SELECT first, which two concurrent requests could both pass.
pub async fn file_appeal(
    pool: &PgPool,
    post_id: Uuid,
    author_id: Uuid,
    body: &str,
) -> Result<AppealStep<Appeal>> {
    let mut tx = pool.begin().await?;

    let post: Option<(Uuid, String)> =
        sqlx::query_as("SELECT author_id, status FROM posts WHERE id = $1 FOR UPDATE")
            .bind(post_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((owner, status)) = post else {
        return Ok(AppealStep::NotFound);
    };
    if owner != author_id {
        return Ok(AppealStep::NotFound);
    }
    if PostStatus::parse(&status) != Some(PostStatus::Hidden) {
        return Ok(AppealStep::Conflict(NOT_HIDDEN.to_string()));
    }

    let removal: Option<(Uuid, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, created_at FROM moderation_actions
         WHERE post_id = $1 AND action = 'hidden'
         ORDER BY created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(post_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((action_id, hidden_at)) = removal else {
        return Ok(AppealStep::Conflict(NO_RECORDED_REMOVAL.to_string()));
    };
    if !appeal_window_open(hidden_at, Utc::now()) {
        return Ok(AppealStep::Conflict(format!(
            "the {APPEAL_WINDOW_DAYS}-day window to appeal this removal has closed"
        )));
    }

    let inserted = sqlx::query_as::<_, Appeal>(
        "INSERT INTO appeals (id, action_id, author_id, body)
         VALUES ($1, $2, $3, $4)
         RETURNING id, action_id, author_id, body, status, admin_notes, created_at, resolved_at",
    )
    .bind(Uuid::now_v7())
    .bind(action_id)
    .bind(author_id)
    .bind(body)
    .fetch_one(&mut *tx)
    .await;

    let appeal = match inserted {
        Ok(appeal) => appeal,
        Err(e) if is_unique_violation(&e) => {
            return Ok(AppealStep::Conflict(ALREADY_APPEALED.to_string()))
        }
        Err(e) => return Err(e.into()),
    };

    tx.commit().await?;
    Ok(AppealStep::Done(appeal))
}

/// What the author of a hidden post is told about it. `appeal_admin_notes` reaches the author on
/// purpose: a denial whose note only moderators can read is a refusal with no reason.
#[derive(Debug, serde::Serialize)]
pub struct ModerationNotice {
    pub post_id: Uuid,
    pub post_title: String,
    pub action_id: Uuid,
    pub reason: String,
    pub hidden_at: DateTime<Utc>,
    pub appeal_deadline: DateTime<Utc>,
    pub appeal_open: bool,
    pub appeal_status: Option<String>,
    pub appeal_admin_notes: Option<String>,
}

#[derive(FromRow)]
struct NoticeRow {
    post_id: Uuid,
    post_title: String,
    action_id: Uuid,
    reason: String,
    hidden_at: DateTime<Utc>,
    appeal_status: Option<String>,
    appeal_admin_notes: Option<String>,
}

/// Each of the author's hidden posts with the removal that holds it now, newest first. A post
/// hidden before removals recorded a reason has no row to show and is left out.
pub async fn moderation_for_author(
    pool: &PgPool,
    author_id: Uuid,
    now: DateTime<Utc>,
) -> Result<Vec<ModerationNotice>> {
    let rows = sqlx::query_as::<_, NoticeRow>(
        r#"SELECT DISTINCT ON (p.id)
                  p.id AS post_id, p.title AS post_title, a.id AS action_id, a.reason,
                  a.created_at AS hidden_at,
                  ap.status AS appeal_status, ap.admin_notes AS appeal_admin_notes
           FROM posts p
           JOIN moderation_actions a ON a.post_id = p.id AND a.action = 'hidden'
           LEFT JOIN appeals ap ON ap.action_id = a.id
           WHERE p.author_id = $1 AND p.status = 'hidden'
           ORDER BY p.id, a.created_at DESC, a.id DESC"#,
    )
    .bind(author_id)
    .fetch_all(pool)
    .await?;

    let mut notices: Vec<ModerationNotice> = rows
        .into_iter()
        .map(|r| ModerationNotice {
            appeal_open: r.appeal_status.is_none() && appeal_window_open(r.hidden_at, now),
            appeal_deadline: appeal_deadline(r.hidden_at),
            post_id: r.post_id,
            post_title: r.post_title,
            action_id: r.action_id,
            reason: r.reason,
            hidden_at: r.hidden_at,
            appeal_status: r.appeal_status,
            appeal_admin_notes: r.appeal_admin_notes,
        })
        .collect();
    notices.sort_by_key(|notice| std::cmp::Reverse(notice.hidden_at));
    Ok(notices)
}

#[derive(Debug, FromRow, serde::Serialize)]
pub struct AppealView {
    pub id: Uuid,
    pub post_id: Uuid,
    pub post_title: String,
    pub author_id: Uuid,
    pub author_name: String,
    pub reason: String,
    pub body: String,
    pub status: String,
    pub admin_notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

/// Undecided appeals first, so the queue opens on the work still owed.
pub async fn list_appeals(pool: &PgPool) -> Result<Vec<AppealView>> {
    let rows = sqlx::query_as::<_, AppealView>(
        r#"SELECT ap.id, a.post_id, p.title AS post_title, ap.author_id,
                  u.display_name AS author_name, a.reason, ap.body, ap.status, ap.admin_notes,
                  ap.created_at, ap.resolved_at
           FROM appeals ap
           JOIN moderation_actions a ON a.id = ap.action_id
           JOIN posts p ON p.id = a.post_id
           JOIN users u ON u.id = ap.author_id
           ORDER BY ap.status = 'pending' DESC, ap.created_at DESC, ap.id DESC"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[derive(FromRow)]
struct AppealTarget {
    status: String,
    post_id: Uuid,
    prior_status: String,
    held_by_this_removal: bool,
}

/// The appeal and its post are locked together, so a hide landing mid-decision cannot be undone
/// by a grant that read the post before it.
pub async fn resolve_appeal(
    pool: &PgPool,
    appeal_id: Uuid,
    decision: AppealDecision,
    admin_notes: Option<&str>,
) -> Result<AppealStep<()>> {
    let mut tx = pool.begin().await?;

    let target = sqlx::query_as::<_, AppealTarget>(
        r#"SELECT ap.status, a.post_id, a.prior_status,
                  COALESCE(p.status = 'hidden' AND a.id = (
                      SELECT m.id FROM moderation_actions m
                      WHERE m.post_id = a.post_id AND m.action = 'hidden'
                      ORDER BY m.created_at DESC, m.id DESC
                      LIMIT 1
                  ), false) AS held_by_this_removal
           FROM appeals ap
           JOIN moderation_actions a ON a.id = ap.action_id
           JOIN posts p ON p.id = a.post_id
           WHERE ap.id = $1
           FOR UPDATE OF ap, p"#,
    )
    .bind(appeal_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(target) = target else {
        return Ok(AppealStep::NotFound);
    };

    let held = target.held_by_this_removal;
    let prior = target.prior_status.as_str();
    let restore = match check_appeal_resolution(&target.status, decision, held, prior) {
        Ok(restore) => restore,
        Err(why) => return Ok(AppealStep::Conflict(why)),
    };

    sqlx::query(
        "UPDATE appeals SET status = $2, admin_notes = $3, resolved_at = now() WHERE id = $1",
    )
    .bind(appeal_id)
    .bind(decision.as_str())
    .bind(admin_notes)
    .execute(&mut *tx)
    .await?;

    if let Some(status) = restore {
        sqlx::query("UPDATE posts SET status = $2 WHERE id = $1")
            .bind(target.post_id)
            .bind(status.as_str())
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(AppealStep::Done(()))
}

/// `23505` is Postgres' unique-violation SQLSTATE; `appeals` carries one unique constraint besides
/// its primary key, whose ids are freshly minted, so on this insert the code identifies it.
fn is_unique_violation(e: &sqlx::Error) -> bool {
    match e {
        sqlx::Error::Database(db) => db.code().is_some_and(|code| code == "23505"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_appeal_window_is_the_approved_fourteen_days() {
        // Checked at compile time: clippy rejects a run-time assert over a constant.
        const _: () = assert!(APPEAL_WINDOW_DAYS == 14);
    }

    #[test]
    fn an_appeal_inside_the_window_is_open() {
        let hidden_at = Utc::now();
        let day_before = hidden_at + Duration::days(APPEAL_WINDOW_DAYS - 1);
        assert!(appeal_window_open(hidden_at, hidden_at));
        assert!(appeal_window_open(hidden_at, day_before));
    }

    #[test]
    fn an_appeal_at_the_deadline_is_open() {
        let hidden_at = Utc::now();
        let deadline = hidden_at + Duration::days(APPEAL_WINDOW_DAYS);
        assert_eq!(appeal_deadline(hidden_at), deadline);
        assert!(appeal_window_open(hidden_at, deadline));
    }

    #[test]
    fn an_appeal_after_the_window_is_closed() {
        let hidden_at = Utc::now();
        let deadline = hidden_at + Duration::days(APPEAL_WINDOW_DAYS);
        let second_late = deadline + Duration::seconds(1);
        let day_late = deadline + Duration::days(1);
        assert!(!appeal_window_open(hidden_at, second_late));
        assert!(!appeal_window_open(hidden_at, day_late));
    }

    #[test]
    fn a_grant_restores_an_active_post_to_active() {
        assert_eq!(
            check_appeal_resolution("pending", AppealDecision::Granted, true, "active"),
            Ok(Some(PostStatus::Active))
        );
    }

    #[test]
    fn a_grant_restores_a_matched_post_to_matched() {
        assert_eq!(
            check_appeal_resolution("pending", AppealDecision::Granted, true, "matched"),
            Ok(Some(PostStatus::Matched))
        );
    }

    #[test]
    fn a_grant_restores_each_status_the_author_or_its_lifecycle_set() {
        let lifecycle = [
            PostStatus::Active,
            PostStatus::Matched,
            PostStatus::Fulfilled,
            PostStatus::Expired,
            PostStatus::Withdrawn,
        ];
        for prior in lifecycle {
            let raw = prior.as_str();
            assert_eq!(
                check_appeal_resolution("pending", AppealDecision::Granted, true, raw),
                Ok(Some(prior)),
                "a grant on a post whose prior status was {raw}"
            );
        }
    }

    #[test]
    fn a_grant_never_restores_a_moderation_status() {
        for prior in ["hidden", "flagged", "", "Matched", "paused"] {
            assert_eq!(
                check_appeal_resolution("pending", AppealDecision::Granted, true, prior),
                Ok(Some(PostStatus::Active)),
                "a grant on a post whose prior status was {prior:?}"
            );
        }
    }

    #[test]
    fn a_first_hide_records_the_status_it_replaces() {
        let first = prior_status_for_hide(PostStatus::Matched, None);
        assert_eq!(first, PostStatus::Matched);
    }

    #[test]
    fn a_hide_after_a_reversed_removal_records_the_current_status() {
        let again = prior_status_for_hide(PostStatus::Matched, Some(PostStatus::Active));
        assert_eq!(again, PostStatus::Matched);
    }

    #[test]
    fn a_re_hide_carries_the_first_hides_prior_status_forward() {
        let re_hide = prior_status_for_hide(PostStatus::Hidden, Some(PostStatus::Matched));
        assert_eq!(re_hide, PostStatus::Matched);
    }

    #[test]
    fn a_re_hide_with_no_earlier_removal_records_hidden() {
        let re_hide = prior_status_for_hide(PostStatus::Hidden, None);
        assert_eq!(re_hide, PostStatus::Hidden);
    }

    #[test]
    fn a_paused_post_hidden_twice_comes_back_paused_on_grant() {
        let first = prior_status_for_hide(PostStatus::Matched, None);
        let second = prior_status_for_hide(PostStatus::Hidden, Some(first));
        assert_eq!(status_after_grant(second.as_str()), PostStatus::Matched);
    }

    #[test]
    fn a_denied_appeal_leaves_the_post_as_it_is() {
        for held in [true, false] {
            assert_eq!(
                check_appeal_resolution("pending", AppealDecision::Denied, held, "matched"),
                Ok(None),
                "a denial with the post held by this removal: {held}"
            );
        }
    }

    #[test]
    fn a_grant_cannot_overturn_a_later_removal() {
        assert_eq!(
            check_appeal_resolution("pending", AppealDecision::Granted, false, "matched"),
            Err(REMOVAL_SUPERSEDED.to_string())
        );
    }

    #[test]
    fn a_decided_appeal_is_not_decided_again() {
        for decided in ["granted", "denied"] {
            for decision in [AppealDecision::Granted, AppealDecision::Denied] {
                assert_eq!(
                    check_appeal_resolution(decided, decision, true, "matched"),
                    Err(APPEAL_ALREADY_DECIDED.to_string()),
                    "{} on an appeal already {decided}",
                    decision.as_str()
                );
            }
        }
    }

    #[test]
    fn decisions_parse_from_their_wire_words_only() {
        for decision in [AppealDecision::Granted, AppealDecision::Denied] {
            assert_eq!(AppealDecision::parse(decision.as_str()), Some(decision));
        }
        for other in ["", "pending", "resolved", "Granted", " denied"] {
            assert_eq!(AppealDecision::parse(other), None, "{other:?}");
        }
    }
}
