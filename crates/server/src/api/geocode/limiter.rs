//! Outbound rate limiting for the Nominatim geocode proxy.

use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::Instant;

/// Spaces outbound calls so that no two start less than `interval` apart.
///
/// Callers `acquire().await` a slot. The first caller runs immediately; every
/// later caller is assigned a slot one `interval` after the previous one and
/// sleeps until then, so requests **queue** instead of being dropped or fired
/// early. A single instance is shared by the whole process, which matches
/// Nominatim's per-application usage policy.
pub struct RateLimiter {
    interval: Duration,
    next_slot: Mutex<Option<Instant>>,
}

impl RateLimiter {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            next_slot: Mutex::new(None),
        }
    }

    /// Waits until this caller's slot is due, then returns.
    pub async fn acquire(&self) {
        let wait_until = {
            let mut next_slot = self.next_slot.lock().await;
            let now = Instant::now();
            let slot = next_slot.map_or(now, |scheduled| scheduled.max(now));
            *next_slot = Some(slot + self.interval);
            slot
        };

        tokio::time::sleep_until(wait_until).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // The clock is paused, so `Instant::now()` and both sleeps below read
    // tokio's timer rather than the wall clock. The 50 ms checkpoint and the
    // 200 ms slot are then exact, instead of being eaten into by the first
    // caller's own latency. `acquire()` is already on that timer and is
    // unchanged.
    #[tokio::test(start_paused = true)]
    async fn second_lookup_is_queued_not_dropped_or_fired_early() {
        let limiter = Arc::new(RateLimiter::new(Duration::from_millis(200)));
        let upstream = Arc::new(AtomicUsize::new(0));
        let start = Instant::now();

        let spawn = || {
            let limiter = limiter.clone();
            let upstream = upstream.clone();
            tokio::spawn(async move {
                limiter.acquire().await;
                upstream.fetch_add(1, Ordering::SeqCst);
                Instant::now()
            })
        };

        let first = spawn();
        let second = spawn();

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            upstream.load(Ordering::SeqCst),
            1,
            "the second lookup reached upstream before its slot was due"
        );

        let a = first.await.unwrap();
        let b = second.await.unwrap();
        let (earliest, latest) = if a <= b { (a, b) } else { (b, a) };

        assert!(
            latest.duration_since(earliest) >= Duration::from_millis(200),
            "queued lookup fired too early ({earliest:?} -> {latest:?})"
        );
        assert!(latest.duration_since(start) >= Duration::from_millis(200));
        assert_eq!(
            upstream.load(Ordering::SeqCst),
            2,
            "the queued lookup must still run"
        );
    }
}
