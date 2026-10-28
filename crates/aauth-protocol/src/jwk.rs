//! Public JWKs: RFC 7638 thumbprints, algorithm determination by the `alg`
//! member alone (draft-hardt-httpbis-signature-key §Algorithm Determination,
//! RFC 9864), and signature verification for the algorithms AAuth uses.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// A public JWK. Private members are never read or written.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Jwk {
    /// Key type: `OKP`, `EC` or `RSA`.
    pub kty: String,
    /// Curve: `Ed25519` or `P-256`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crv: Option<String>,
    /// The public key (`OKP`), or its x coordinate (`EC`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    /// An `EC` key's y coordinate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
    /// An `RSA` key's modulus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n: Option<String>,
    /// An `RSA` key's exponent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub e: Option<String>,
    /// The fully-specified algorithm the key signs with. Required to verify.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alg: Option<String>,
    /// The key's identifier in its JWKS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,
    /// `sig` for a signing key.
    #[serde(default, rename = "use", skip_serializing_if = "Option::is_none")]
    pub use_: Option<String>,
}

/// A JWK Set.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Jwks {
    /// The keys.
    pub keys: Vec<Jwk>,
}

/// The fully-specified signature algorithms this crate signs and verifies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Algorithm {
    /// EdDSA over Ed25519 (RFC 9864's `Ed25519`): what this crate signs with.
    Ed25519,
    /// ECDSA over P-256 with SHA-256.
    Es256,
}

impl Algorithm {
    /// Its JOSE name.
    pub fn as_str(self) -> &'static str {
        match self {
            Algorithm::Ed25519 => "Ed25519",
            Algorithm::Es256 => "ES256",
        }
    }

    /// The algorithm a JOSE name names, among those supported.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Ed25519" => Some(Algorithm::Ed25519),
            "ES256" => Some(Algorithm::Es256),
            _ => None,
        }
    }
}

/// Why a key cannot be used, by its `Signature-Error` code.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyProblem {
    /// `invalid_key`: malformed, symmetric, or inconsistent with its `alg`.
    Invalid(String),
    /// `unsupported_algorithm`: well-formed, but not one this crate verifies.
    Unsupported(String),
}

impl Jwk {
    /// RFC 7638: the required members, lexicographically ordered, no whitespace.
    pub fn thumbprint(&self) -> Result<String> {
        let member = |v: &Option<String>, name: &str| {
            v.clone().ok_or_else(|| {
                Error::Key(format!("{} key missing required member {name}", self.kty))
            })
        };
        let canonical = match self.kty.as_str() {
            "OKP" => serde_json::json!({
                "crv": member(&self.crv, "crv")?, "kty": "OKP", "x": member(&self.x, "x")?,
            }),
            "EC" => serde_json::json!({
                "crv": member(&self.crv, "crv")?, "kty": "EC",
                "x": member(&self.x, "x")?, "y": member(&self.y, "y")?,
            }),
            "RSA" => serde_json::json!({
                "e": member(&self.e, "e")?, "kty": "RSA", "n": member(&self.n, "n")?,
            }),
            other => return Err(Error::Key(format!("unsupported key type: {other}"))),
        };
        // serde_json's map is ordered by key, which is what RFC 7638 asks for.
        Ok(URL_SAFE_NO_PAD.encode(Sha256::digest(canonical.to_string().as_bytes())))
    }

    /// The algorithm this key signs with, from its `alg` member, checked
    /// against its key material.
    pub fn algorithm(&self) -> Result<Algorithm, KeyProblem> {
        if self.kty.is_empty() {
            return Err(KeyProblem::Invalid(
                "JWK missing required member: kty".into(),
            ));
        }
        if self.kty == "oct" {
            return Err(KeyProblem::Invalid(
                "symmetric keys are not permitted".into(),
            ));
        }
        let Some(alg) = self.alg.as_deref() else {
            return Err(KeyProblem::Invalid(
                "JWK missing required member: alg".into(),
            ));
        };
        if alg == "EdDSA" || alg.starts_with("HS") || alg == "none" {
            return Err(KeyProblem::Invalid(format!(
                "alg {alg:?} is not fully specified or is symmetric"
            )));
        }
        let algorithm = Algorithm::from_name(alg)
            .ok_or_else(|| KeyProblem::Unsupported(format!("unsupported algorithm {alg:?}")))?;
        let (kty, crv, members): (&str, &str, &[&Option<String>]) = match algorithm {
            Algorithm::Ed25519 => ("OKP", "Ed25519", &[&self.x]),
            Algorithm::Es256 => ("EC", "P-256", &[&self.x, &self.y]),
        };
        if self.kty != kty || self.crv.as_deref() != Some(crv) {
            return Err(KeyProblem::Invalid(format!(
                "JWK kty/crv are inconsistent with alg {alg}"
            )));
        }
        if members.iter().any(|m| m.is_none()) {
            return Err(KeyProblem::Invalid("JWK missing key material".into()));
        }
        Ok(algorithm)
    }

