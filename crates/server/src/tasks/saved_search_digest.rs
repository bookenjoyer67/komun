//! The saved-search digest: in-app always, by email only when `[email]` is configured, and at
//! most one per saved search per window. A digest lists posts by time or by distance and by
//! nothing else: an engagement order is refusal R-1 in ADR-008.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use komun_core::models::PostStatus;
use lettre::message::{header::ContentType, Mailbox, Message};
use tokio::time;
use uuid::Uuid;

use crate::api::posts::validate_filters;
use crate::api::saved_searches::feed_filters;
use crate::db::posts::{FeedSort, MAX_LIMIT};
use crate::db::saved_searches::DueSearch;
use crate::AppState;

pub(crate) const DIGEST_WINDOW_HOURS: i64 = 24;

pub(crate) const NOTIFICATION_KIND: &str = "saved_search";

const TITLE: &str = "New posts for your saved search";

const FIRST_RUN_DELAY: StdDuration = StdDuration::from_secs(180);

/// A due search waits at most this long past the end of its window.
const RUN_INTERVAL: StdDuration = StdDuration::from_secs(6 * 3600);

/// `db::posts::list` orders by urgency before time, so paging cannot stop at the first post older
/// than the window; a search matching more active posts than this many pages can miss a new one.
const MAX_PAGES: i64 = 5;

/// What the digest knows about a matching post. It carries no counter, score or urgency, so
/// nothing in it can be used to rank by engagement.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DigestCandidate {
    pub id: Uuid,
    pub author_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub distance_km: Option<f64>,
}

/// One saved search's packed posts in one run, and when its owner was last notified.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DigestBatch {
    pub search_id: Uuid,
    pub last_notified_at: DateTime<Utc>,
    pub post_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DigestNotice {
    pub search_id: Uuid,
    pub post_ids: Vec<Uuid>,
}

pub(crate) fn window() -> Duration {
    Duration::hours(DIGEST_WINDOW_HOURS)
}

/// The ids of the posts a digest lists, in the order it lists them.
pub(crate) fn pack(
    candidates: &[DigestCandidate],
    owner: Uuid,
    since: DateTime<Utc>,
    sort: FeedSort,
) -> Vec<Uuid> {
    let mut seen = HashSet::new();
    let mut picked: Vec<&DigestCandidate> = candidates
        .iter()
        .filter(|c| c.created_at > since && c.author_id != owner)
        .filter(|c| seen.insert(c.id))
        .collect();

    match sort {
        FeedSort::Recency => picked.sort_by(|a, b| newest_first(a, b)),
        FeedSort::Distance => picked.sort_by(|a, b| {
            nearest_first(a.distance_km, b.distance_km).then_with(|| newest_first(a, b))
        }),
    }

    picked.into_iter().map(|c| c.id).collect()
}

fn newest_first(a: &DigestCandidate, b: &DigestCandidate) -> Ordering {
    b.created_at
        .cmp(&a.created_at)
        .then_with(|| b.id.cmp(&a.id))
}

