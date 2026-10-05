//! Outbound rate limiting for the Nominatim geocode proxy.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::time::Instant;

/// Past this expected wait a caller is refused at once: an unbounded queue lets anonymous
/// callers hold requests open for as long as they keep sending.
pub const MAX_BACKLOG: Duration = Duration::from_secs(10);

#[derive(Debug, PartialEq, Eq)]
pub struct Busy;

/// Spaces outbound calls at least `interval` apart. One instance is shared process-wide, matching
/// Nominatim's per-application usage policy. Nothing is reserved until a call fires, so a caller
/// dropped while it waits gives its place back.
pub struct RateLimiter {
    interval: Duration,
    /// One permit, handed out in FIFO order.
    gate: Semaphore,
    last_fire: Mutex<Option<Instant>>,
    waiting: AtomicUsize,
}

struct InQueue<'a>(&'a AtomicUsize);

impl Drop for InQueue<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl RateLimiter {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            gate: Semaphore::new(1),
            last_fire: Mutex::new(None),
            waiting: AtomicUsize::new(0),
        }
    }

    pub async fn acquire(&self) -> Result<(), Busy> {
        let ahead = self.waiting.fetch_add(1, Ordering::SeqCst);
        let _in_queue = InQueue(&self.waiting);

        if self.expected_wait(ahead) > MAX_BACKLOG {
            return Err(Busy);
        }

        let _permit = self.gate.acquire().await.map_err(|_| Busy)?;
        if let Some(last) = self.last_fire() {
            tokio::time::sleep_until(last + self.interval).await;
        }
        *self
            .last_fire
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
        Ok(())
    }

    fn last_fire(&self) -> Option<Instant> {
        *self
            .last_fire
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn expected_wait(&self, ahead: usize) -> Duration {
        let current = self.last_fire().map_or(Duration::ZERO, |last| {
            (last + self.interval).saturating_duration_since(Instant::now())
        });
        let queued = u32::try_from(ahead)
            .ok()
            .and_then(|n| self.interval.checked_mul(n))
            .unwrap_or(Duration::MAX);
        queued.saturating_add(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // The paused clock makes `Instant::now()` and the sleeps below read tokio's timer, so the
    // 50 ms checkpoint and 200 ms slot are exact.
    #[tokio::test(start_paused = true)]
    async fn second_lookup_is_queued_not_dropped_or_fired_early() {
        let limiter = Arc::new(RateLimiter::new(Duration::from_millis(200)));
        let upstream = Arc::new(AtomicUsize::new(0));
        let start = Instant::now();

        let spawn = || {
            let limiter = limiter.clone();
            let upstream = upstream.clone();
            tokio::spawn(async move {
                limiter
                    .acquire()
                    .await
                    .expect("a second lookup is well inside the backlog");
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

    /// T8
    #[tokio::test(start_paused = true)]
    async fn a_cancelled_waiter_gives_its_slot_back() {
        let interval = Duration::from_millis(200);
        let limiter = Arc::new(RateLimiter::new(interval));
        let start = Instant::now();

        let _ = limiter.acquire().await;

        let cancelled = {
            let limiter = limiter.clone();
            tokio::spawn(async move {
                let _ = limiter.acquire().await;
            })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancelled.abort();
        let _ = cancelled.await;

        let _ = limiter.acquire().await;
        let fired = Instant::now().duration_since(start);

        assert!(
            fired < interval * 2,
            "the next caller waited behind a cancelled one ({fired:?})"
        );
    }

    /// T9
    #[tokio::test(start_paused = true)]
    async fn a_caller_past_the_backlog_is_answered_at_once() {
        let limiter = Arc::new(RateLimiter::new(Duration::from_secs(1)));
        let start = Instant::now();

        let mut callers: Vec<_> = (0..12)
            .map(|_| {
                let limiter = limiter.clone();
                tokio::spawn(async move {
                    let _ = limiter.acquire().await;
                    Instant::now()
                })
            })
            .collect();

        let last = callers.pop().expect("twelve callers");
        let answered = last.await.unwrap().duration_since(start);
        for caller in callers {
            caller.abort();
        }

        assert!(
            answered < Duration::from_millis(50),
            "the twelfth caller queued for {answered:?} instead of being refused"
        );
    }

    /// T9b
    #[tokio::test(start_paused = true)]
    async fn the_backlog_bound_is_busy_and_the_callers_inside_it_still_run() {
        let limiter = Arc::new(RateLimiter::new(Duration::from_secs(1)));

        let mut callers: Vec<_> = (0..12)
            .map(|_| {
                let limiter = limiter.clone();
                tokio::spawn(async move { limiter.acquire().await })
            })
            .collect();

        let last = callers.pop().expect("twelve callers");
        assert_eq!(last.await.unwrap(), Err(Busy));

        let within = callers.pop().expect("the eleventh caller");
        assert_eq!(
            within.await.unwrap(),
            Ok(()),
            "a caller about ten seconds back must still be served"
        );
        for caller in callers {
            caller.abort();
        }
    }
}
