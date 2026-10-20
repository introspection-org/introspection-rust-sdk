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

/// Encode `value` for an `application/x-www-form-urlencoded` body.
///
/// Same as [`encode`] except that a space becomes `+`, which is what the form
/// encoding calls for and what a form parser round-trips. `%20` decodes to a
/// space in most parsers too, but only one of the two is the encoding this
/// content type names, and a space is the ordinary case here: `scope` is a
/// space-separated list.
///
/// Rewriting `%20` afterwards is sound because [`encode`] escapes a literal
/// `+` to `%2B` first, so no `+` in the output can be anything but a space.
pub(crate) fn encode_form_component(value: &str) -> String {
    encode(value).replace("%20", "+")
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
    fn form_encoding_uses_plus_for_a_space() {
        use super::encode_form_component;
        // The ordinary case: a space-separated `scope`.
        assert_eq!(encode_form_component("a b"), "a+b");
        // A literal `+` is escaped before the rewrite, so it cannot be
        // mistaken for one.
        assert_eq!(encode_form_component("a+b"), "a%2Bb");
        assert_eq!(encode_form_component("a+b c"), "a%2Bb+c");
        // Everything else matches the URL encoding.
        assert_eq!(encode_form_component("a&b=c"), "a%26b%3Dc");
        assert_eq!(encode_form_component("é"), "%C3%A9");
    }

    #[test]
    fn escapes_multibyte_input_per_utf8_byte() {
        assert_eq!(encode("é"), "%C3%A9");
    }
}
