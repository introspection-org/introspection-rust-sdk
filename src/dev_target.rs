//! Development-target resolution — which `introspection dev` server this
//! process's tasks should reach.
//!
//! Two developers can run `introspection dev` against one shared Runtime. A
//! task created by a shared application credential carries no developer, so
//! the platform cannot tell their machines apart; the caller names one
//! instead. `introspection dev` prints the value to set:
//!
//! ```text
//! serving as: roland
//! for your app: INTROSPECTION_DEV_TARGET=roland
//! ```
//!
//! Carried as a request header rather than on the runner's `caller` payload:
//! `caller` is descriptive metadata the platform never acts on, and a target
//! is a per-request selector the platform does act on. Keeping them apart is
//! what lets `caller` stay a free-form bag, and the header is the only
//! transport that reaches a bare `POST /v1/tasks` with a dev API key, whose
//! JWT is minted from the key row with no per-request input path.
//!
//! Deliberately env-only, with no local-username fallback. Defaulting to the
//! login name would be zero-config on a laptop and wrong everywhere else: a
//! process running in a shared development deployment under an account like
//! `app` would silently name a machine nobody is serving and fail closed,
//! where today it reaches the one connected dev server. The CLI defaults to
//! the username because it is naming *itself* and always runs on the
//! developer's machine; this names *someone else's* machine and can run
//! anywhere.
//!
//! Inert outside development: the Data Plane consults a target only on the
//! development pin path, so a stray value in staging or production is ignored.

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

/// Everything outside RFC 3986's *unreserved* set.
///
/// `NON_ALPHANUMERIC` alone would encode `-`, `.`, `_` and `~`, turning an
/// everyday `my-laptop` into `my%2Dlaptop`. The Data Plane decodes before it
/// normalizes, so that still routes — but it is not what the other SDKs send,
/// not what `introspection dev` prints for you to copy, and not what anyone
/// reading a header or a log expects to see.
const TARGET_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');
use std::collections::HashMap;
use std::env;

/// Header carrying the target on requests that have no runner to ride.
pub const DEV_TARGET_HEADER: &str = "x-introspection-dev-target";

/// The env var `introspection dev` prints, read by both the CLI and this SDK.
pub const DEV_TARGET_ENV: &str = "INTROSPECTION_DEV_TARGET";

/// The development target for this process, or `None` when unset or blank.
///
/// Percent-encoded, because the value becomes an HTTP header and a header is
/// bytes: `HeaderValue` rejects a non-ASCII login name like `andré` outright,
/// so encoding is what lets one route rather than fail the request. An
/// ordinary ASCII name encodes to itself.
///
/// Safe to send encoded because the Data Plane decodes before it normalizes,
/// so `andré` and `andr%C3%A9` land on the same target as the `--as andré`
/// the CLI advertises over protobuf, where no encoding is needed.
pub fn resolve_dev_target() -> Option<String> {
    let raw = env::var(DEV_TARGET_ENV).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(utf8_percent_encode(trimmed, TARGET_ENCODE_SET).to_string())
    }
}

/// `headers` with the development target merged in.
///
/// Merged *under* the caller's own entries, so an explicitly configured
/// `x-introspection-dev-target` still wins. A client that never opts in
/// carries nothing new.
pub fn with_dev_target(mut headers: HashMap<String, String>) -> HashMap<String, String> {
    if let Some(target) = resolve_dev_target() {
        headers
            .entry(DEV_TARGET_HEADER.to_string())
            .or_insert(target);
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `std::env` is process-global, so these run under one lock and restore
    /// what they found rather than assuming the variable started unset.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_env<T>(value: Option<&str>, body: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = env::var(DEV_TARGET_ENV).ok();
        match value {
            Some(v) => unsafe { env::set_var(DEV_TARGET_ENV, v) },
            None => unsafe { env::remove_var(DEV_TARGET_ENV) },
        }
        let out = body();
        match previous {
            Some(v) => unsafe { env::set_var(DEV_TARGET_ENV, v) },
            None => unsafe { env::remove_var(DEV_TARGET_ENV) },
        }
        out
    }

    #[test]
    fn resolves_from_the_environment() {
        with_env(Some("roland"), || {
            assert_eq!(resolve_dev_target().as_deref(), Some("roland"));
        });
    }

    #[test]
    fn is_trimmed_and_blank_is_the_same_as_unset() {
        with_env(Some("  roland  "), || {
            assert_eq!(resolve_dev_target().as_deref(), Some("roland"));
        });
        with_env(Some("   "), || assert!(resolve_dev_target().is_none()));
        with_env(None, || assert!(resolve_dev_target().is_none()));
    }

    #[test]
    fn ordinary_machine_names_survive_encoding_intact() {
        // The three SDKs must put the same bytes on the wire for the same
        // target: a hyphenated hostname is the common case, and encoding it
        // differently from the JS and Python clients would show up in every
        // header and log line even though routing tolerates it.
        for name in ["my-laptop", "roland_box", "host.local", "a~b"] {
            with_env(Some(name), || {
                assert_eq!(resolve_dev_target().as_deref(), Some(name));
            });
        }
    }

    #[test]
    fn non_ascii_and_spaced_targets_are_percent_encoded() {
        // A HeaderValue cannot carry these bytes; the Data Plane decodes
        // before it normalizes, so the encoded form matches what the CLI
        // advertises unencoded over protobuf.
        with_env(Some("andré"), || {
            assert_eq!(resolve_dev_target().as_deref(), Some("andr%C3%A9"));
        });
        with_env(Some("roland laptop"), || {
            assert_eq!(resolve_dev_target().as_deref(), Some("roland%20laptop"));
        });
        // The ordinary case is untouched.
        with_env(Some("roland"), || {
            assert_eq!(resolve_dev_target().as_deref(), Some("roland"));
        });
    }

    #[test]
    fn unset_leaves_headers_untouched() {
        with_env(None, || {
            assert!(with_dev_target(HashMap::new()).is_empty());
        });
    }

    #[test]
    fn merges_under_caller_supplied_headers() {
        with_env(Some("roland"), || {
            let merged = with_dev_target(HashMap::new());
            assert_eq!(
                merged.get(DEV_TARGET_HEADER).map(String::as_str),
                Some("roland")
            );

            // An explicitly configured header is more specific than an env var.
            let mut explicit = HashMap::new();
            explicit.insert(DEV_TARGET_HEADER.to_string(), "explicit".to_string());
            let merged = with_dev_target(explicit);
            assert_eq!(
                merged.get(DEV_TARGET_HEADER).map(String::as_str),
                Some("explicit")
            );
        });
    }
}
