//! Shared retry/backoff primitives.
//!
//! Both the unary REST retry path ([`crate::api::http`]) and the resumable
//! run-stream ([`crate::api::resumable`]) back off the same way — a
//! `Retry-After` floor plus capped exponential full jitter — so the
//! math, the cap, and the header parsing live here once rather than being copied
//! into each. The *retry decision* (which statuses, which methods, readiness vs
//! severance) stays in each caller, since those differ.

use std::time::Duration;

use reqwest::header::HeaderMap;

/// Cap on any single backoff step.
pub(crate) const MAX_BACKOFF: Duration = Duration::from_secs(10);

/// The server's `retry_after` minimum plus capped exponential full jitter.
pub(crate) fn backoff_delay(
    attempt: u32,
    base: Duration,
    retry_after: Option<Duration>,
) -> Duration {
    backoff_delay_with_jitter(attempt, base, retry_after, fastrand::f64())
}

fn backoff_delay_with_jitter(
    attempt: u32,
    base: Duration,
    retry_after: Option<Duration>,
    random: f64,
) -> Duration {
    let factor = 1u64.checked_shl(attempt.min(20)).unwrap_or(u64::MAX);
    let exp =
        Duration::from_millis((base.as_millis() as u64).saturating_mul(factor)).min(MAX_BACKOFF);
    let floor = retry_after.unwrap_or_default().min(MAX_BACKOFF);
    let jitter_room = exp.min(MAX_BACKOFF.saturating_sub(floor));
    floor + jitter_room.mul_f64(random.clamp(0.0, 1.0))
}

/// Parse a `Retry-After` response header as a delay. Only the delta-seconds
/// form is honoured (what the DP emits); an HTTP-date value is ignored.
pub(crate) fn retry_after_from(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<f64>().ok())
        .filter(|secs| secs.is_finite() && *secs >= 0.0)
        .map(Duration::from_secs_f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_without_retry_after() {
        let base = Duration::from_millis(500);
        assert_eq!(
            backoff_delay_with_jitter(0, base, None, 0.0),
            Duration::ZERO
        );
        assert_eq!(
            backoff_delay_with_jitter(1, base, None, 0.5),
            Duration::from_millis(500)
        );
        assert_eq!(
            backoff_delay_with_jitter(2, base, None, 1.0),
            Duration::from_millis(2000)
        );
    }

    #[test]
    fn caps_at_max_backoff() {
        let base = Duration::from_secs(1);
        // 2^20 * 1s would overflow the cap many times over.
        assert_eq!(backoff_delay_with_jitter(20, base, None, 1.0), MAX_BACKOFF);
    }

    #[test]
    fn retry_after_is_a_floor_below_jitter() {
        let base = Duration::from_millis(500);
        // Retry-After above the exponential step wins.
        assert_eq!(
            backoff_delay_with_jitter(0, base, Some(Duration::from_secs(2)), 0.0),
            Duration::from_secs(2)
        );
        assert_eq!(
            backoff_delay_with_jitter(1, base, Some(Duration::from_secs(1)), 0.5),
            Duration::from_millis(1500)
        );
        assert_eq!(
            backoff_delay_with_jitter(4, base, Some(Duration::from_secs(9)), 1.0),
            MAX_BACKOFF
        );
    }

    #[test]
    fn parses_delta_seconds_retry_after() {
        let mut h = HeaderMap::new();
        h.insert(reqwest::header::RETRY_AFTER, "2".parse().unwrap());
        assert_eq!(retry_after_from(&h), Some(Duration::from_secs(2)));
    }

    #[test]
    fn ignores_absent_or_non_numeric_retry_after() {
        assert_eq!(retry_after_from(&HeaderMap::new()), None);
        let mut h = HeaderMap::new();
        // HTTP-date form is not honoured.
        h.insert(
            reqwest::header::RETRY_AFTER,
            "Wed, 21 Oct 2026 07:28:00 GMT".parse().unwrap(),
        );
        assert_eq!(retry_after_from(&h), None);
    }
}
