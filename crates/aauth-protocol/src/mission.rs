//! Missions (AAuth §Mission): what a person approved an agent to do. A
//! mission is named everywhere afterwards by its `s256`, the hash of its
//! exact bytes, so nobody can change what was approved without changing its
//! name.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A mission as a Person Server approved it, checked against its hash.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApprovedMission {
    /// `BASE64URL(SHA-256(mission bytes))`: the `mission_s256` later tokens carry.
    pub s256: String,
    /// The decoded mission document.
    pub mission: Value,
}

impl ApprovedMission {
    /// The mission an approval body carries (`s256`, and `mission` as
    /// base64url bytes), when its bytes hash to the `s256` it names.
    ///
    /// ```
    /// # use aauth_protocol::mission::ApprovedMission;
    /// use base64::Engine;
    /// let bytes = br#"{"description":"Sydney","approved_resources":["https://api.booking.example"]}"#;
    /// let body = serde_json::json!({
    ///     "s256": aauth_protocol::s256(bytes),
    ///     "mission": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes),
    /// });
    /// let mission = ApprovedMission::from_approval(&body).unwrap();
    /// assert_eq!(mission.approved_resources(), ["https://api.booking.example"]);
    /// ```
    pub fn from_approval(body: &Value) -> Option<Self> {
        let s256 = body.get("s256")?.as_str()?;
        let bytes = URL_SAFE_NO_PAD
            .decode(body.get("mission")?.as_str()?.trim_end_matches('='))
            .ok()?;
        (crate::s256(&bytes) == s256).then(|| Self {
            s256: s256.to_string(),
            mission: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        })
    }

    /// The resources the mission was approved for.
    pub fn approved_resources(&self) -> Vec<String> {
        self.mission
            .get("approved_resources")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|r| r.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_mission_whose_bytes_do_not_match_its_hash_is_refused() {
        let bytes = br#"{"agent":"a"}"#;
        let granted =
            json!({ "s256": crate::s256(bytes), "mission": URL_SAFE_NO_PAD.encode(bytes) });
        assert!(ApprovedMission::from_approval(&granted).is_some());
        let mut tampered = granted.clone();
        tampered["s256"] = json!("not-the-hash");
        assert!(ApprovedMission::from_approval(&tampered).is_none());
    }
}
