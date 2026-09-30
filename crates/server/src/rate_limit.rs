//! In-process rate limiting for the auth routes (A2a / SPEC Part 1.5).
//!
//! A token bucket keyed by `(route class, client IP)`. Deliberately in-process and
//! deliberately not exact: the goal is to make online password guessing and mail-flooding
//! expensive, not to build a distributed quota system. A single-node deployment is what Komun
//! targets; behind several nodes each keeps its own buckets and the effective limit multiplies by
//! the node count, which is a documented trade, not a bug.
//!
//! Client IP comes from the socket unless the peer is a configured trusted proxy. Honouring
//! `X-Forwarded-For` from anyone would let an attacker reset their own limit by inventing a header
//! — which is strictly worse than having no limiter at all, because it looks like one.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// What a bucket permits: `capacity` requests in a burst, refilling over `per`.
#[derive(Debug, Clone, Copy)]
pub struct Quota {
    pub capacity: u32,
    pub per: Duration,
}

impl Quota {
    pub const fn new(capacity: u32, per: Duration) -> Self {
        Self { capacity, per }
    }

    fn refill_per_second(&self) -> f64 {
        if self.per.is_zero() {
            return f64::INFINITY;
        }
        f64::from(self.capacity) / self.per.as_secs_f64()
    }
}

/// The route classes that are limited, each with its own bucket namespace so that a burst of
/// sign-in attempts cannot exhaust a user's ability to request a password reset (or vice versa).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RouteClass {
    /// Password sign-in: the one an attacker actually grinds.
    SignIn,
    /// Account creation.
    SignUp,
    /// Resending a verification mail — limited hard because each hit sends email to a third party.
    VerifyResend,
    /// Requesting a password-reset mail. Same reasoning as above.
    PasswordReset,
}

impl RouteClass {
    /// The default quota for each class. Sign-in is the loosest of the four because a legitimate
    /// household or office behind one NAT address shares a bucket; the mail-sending routes are the
    /// tightest because the cost of abuse lands on someone who did not ask for it.
    pub const fn quota(self) -> Quota {
        match self {
            RouteClass::SignIn => Quota::new(10, Duration::from_secs(300)),
            RouteClass::SignUp => Quota::new(5, Duration::from_secs(3600)),
            RouteClass::VerifyResend => Quota::new(3, Duration::from_secs(3600)),
            RouteClass::PasswordReset => Quota::new(3, Duration::from_secs(3600)),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            RouteClass::SignIn => "sign_in",
            RouteClass::SignUp => "sign_up",
            RouteClass::VerifyResend => "verify_resend",
            RouteClass::PasswordReset => "password_reset",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

/// The limiter itself. Cheap to clone-by-reference through `Arc` in `AppState`.
#[derive(Debug)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<(RouteClass, IpAddr), Bucket>>,
    /// Sweep threshold: once the map passes this many keys, drop the full buckets. Without it a
    /// long-running process accumulates one entry per IP that has ever knocked, which is itself a
    /// memory-exhaustion vector.
    sweep_at: usize,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            sweep_at: 10_000,
        }
    }

    /// Take one token. `Ok(())` means proceed; `Err(retry_after)` means reject with 429 and that
    /// `Retry-After`.
    pub fn check(&self, class: RouteClass, ip: IpAddr) -> Result<(), Duration> {
        self.check_at(class, ip, Instant::now())
    }

    /// Testable core: the clock is a parameter so the tests do not have to sleep.
    fn check_at(&self, class: RouteClass, ip: IpAddr, now: Instant) -> Result<(), Duration> {
        let quota = class.quota();
        let rate = quota.refill_per_second();

        let mut buckets = match self.buckets.lock() {
            Ok(g) => g,
            // A poisoned mutex means some other thread panicked mid-update. Failing closed here
            // would lock every user out of sign-in over an unrelated bug, so recover the guard.
            Err(poisoned) => poisoned.into_inner(),
        };

        if buckets.len() >= self.sweep_at {
            buckets.retain(|(c, _), b| {
                let refilled = b.tokens
                    + now.saturating_duration_since(b.last_refill).as_secs_f64()
                        * c.quota().refill_per_second();
                refilled < f64::from(c.quota().capacity)
            });
        }

        let bucket = buckets.entry((class, ip)).or_insert(Bucket {
            tokens: f64::from(quota.capacity),
            last_refill: now,
        });

        let elapsed = now
            .saturating_duration_since(bucket.last_refill)
            .as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * rate).min(f64::from(quota.capacity));
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            let deficit = 1.0 - bucket.tokens;
            let seconds = if rate > 0.0 { deficit / rate } else { f64::MAX };
            Err(Duration::from_secs_f64(seconds.clamp(1.0, 86_400.0)))
        }
    }

    /// Hand back a token after an operation that turned out not to be chargeable (for example a
    /// resend that was refused for a reason unrelated to abuse).
    pub fn refund(&self, class: RouteClass, ip: IpAddr) {
        let mut buckets = match self.buckets.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(bucket) = buckets.get_mut(&(class, ip)) {
            bucket.tokens = (bucket.tokens + 1.0).min(f64::from(class.quota().capacity));
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.buckets.lock().map(|b| b.len()).unwrap_or(0)
    }
}

