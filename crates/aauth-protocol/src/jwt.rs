//! Compact JWS: minting with a [`SigningKey`] and verifying against a [`Jwk`],
//! under fully-specified algorithms only.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::jwk::{Algorithm, Jwk};
use crate::keys::SigningKey;

/// A JWT split into its decoded header and claims. Nothing here is verified.
#[derive(Clone, Debug)]
pub struct Decoded {
    /// The JOSE header.
    pub header: Map<String, Value>,
    /// The claims.
    pub claims: Map<String, Value>,
    signing_input: String,
    signature: Vec<u8>,
}

impl Decoded {
    /// The `typ` header.
    pub fn typ(&self) -> Option<&str> {
        self.header.get("typ").and_then(Value::as_str)
    }

    /// The `alg` header.
    pub fn alg(&self) -> Option<&str> {
        self.header.get("alg").and_then(Value::as_str)
    }

    /// The `kid` header.
    pub fn kid(&self) -> Option<&str> {
        self.header.get("kid").and_then(Value::as_str)
    }

    /// A non-empty string claim.
    pub fn claim_str(&self, name: &str) -> Option<&str> {
        self.claims
            .get(name)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    }

    /// Whether `key` signed this JWT under `algorithm`.
    pub fn verify_signature(&self, key: &Jwk, algorithm: Algorithm) -> bool {
        key.verify(algorithm, self.signing_input.as_bytes(), &self.signature)
    }
}

fn decode_part(part: &str) -> Result<Map<String, Value>> {
    let bytes = URL_SAFE_NO_PAD
        .decode(part.trim_end_matches('='))
        .map_err(|_| Error::Invalid("JWT part is not base64url".into()))?;
    match serde_json::from_slice(&bytes) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(Error::Invalid("JWT part is not a JSON object".into())),
    }
}

/// Split a compact JWS without verifying it.
pub fn decode(jwt: &str) -> Result<Decoded> {
    let mut parts = jwt.split('.');
    let (Some(header), Some(claims), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Error::Invalid("not a compact JWS".into()));
    };
    Ok(Decoded {
        header: decode_part(header)?,
        claims: decode_part(claims)?,
        signing_input: format!("{header}.{claims}"),
        signature: URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| Error::Invalid("JWT signature is not base64url".into()))?,
    })
}

/// Mint a JWT: `header` gains `alg`, and `kid` when the key's thumbprint is
/// wanted and none is given.
pub fn encode(key: &SigningKey, mut header: Map<String, Value>, claims: &Value) -> String {
    header.insert("alg".into(), Value::from(key.algorithm().as_str()));
    let input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(Value::Object(header).to_string()),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    );
    let signature = key.sign(input.as_bytes());
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature))
}

/// Seconds until a JWT's `exp`, less `margin`; `None` when unreadable or due.
/// For caches deciding how long to keep a token they did not mint.
pub fn remaining_secs(jwt: &str, now: i64, margin: i64) -> Option<u64> {
    let payload = jwt.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let exp = serde_json::from_slice::<Value>(&bytes)
        .ok()?
        .get("exp")?
        .as_i64()?;
    let left = exp - now - margin;
    (left > 0).then_some(left as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_minted_jwt_verifies_and_a_tampered_one_does_not() {
        let key = SigningKey::generate();
        let header = json!({"typ": "aa-agent+jwt"}).as_object().unwrap().clone();
        let jwt = encode(&key, header, &json!({"sub": "a"}));
        let decoded = decode(&jwt).unwrap();
        assert_eq!(decoded.alg(), Some("Ed25519"));
        assert!(decoded.verify_signature(&key.public_jwk(), Algorithm::Ed25519));

        let mut parts: Vec<&str> = jwt.split('.').collect();
        let forged = URL_SAFE_NO_PAD.encode(json!({"sub": "b"}).to_string());
        parts[1] = &forged;
        let tampered = decode(&parts.join(".")).unwrap();
        assert!(!tampered.verify_signature(&key.public_jwk(), Algorithm::Ed25519));
    }

    #[test]
    fn a_token_is_kept_until_shortly_before_it_expires() {
        let enc = |v: Value| URL_SAFE_NO_PAD.encode(v.to_string());
        let jwt = |exp: i64| {
            format!(
                "{}.{}.sig",
                enc(json!({"alg": "ES256"})),
                enc(json!({"exp": exp}))
            )
        };
        assert_eq!(remaining_secs(&jwt(1_100), 1_000, 30), Some(70));
        assert_eq!(remaining_secs(&jwt(1_020), 1_000, 30), None);
        assert_eq!(remaining_secs("garbage", 1_000, 30), None);
    }
}