    /// Whether `signature` is this key's, under `algorithm`, over `message`.
    /// ES256 signatures are the raw 64-byte `r || s` both JWS and RFC 9421 use.
    pub fn verify(&self, algorithm: Algorithm, message: &[u8], signature: &[u8]) -> bool {
        let decode = |v: &Option<String>| v.as_deref().and_then(|v| URL_SAFE_NO_PAD.decode(v).ok());
        match algorithm {
            Algorithm::Ed25519 => {
                let Some(Ok(x)) = decode(&self.x).map(<[u8; 32]>::try_from) else {
                    return false;
                };
                let Ok(key) = ed25519_dalek::VerifyingKey::from_bytes(&x) else {
                    return false;
                };
                let Ok(signature) = ed25519_dalek::Signature::from_slice(signature) else {
                    return false;
                };
                // Strict: a weak (small-order) key would verify a forged signature.
                key.verify_strict(message, &signature).is_ok()
            }
            Algorithm::Es256 => {
                use p256::ecdsa::signature::Verifier;
                let (Some(x), Some(y)) = (decode(&self.x), decode(&self.y)) else {
                    return false;
                };
                if x.len() != 32 || y.len() != 32 {
                    return false;
                }
                // The uncompressed SEC1 point: 0x04, then x and y.
                let point = [&[0x04][..], &x, &y].concat();
                let Ok(key) = p256::ecdsa::VerifyingKey::from_sec1_bytes(&point) else {
                    return false;
                };
                let Ok(signature) = p256::ecdsa::Signature::from_slice(signature) else {
                    return false;
                };
                key.verify(message, &signature).is_ok()
            }
        }
    }

    /// The structural checks on a token's `cnf.jwk` (AAuth §Request-Context
    /// Binding), before it is decoded.
    pub fn check_confirmation_key(&self) -> Result<()> {
        let required: &[(&str, &Option<String>)] = match self.kty.as_str() {
            "" => {
                return Err(Error::Key(
                    "cnf.jwk is structurally incomplete: missing kty".into(),
                ))
            }
            "oct" => return Err(Error::Key("cnf.jwk uses a symmetric key type".into())),
            "OKP" => &[("crv", &self.crv), ("x", &self.x)],
            "EC" => &[("crv", &self.crv), ("x", &self.x), ("y", &self.y)],
            "RSA" => &[("n", &self.n), ("e", &self.e)],
            other => {
                return Err(Error::Key(format!(
                    "cnf.jwk uses an unsupported key type: {other}"
                )))
            }
        };
        if let Some((name, _)) = required.iter().find(|(_, v)| v.is_none()) {
            return Err(Error::Key(format!(
                "cnf.jwk is structurally incomplete: missing {name}"
            )));
        }
        match self.alg.as_deref() {
            None | Some("") => Err(Error::Key(
                "cnf.jwk: alg is REQUIRED and must be fully specified".into(),
            )),
            Some(alg @ ("none" | "EdDSA" | "HS256" | "HS384" | "HS512")) => {
                Err(Error::Key(format!("cnf.jwk: alg {alg:?} MUST NOT be used")))
            }
            Some("Ed25519") if self.kty == "OKP" && self.crv.as_deref() != Some("Ed25519") => {
                Err(Error::Key("cnf.jwk crv disagrees with alg".into()))
            }
            Some("ES256") if self.kty == "EC" && self.crv.as_deref() != Some("P-256") => {
                Err(Error::Key("cnf.jwk crv disagrees with alg".into()))
            }
            Some(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thumbprint_is_rfc_8037s() {
        // RFC 8037 Appendix A.3.
        let jwk = Jwk {
            kty: "OKP".into(),
            crv: Some("Ed25519".into()),
            x: Some("11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo".into()),
            ..Default::default()
        };
        assert_eq!(
            jwk.thumbprint().unwrap(),
            "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k"
        );
    }

    #[test]
    fn the_algorithm_comes_from_alg_and_must_agree_with_the_key() {
        let mut jwk = Jwk {
            kty: "OKP".into(),
            crv: Some("Ed25519".into()),
            x: Some("x".into()),
            alg: Some("Ed25519".into()),
            ..Default::default()
        };
        assert_eq!(jwk.algorithm(), Ok(Algorithm::Ed25519));
        jwk.alg = Some("EdDSA".into());
        assert!(matches!(jwk.algorithm(), Err(KeyProblem::Invalid(_))));
        jwk.alg = None;
        assert!(matches!(jwk.algorithm(), Err(KeyProblem::Invalid(_))));
        jwk.alg = Some("ES256".into());
        assert!(matches!(jwk.algorithm(), Err(KeyProblem::Invalid(_))));
        jwk.alg = Some("Ed448".into());
        assert!(matches!(jwk.algorithm(), Err(KeyProblem::Unsupported(_))));
    }

    #[test]
    fn an_es256_signature_verifies_against_its_jwk_and_nothing_else() {
        use p256::ecdsa::signature::Signer;

        let key = p256::ecdsa::SigningKey::from_slice(&[7u8; 32]).unwrap();
        let point = key.verifying_key().to_sec1_point(false);
        let (x, y) = point.as_bytes()[1..].split_at(32);
        let jwk = Jwk {
            kty: "EC".into(),
            crv: Some("P-256".into()),
            x: Some(URL_SAFE_NO_PAD.encode(x)),
            y: Some(URL_SAFE_NO_PAD.encode(y)),
            alg: Some("ES256".into()),
            ..Default::default()
        };
        let signature: p256::ecdsa::Signature = key.sign(b"message");

        assert!(jwk.verify(Algorithm::Es256, b"message", &signature.to_bytes()));
        assert!(!jwk.verify(Algorithm::Es256, b"tampered", &signature.to_bytes()));
    }

    #[test]
    fn a_small_order_key_verifies_nothing() {
        // The identity point, with an all-zero-R signature that a
        // non-strict check accepts for any message.
        let mut identity = [0u8; 32];
        identity[0] = 1;
        let jwk = Jwk {
            kty: "OKP".into(),
            crv: Some("Ed25519".into()),
            x: Some(URL_SAFE_NO_PAD.encode(identity)),
            alg: Some("Ed25519".into()),
            ..Default::default()
        };
        let mut signature = [0u8; 64];
        signature[0] = 1;
        assert!(!jwk.verify(Algorithm::Ed25519, b"anything", &signature));
    }
}
