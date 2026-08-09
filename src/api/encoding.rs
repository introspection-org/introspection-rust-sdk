//! RFC 3986 percent encoding for values interpolated into URLs.
//!
//! Every path segment, query value, and header value the SDK builds from
//! caller-supplied text goes through here. Keeping one definition is what
//! stops a call site from picking a laxer set than its neighbour: eight
//! hand-rolled copies of this used to exist, and a divergence in any one of
//! them lets a slug carrying `/`, `&`, or `#` rewrite the URL it rides in.

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

/// Everything outside RFC 3986's *unreserved* set (`ALPHA / DIGIT / - . _ ~`).
///
/// Safe for a path segment, a query value, and a header value alike: it is
/// the strictest of the three, so one set covers all of them.
pub(crate) const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Percent-encode `value` down to the RFC 3986 unreserved set.
pub(crate) fn encode(value: &str) -> String {
    utf8_percent_encode(value, UNRESERVED).to_string()
}

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn passes_unreserved_characters_through() {
        assert_eq!(encode("abcXYZ019-._~"), "abcXYZ019-._~");
    }

    #[test]
    fn escapes_the_characters_that_would_rewrite_a_url() {
        assert_eq!(encode("a/b"), "a%2Fb");
        assert_eq!(encode("a b"), "a%20b");
        assert_eq!(encode("a&b=c"), "a%26b%3Dc");
        assert_eq!(encode("a#b?c"), "a%23b%3Fc");
    }

    #[test]
    fn escapes_multibyte_input_per_utf8_byte() {
        assert_eq!(encode("é"), "%C3%A9");
    }
}
