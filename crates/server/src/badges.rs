//! Reciprocity badges (D3). Each one is a yes/no fact about a user's own history, shown on that
//! user's profile only: no score is derived from badges and nothing is ranked by them.

use chrono::{DateTime, Duration, Utc};

pub(crate) const RELIABLE_MIN_ENDORSEMENTS: i64 = 3;
pub(crate) const REGULAR_GIVER_MIN_POSTS: i64 = 5;
pub(crate) const REGULAR_GIVER_MIN_SPAN_DAYS: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Badge {
    Reliable,
    RegularGiver,
}

impl Badge {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Badge::Reliable => "reliable",
            Badge::RegularGiver => "regular_giver",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Badge::Reliable => "Reliable",
            Badge::RegularGiver => "Regular giver",
        }
    }
}

pub(crate) fn is_reliable(endorsement_count: i64) -> bool {
    endorsement_count >= RELIABLE_MIN_ENDORSEMENTS
}

pub(crate) fn is_regular_giver(
    give_count: i64,
    first_give_at: Option<DateTime<Utc>>,
    last_give_at: Option<DateTime<Utc>>,
) -> bool {
    let (Some(first), Some(last)) = (first_give_at, last_give_at) else {
        return false;
    };
    give_count >= REGULAR_GIVER_MIN_POSTS
        && last - first >= Duration::days(REGULAR_GIVER_MIN_SPAN_DAYS)
}

pub(crate) fn earned(
    endorsement_count: i64,
    give_count: i64,
    first_give_at: Option<DateTime<Utc>>,
    last_give_at: Option<DateTime<Utc>>,
) -> Vec<Badge> {
    let mut badges = Vec::new();
    if is_reliable(endorsement_count) {
        badges.push(Badge::Reliable);
    }
    if is_regular_giver(give_count, first_give_at, last_give_at) {
        badges.push(Badge::RegularGiver);
    }
    badges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history(span: Duration) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
        let first = Utc::now();
        (Some(first), Some(first + span))
    }

    fn span_threshold() -> Duration {
        Duration::days(REGULAR_GIVER_MIN_SPAN_DAYS)
    }

    #[test]
    fn thresholds_are_the_approved_d3_values() {
        // Checked at compile time: clippy rejects a run-time assert over a constant.
        const _: () = assert!(RELIABLE_MIN_ENDORSEMENTS == 3);
        const _: () = assert!(REGULAR_GIVER_MIN_POSTS == 5);
        const _: () = assert!(REGULAR_GIVER_MIN_SPAN_DAYS == 30);
    }

    #[test]
    fn reliable_is_withheld_below_the_threshold() {
        assert!(!is_reliable(0));
        assert!(!is_reliable(RELIABLE_MIN_ENDORSEMENTS - 1));
    }

    #[test]
    fn reliable_is_earned_at_the_threshold() {
        assert!(is_reliable(RELIABLE_MIN_ENDORSEMENTS));
    }

    #[test]
    fn reliable_is_earned_above_the_threshold() {
        assert!(is_reliable(RELIABLE_MIN_ENDORSEMENTS + 1));
    }

    #[test]
    fn regular_giver_is_withheld_below_the_post_threshold() {
        let (first, last) = history(span_threshold() * 2);
        assert!(!is_regular_giver(REGULAR_GIVER_MIN_POSTS - 1, first, last));
    }

    #[test]
    fn regular_giver_is_earned_at_the_post_threshold() {
        let (first, last) = history(span_threshold() * 2);
        assert!(is_regular_giver(REGULAR_GIVER_MIN_POSTS, first, last));
    }

    #[test]
    fn regular_giver_is_earned_above_the_post_threshold() {
        let (first, last) = history(span_threshold() * 2);
        assert!(is_regular_giver(REGULAR_GIVER_MIN_POSTS + 1, first, last));
    }

    #[test]
    fn regular_giver_is_withheld_below_the_span_threshold() {
        let (first, last) = history(span_threshold() - Duration::seconds(1));
        assert!(!is_regular_giver(REGULAR_GIVER_MIN_POSTS, first, last));
    }

    #[test]
    fn regular_giver_is_earned_at_the_span_threshold() {
        let (first, last) = history(span_threshold());
        assert!(is_regular_giver(REGULAR_GIVER_MIN_POSTS, first, last));
    }

    #[test]
    fn regular_giver_is_earned_above_the_span_threshold() {
        let (first, last) = history(span_threshold() + Duration::days(1));
        assert!(is_regular_giver(REGULAR_GIVER_MIN_POSTS, first, last));
    }

    #[test]
    fn regular_giver_is_withheld_without_a_dated_history() {
        assert!(!is_regular_giver(REGULAR_GIVER_MIN_POSTS, None, None));
    }

    #[test]
    fn earned_lists_each_badge_once_in_a_fixed_order() {
        let (first, last) = history(span_threshold());
        let both = earned(
            RELIABLE_MIN_ENDORSEMENTS,
            REGULAR_GIVER_MIN_POSTS,
            first,
            last,
        );
        assert_eq!(both, vec![Badge::Reliable, Badge::RegularGiver]);
        assert!(earned(0, 0, None, None).is_empty());
    }

    #[test]
    fn each_badge_has_a_stable_code_and_label() {
        assert_eq!(Badge::Reliable.code(), "reliable");
        assert_eq!(Badge::Reliable.label(), "Reliable");
        assert_eq!(Badge::RegularGiver.code(), "regular_giver");
        assert_eq!(Badge::RegularGiver.label(), "Regular giver");
    }
}