fn nearest_first(a: Option<f64>, b: Option<f64>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// The notifications one run sends.
pub(crate) fn notices(batches: &[DigestBatch], now: DateTime<Utc>) -> Vec<DigestNotice> {
    let mut sent: Vec<DigestNotice> = Vec::new();
    for batch in batches {
        if now - batch.last_notified_at < window() || batch.post_ids.is_empty() {
            continue;
        }
        let index = match sent.iter().position(|n| n.search_id == batch.search_id) {
            Some(index) => index,
            None => {
                sent.push(DigestNotice {
                    search_id: batch.search_id,
                    post_ids: Vec::new(),
                });
                sent.len() - 1
            }
        };
        let notice = &mut sent[index];
        for id in &batch.post_ids {
            if !notice.post_ids.contains(id) {
                notice.post_ids.push(*id);
            }
        }
    }
    sent
}

pub async fn digest_loop(state: AppState) {
    time::sleep(FIRST_RUN_DELAY).await;

    loop {
        if let Err(e) = run_once(&state).await {
            tracing::warn!("saved-search digest error: {e}");
        }
        time::sleep(RUN_INTERVAL).await;
    }
}

pub(crate) async fn run_once(state: &AppState) -> anyhow::Result<()> {
    let now = Utc::now();
    let due = crate::db::saved_searches::due(&state.pool, now - window()).await?;

    let mut batches = Vec::with_capacity(due.len());
    for search in &due {
        match matching_posts(state, search).await {
            Ok(candidates) => batches.push(DigestBatch {
                search_id: search.id,
                last_notified_at: search.last_notified_at,
                post_ids: pack(
                    &candidates,
                    search.user_id,
                    search.last_notified_at,
                    digest_order(search),
                ),
            }),
            // One search that fails must not hold back everyone else's digest.
            Err(e) => tracing::warn!("saved search {} skipped: {e}", search.id),
        }
    }

    let by_id: HashMap<Uuid, &DueSearch> = due.iter().map(|s| (s.id, s)).collect();
    for notice in notices(&batches, now) {
        let Some(search) = by_id.get(&notice.search_id) else {
            continue;
        };
        match notify(state, search, &notice, now).await {
            Ok(true) => mail(state, search, &notice).await,
            Ok(false) => {}
            Err(e) => tracing::warn!("saved search {} digest not sent: {e}", search.id),
        }
    }
    Ok(())
}

/// The same filters the search runs live, through the same validator and the same feed query;
/// only the window and the owner are applied afterwards, by [`pack`].
async fn matching_posts(
    state: &AppState,
    search: &DueSearch,
) -> anyhow::Result<Vec<DigestCandidate>> {
    let mut found = Vec::new();
    for page in 0..MAX_PAGES {
        let mut raw = feed_filters(
            search.q.as_deref(),
            search.kind.as_deref(),
            search.category.as_deref(),
            search.near_lat,
            search.near_lon,
            search.radius_km,
        );
        raw.status = Some(PostStatus::Active.as_str().to_string());
        raw.limit = Some(MAX_LIMIT.to_string());
        raw.offset = Some((page * MAX_LIMIT).to_string());
        let filter = validate_filters(&raw)
            .map_err(|why| anyhow::anyhow!("its filters no longer validate: {why}"))?;

        let posts = crate::db::posts::list(&state.pool, &filter).await?;
        let last_page = (posts.len() as i64) < filter.limit;
        found.extend(posts.into_iter().map(|item| DigestCandidate {
            id: item.post.id,
            author_id: item.post.author_id,
            created_at: item.post.created_at,
            distance_km: item.distance_km,
        }));
        if last_page {
            break;
        }
    }
    Ok(found)
}

fn digest_order(search: &DueSearch) -> FeedSort {
    if search.near_lat.is_some() && search.near_lon.is_some() {
        FeedSort::Distance
    } else {
        FeedSort::Recency
    }
}

/// `Ok(false)` when another run claimed this window first. The claim is taken before the
/// notification is written and committed after it, so a failed write leaves the window open for
/// the next run; a commit that fails after the write can repeat one digest, never drop one.
async fn notify(
    state: &AppState,
    search: &DueSearch,
    notice: &DigestNotice,
    now: DateTime<Utc>,
) -> anyhow::Result<bool> {
    let mut tx = state.pool.begin().await?;
    let claimed =
        crate::db::saved_searches::claim_window(&mut tx, search.id, search.last_notified_at, now)
            .await?;
    if !claimed {
        return Ok(false);
    }

    crate::db::notifications::create(
        &state.pool,
        search.user_id,
        NOTIFICATION_KIND,
        TITLE,
        Some(&summary(notice.post_ids.len(), words(search))),
        Some(&digest_link(search.q.as_deref())),
    )
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// A delivery failure is logged without its error: an SMTP reply can quote the recipient's address.
async fn mail(state: &AppState, search: &DueSearch, notice: &DigestNotice) {
    let Some(mailer) = state.mailer.as_ref() else {
        return;
    };
    let recipient =
        match crate::db::saved_searches::verified_recipient(&state.pool, search.user_id).await {
            Ok(Some(recipient)) => recipient,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!("saved search {} digest mail skipped: {e}", search.id);
                return;
            }
        };
    let (email, display_name) = recipient;

    let link = format!(
        "{}{}",
        mailer.public_url(),
        digest_link(search.q.as_deref())
    );
    let sent = match digest_message(
        mailer.from_address(),
        mailer.public_url(),
        &email,
        &display_name,
        &summary(notice.post_ids.len(), words(search)),
        &link,
    ) {
        Ok(message) => mailer.send(message).await,
        Err(e) => Err(e),
    };
    if sent.is_err() {
        tracing::warn!("saved search {} digest mail not delivered", search.id);
    }
}

fn words(search: &DueSearch) -> &str {
    search
        .label
        .as_deref()
        .or(search.q.as_deref())
        .unwrap_or("your saved search")
}

fn summary(count: usize, words: &str) -> String {
    if count == 1 {
        format!("1 new post matches \"{words}\"")
    } else {
        format!("{count} new posts match \"{words}\"")
    }
}

/// The centre stays out of the link: a URL is shared, logged and kept in history, and the search
/// page takes its centre from the location store instead.
fn digest_link(q: Option<&str>) -> String {
    match q {
        Some(q) => format!("/search?q={}", encode_query_value(q)),
        None => "/search".to_string(),
    }
}

fn encode_query_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn digest_message(
    from: &Mailbox,
    public_url: &str,
    to: &str,
    display_name: &str,
    summary: &str,
    link: &str,
) -> anyhow::Result<Message> {
    let to: Mailbox = to
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid recipient address: {e}"))?;

    let body = format!(
        "Hello {display_name},\n\n\
         {summary} since your last digest:\n\n\
         {link}\n\n\
         You get at most one of these a day for each saved search. To stop them, delete the \
         search on your account page: {public_url}/account\n"
    );

    Message::builder()
        .from(from.clone())
        .to(to)
        .subject(TITLE)
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .map_err(|e| anyhow::anyhow!("failed to compose the digest email: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn base() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 7, 12, 0, 0)
            .single()
            .expect("a valid fixed instant")
    }

    fn post(author: Uuid, minutes_after_base: i64, distance_km: Option<f64>) -> DigestCandidate {
        DigestCandidate {
            id: Uuid::now_v7(),
            author_id: author,
            created_at: base() + Duration::minutes(minutes_after_base),
            distance_km,
        }
    }

    fn batch(search_id: Uuid, last_notified_at: DateTime<Utc>, post_ids: Vec<Uuid>) -> DigestBatch {
        DigestBatch {
            search_id,
            last_notified_at,
            post_ids,
        }
    }

    fn ids(posts: &[&DigestCandidate]) -> Vec<Uuid> {
        posts.iter().map(|p| p.id).collect()
    }

    // A post created at exactly `since` fell inside the previous window, so it was already sent.
    #[test]
    fn only_posts_created_after_the_last_digest_go_in() {
        let owner = Uuid::now_v7();
        let neighbour = Uuid::now_v7();
        let before = post(neighbour, -1, None);
        let at_cutoff = post(neighbour, 0, None);
        let after = post(neighbour, 1, None);

        let packed = pack(
            &[before, at_cutoff, after.clone()],
            owner,
            base(),
            FeedSort::Recency,
        );

        assert_eq!(packed, vec![after.id]);
    }

    #[test]
    fn the_owners_own_posts_are_left_out() {
        let owner = Uuid::now_v7();
        let neighbour = Uuid::now_v7();
        let own = post(owner, 5, None);
        let theirs = post(neighbour, 5, None);

        let packed = pack(&[own, theirs.clone()], owner, base(), FeedSort::Recency);

        assert_eq!(packed, vec![theirs.id]);
    }

    // The feed pages by offset, so a post published between two page reads comes back twice.
    #[test]
    fn a_post_read_twice_is_listed_once() {
        let owner = Uuid::now_v7();
        let p = post(Uuid::now_v7(), 5, None);

        let packed = pack(&[p.clone(), p.clone()], owner, base(), FeedSort::Recency);

        assert_eq!(packed, vec![p.id]);
    }

    #[test]
    fn a_recency_digest_is_newest_first_whatever_order_the_posts_arrive_in() {
        let owner = Uuid::now_v7();
        let author = Uuid::now_v7();
        let oldest = post(author, 10, Some(1.0));
        let middle = post(author, 20, Some(90.0));
        let newest = post(author, 30, Some(40.0));

        let packed = pack(
            &[middle.clone(), oldest.clone(), newest.clone()],
            owner,
            base(),
            FeedSort::Recency,
        );

        assert_eq!(packed, ids(&[&newest, &middle, &oldest]));
    }

    #[test]
    fn a_distance_digest_is_nearest_first_with_unlocated_posts_last() {
        let owner = Uuid::now_v7();
        let author = Uuid::now_v7();
        let far = post(author, 30, Some(80.0));
        let near = post(author, 10, Some(3.0));
        let nowhere = post(author, 40, None);
        let middle = post(author, 20, Some(25.0));

        let packed = pack(
            &[nowhere.clone(), far.clone(), near.clone(), middle.clone()],
            owner,
            base(),
            FeedSort::Distance,
        );

        assert_eq!(packed, ids(&[&near, &middle, &far, &nowhere]));
    }

    // R-1: the match has no wildcard arm, so a third feed order does not compile here until
    // someone decides what a digest does with it.
    #[test]
    fn every_digest_order_is_by_time_or_by_distance_and_nothing_else() {
        let owner = Uuid::now_v7();
        let author = Uuid::now_v7();
        let candidates = vec![
            post(author, 15, Some(60.0)),
            post(author, 45, None),
            post(author, 5, Some(2.0)),
            post(author, 30, Some(12.0)),
        ];
        let by_id = |id: &Uuid| {
            candidates
                .iter()
                .find(|c| c.id == *id)
                .expect("a packed id comes from the candidates")
        };

        for sort in FeedSort::ALL {
            let packed = pack(&candidates, owner, base(), *sort);
            assert_eq!(packed.len(), candidates.len(), "{sort:?} dropped a post");
            for pair in packed.windows(2) {
                let (a, b) = (by_id(&pair[0]), by_id(&pair[1]));
                match sort {
                    FeedSort::Recency => {
                        assert!(a.created_at >= b.created_at, "recency must be newest first")
                    }
                    FeedSort::Distance => assert!(
                        match (a.distance_km, b.distance_km) {
                            (Some(x), Some(y)) => x <= y,
                            (Some(_), None) | (None, None) => true,
                            (None, Some(_)) => false,
                        },
                        "distance must be nearest first, unlocated last"
                    ),
                }
            }
        }
    }

    #[test]
    fn many_matching_posts_make_one_notification_not_one_each() {
        let search = Uuid::now_v7();
        let posts: Vec<Uuid> = (0..30).map(|_| Uuid::now_v7()).collect();
        let now = base();

        let sent = notices(&[batch(search, now - window(), posts.clone())], now);

        assert_eq!(
            sent,
            vec![DigestNotice {
                search_id: search,
                post_ids: posts
            }]
        );
    }

    #[test]
    fn a_search_notified_inside_the_window_gets_nothing_until_it_closes() {
        let search = Uuid::now_v7();
        let posts = vec![Uuid::now_v7()];
        let now = base();

        let too_soon = now - window() + Duration::seconds(1);
        assert!(notices(&[batch(search, too_soon, posts.clone())], now).is_empty());

        let closed = now - window();
        assert_eq!(
            notices(&[batch(search, closed, posts)], now).len(),
            1,
            "the window has closed, so one digest is due"
        );
    }

    #[test]
    fn a_search_with_no_new_posts_gets_no_notification() {
        let now = base();
        let sent = notices(
            &[batch(Uuid::now_v7(), now - window() * 3, Vec::new())],
            now,
        );
        assert!(sent.is_empty());
    }

    #[test]
    fn one_search_seen_twice_in_a_run_still_gets_one_notification() {
        let search = Uuid::now_v7();
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();
        let now = base();
        let last = now - window();

        let sent = notices(
            &[
                batch(search, last, vec![first, second]),
                batch(search, last, vec![second]),
            ],
            now,
        );

        assert_eq!(
            sent,
            vec![DigestNotice {
                search_id: search,
                post_ids: vec![first, second]
            }]
        );
    }

    #[test]
    fn each_due_search_gets_its_own_notification() {
        let (wool, ladder) = (Uuid::now_v7(), Uuid::now_v7());
        let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
        let now = base();

        let mut sent = notices(
            &[
                batch(wool, now - window(), vec![a]),
                batch(ladder, now - window() * 2, vec![b]),
            ],
            now,
        );
        sent.sort_by_key(|n| n.search_id);

        let mut want = vec![
            DigestNotice {
                search_id: wool,
                post_ids: vec![a],
            },
            DigestNotice {
                search_id: ladder,
                post_ids: vec![b],
            },
        ];
        want.sort_by_key(|n| n.search_id);
        assert_eq!(sent, want);
    }

    #[test]
    fn the_summary_counts_posts_in_words_a_person_reads() {
        assert_eq!(summary(1, "wool"), "1 new post matches \"wool\"");
        assert_eq!(summary(3, "wool"), "3 new posts match \"wool\"");
    }

    #[test]
    fn the_link_reopens_the_search_and_carries_nothing_but_the_words() {
        assert_eq!(
            digest_link(Some("wool & yarn")),
            "/search?q=wool%20%26%20yarn"
        );
        assert_eq!(digest_link(None), "/search");
    }

    #[test]
    fn the_digest_mail_goes_to_the_owner_under_the_digest_subject() {
        let from: Mailbox = "Komun <noreply@example.org>".parse().expect("valid from");
        let message = digest_message(
            &from,
            "https://komun.example.org",
            "member@example.com",
            "Ada",
            "3 new posts match \"wool\"",
            "https://komun.example.org/search?q=wool",
        )
        .expect("compose");
        let wire = crate::auth::email::render(&message);

        assert!(wire.contains("To: member@example.com"), "{wire}");
        assert!(wire.contains(&format!("Subject: {TITLE}")), "{wire}");
    }

    #[test]
    fn a_bad_recipient_is_an_error_not_a_panic() {
        let from: Mailbox = "Komun <noreply@example.org>".parse().expect("valid from");
        let composed = digest_message(&from, "https://x.example", "not-an-address", "A", "s", "l");
        assert!(composed.is_err());
    }
}
