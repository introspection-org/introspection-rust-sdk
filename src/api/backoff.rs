//! Shared retry/backoff primitives.
//!
//! Both the unary REST retry path ([`crate::api::http`]) and the resumable
//! run-stream ([`crate::api::resumable`]) back off the same way — a
//! capped-exponential delay with the server's `Retry-After` as a floor — so the
//! math, the cap, and the header parsing live here once rather than being copied
//! into each. The *retry decision* (which statuses, which methods, readiness vs
//! severance) stays in each caller, since those differ.

use std::time::Duration;

use reqwest::header::HeaderMap;

/// Cap on any single backoff step.
pub(crate) const MAX_BACKOFF: Duration = Duration::from_secs(10);

/// Capped-exponential backoff: `base * 2^attempt`, clamped to [`MAX_BACKOFF`],
/// with `retry_after` used as the floor when present.
pub(crate) fn backoff_delay(
    attempt: u32,
    base: Duration,
    retry_after: Option<Duration>,
) -> Duration {
    let factor = 1u64.checked_shl(attempt.min(20)).unwrap_or(u64::MAX);
    let exp =
        Duration::from_millis((base.as_millis() as u64).saturating_mul(factor)).min(MAX_BACKOFF);
    retry_after.map(|ra| ra.max(exp)).unwrap_or(exp)
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
        assert_eq!(backoff_delay(0, base, None), Duration::from_millis(500));
        assert_eq!(backoff_delay(1, base, None), Duration::from_millis(1000));
        assert_eq!(backoff_delay(2, base, None), Duration::from_millis(2000));
    }

    #[test]
    fn caps_at_max_backoff() {
        let base = Duration::from_secs(1);
        // 2^20 * 1s would overflow the cap many times over.
        assert_eq!(backoff_delay(20, base, None), MAX_BACKOFF);
    }

    #[test]
    fn retry_after_is_a_floor_not_a_ceiling() {
        let base = Duration::from_millis(500);
        // Retry-After above the exponential step wins.
        assert_eq!(
            backoff_delay(0, base, Some(Duration::from_secs(2))),
            Duration::from_secs(2)
        );
        // Retry-After below the exponential step is ignored (floor only).
        assert_eq!(
            backoff_delay(3, base, Some(Duration::from_millis(100))),
            Duration::from_millis(4000)
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