/// Resolve the client IP for limiting purposes.
///
/// `socket_ip` is the peer that actually opened the connection. `forwarded_for` is the raw
/// `X-Forwarded-For` header, which is consulted **only** when the socket peer is in
/// `trusted_proxies`. The value taken is the right-most entry that is not itself a trusted proxy:
/// entries to the left of that are attacker-supplied and must not be believed.
pub fn client_ip(
    socket_ip: IpAddr,
    forwarded_for: Option<&str>,
    trusted_proxies: &[IpAddr],
) -> IpAddr {
    if !trusted_proxies.contains(&socket_ip) {
        return socket_ip;
    }
    let Some(header) = forwarded_for else {
        return socket_ip;
    };

    header
        .split(',')
        .rev()
        .filter_map(|hop| hop.trim().parse::<IpAddr>().ok())
        .find(|ip| !trusted_proxies.contains(ip))
        .unwrap_or(socket_ip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ip(last: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(203, 0, 113, last))
    }

    #[test]
    fn burst_is_allowed_then_refused() {
        let limiter = RateLimiter::new();
        let who = ip(1);
        for i in 0..RouteClass::SignIn.quota().capacity {
            assert!(
                limiter.check(RouteClass::SignIn, who).is_ok(),
                "attempt {i} within the burst must pass"
            );
        }
        let retry = limiter
            .check(RouteClass::SignIn, who)
            .expect_err("the attempt past the burst must be refused");
        assert!(
            retry.as_secs() >= 1,
            "Retry-After must be usable: {retry:?}"
        );
    }

    #[test]
    fn buckets_refill_over_time() {
        let limiter = RateLimiter::new();
        let who = ip(2);
        let t0 = Instant::now();
        for _ in 0..RouteClass::SignIn.quota().capacity {
            limiter
                .check_at(RouteClass::SignIn, who, t0)
                .expect("burst");
        }
        assert!(limiter.check_at(RouteClass::SignIn, who, t0).is_err());

        // 10 per 300s = one token per 30s.
        let later = t0 + Duration::from_secs(31);
        assert!(
            limiter.check_at(RouteClass::SignIn, who, later).is_ok(),
            "a bucket must refill"
        );
        assert!(
            limiter.check_at(RouteClass::SignIn, who, later).is_err(),
            "but only by what actually elapsed"
        );
    }

    #[test]
    fn classes_and_addresses_do_not_share_buckets() {
        let limiter = RateLimiter::new();
        let attacker = ip(3);
        for _ in 0..RouteClass::SignIn.quota().capacity {
            limiter.check(RouteClass::SignIn, attacker).expect("burst");
        }
        assert!(limiter.check(RouteClass::SignIn, attacker).is_err());

        // Same address, different route: unaffected.
        assert!(limiter.check(RouteClass::PasswordReset, attacker).is_ok());
        // Different address, same route: unaffected. One noisy IP must not lock out the world.
        assert!(limiter.check(RouteClass::SignIn, ip(4)).is_ok());
    }

    #[test]
    fn mail_sending_routes_are_the_tightest() {
        for class in [RouteClass::VerifyResend, RouteClass::PasswordReset] {
            let limiter = RateLimiter::new();
            let who = ip(5);
            for _ in 0..3 {
                limiter.check(class, who).expect("three per hour");
            }
            assert!(
                limiter.check(class, who).is_err(),
                "{} must not send a fourth mail in an hour",
                class.as_str()
            );
        }
    }

    #[test]
    fn refund_returns_a_token() {
        let limiter = RateLimiter::new();
        let who = ip(6);
        for _ in 0..RouteClass::VerifyResend.quota().capacity {
            limiter.check(RouteClass::VerifyResend, who).expect("burst");
        }
        assert!(limiter.check(RouteClass::VerifyResend, who).is_err());
        limiter.refund(RouteClass::VerifyResend, who);
        assert!(limiter.check(RouteClass::VerifyResend, who).is_ok());
    }

    #[test]
    fn forwarded_for_is_ignored_from_an_untrusted_peer() {
        let attacker = ip(10);
        // No trusted proxies configured: the header is a lie and must be discarded, otherwise the
        // attacker resets their own bucket on every request.
        assert_eq!(client_ip(attacker, Some("198.51.100.7"), &[]), attacker);
        assert_eq!(
            client_ip(attacker, Some("198.51.100.7"), &[ip(99)]),
            attacker,
            "trusted list that does not contain the peer changes nothing"
        );
    }

    #[test]
    fn forwarded_for_is_honoured_from_a_trusted_proxy() {
        let proxy = ip(20);
        let real = "198.51.100.7".parse::<IpAddr>().unwrap();
        assert_eq!(client_ip(proxy, Some("198.51.100.7"), &[proxy]), real);

        // Attacker prepends a fake hop; the right-most untrusted entry still wins.
        assert_eq!(
            client_ip(proxy, Some("1.2.3.4, 198.51.100.7"), &[proxy]),
            real
        );

        // Chained trusted proxies are skipped over.
        let inner = ip(21);
        assert_eq!(
            client_ip(
                proxy,
                Some(&format!("198.51.100.7, {inner}")),
                &[proxy, inner]
            ),
            real
        );

        // Garbage header falls back to the socket peer rather than failing open.
        assert_eq!(client_ip(proxy, Some("not-an-ip"), &[proxy]), proxy);
        assert_eq!(client_ip(proxy, None, &[proxy]), proxy);
    }

    #[test]
    fn full_buckets_are_swept() {
        let mut limiter = RateLimiter::new();
        limiter.sweep_at = 4;
        let t0 = Instant::now();
        for i in 0..4u8 {
            limiter
                .check_at(RouteClass::SignIn, ip(i), t0)
                .expect("first hit");
        }
        assert_eq!(limiter.len(), 4);
        // An hour later every one of those buckets is full again, so the sweep discards them
        // instead of growing the map without bound.
        limiter
            .check_at(RouteClass::SignIn, ip(200), t0 + Duration::from_secs(3600))
            .expect("hit after the sweep");
        assert_eq!(limiter.len(), 1, "stale full buckets must be reclaimed");
    }
}
