//! Deserialization helpers for constraints serde's derive cannot state.

use serde::{de::Error, Deserialize, Deserializer};
use serde_json::Value;

/// An optional open value that keeps an explicit `null` as `Some(Value::Null)`,
/// so it serializes back as sent rather than disappearing. Pair it with
/// `#[serde(default)]`.
pub(crate) fn keep_null<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

/// A list the schema requires to hold at least one item (`minItems: 1`).
pub(crate) fn non_empty<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let items = Vec::<T>::deserialize(d)?;
    if items.is_empty() {
        return Err(D::Error::invalid_length(0, &"at least one item"));
    }
    Ok(items)
}
