//! JSON Patch (RFC 6902) operations, as `STATE_DELTA` and `ACTIVITY_DELTA`
//! carry them.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A list of JSON Patch operations, applied in order.
pub type JsonPatch = Vec<JsonPatchOperation>;

/// One JSON Patch operation, discriminated by `op`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum JsonPatchOperation {
    Add {
        path: JsonPointer,
        value: Value,
    },
    Remove {
        path: JsonPointer,
    },
    Replace {
        path: JsonPointer,
        value: Value,
    },
    Move {
        from: JsonPointer,
        path: JsonPointer,
    },
    Copy {
        from: JsonPointer,
        path: JsonPointer,
    },
    Test {
        path: JsonPointer,
        value: Value,
    },
}

/// An RFC 6901 JSON Pointer: empty, or `/`-separated tokens in which `~` is
/// only ever followed by `0` or `1`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct JsonPointer(String);

impl JsonPointer {
    /// The pointer as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for JsonPointer {
    type Error = InvalidJsonPointer;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        let mut chars = s.chars();
        let leading_slash = s.is_empty() || s.starts_with('/');
        let mut escapes_valid = true;
        while let Some(c) = chars.next() {
            if c == '~' && !matches!(chars.next(), Some('0' | '1')) {
                escapes_valid = false;
                break;
            }
        }
        if !(leading_slash && escapes_valid) {
            return Err(InvalidJsonPointer(s));
        }
        Ok(Self(s))
    }
}

impl TryFrom<&str> for JsonPointer {
    type Error = InvalidJsonPointer;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::try_from(s.to_owned())
    }
}

impl From<JsonPointer> for String {
    fn from(p: JsonPointer) -> Self {
        p.0
    }
}

impl std::fmt::Display for JsonPointer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A string that is not a valid [`JsonPointer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidJsonPointer(pub String);

impl std::fmt::Display for InvalidJsonPointer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid JSON pointer {:?}", self.0)
    }
}

impl std::error::Error for InvalidJsonPointer {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_pointer_follows_rfc_6901() {
        for ok in ["", "/", "/a/b", "/a~0b/c~1d", "//"] {
            assert!(JsonPointer::try_from(ok).is_ok(), "{ok:?}");
        }
        for bad in ["a", "a/b", "/a~", "/a~2", "~0"] {
            assert!(JsonPointer::try_from(bad).is_err(), "{bad:?}");
        }
    }
}
