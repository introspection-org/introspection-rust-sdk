//! The Ed25519 keys this crate signs with: an agent's session key, which its
//! agent token's `cnf.jwk` binds, and a server's token-signing key.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use ed25519_dalek::Signer;

use crate::error::{Error, Result};
use crate::jwk::{Algorithm, Jwk, Jwks};

/// An Ed25519 private key, with its public JWK and thumbprint.
///
/// ```
/// use aauth_protocol::SigningKey;
///
/// let key = SigningKey::generate();
/// let again = SigningKey::from_pkcs8_pem(&key.to_pkcs8_pem()?)?;
/// assert_eq!(key.thumbprint(), again.thumbprint());
/// # Ok::<(), aauth_protocol::Error>(())
/// ```
pub struct SigningKey {
    key: ed25519_dalek::SigningKey,
    jwk: Jwk,
    thumbprint: String,
}

impl std::fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigningKey")
            .field("thumbprint", &self.thumbprint)
            .finish_non_exhaustive()
    }
}

impl SigningKey {
    /// A fresh key from the operating system's randomness.
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).expect("the operating system provides randomness");
        Self::from_bytes(&seed)
    }

    /// The key a 32-byte seed derives.
    pub fn from_bytes(seed: &[u8; 32]) -> Self {
        Self::new(ed25519_dalek::SigningKey::from_bytes(seed))
    }

    /// A PKCS#8 PEM (`-----BEGIN PRIVATE KEY-----`).
    pub fn from_pkcs8_pem(pem: &str) -> Result<Self> {
        ed25519_dalek::SigningKey::from_pkcs8_pem(pem)
            .map(Self::new)
            .map_err(|e| Error::Key(e.to_string()))
    }

    /// The key as a PKCS#8 PEM. It is secret: store it like a password.
    pub fn to_pkcs8_pem(&self) -> Result<String> {
        self.key
            .to_pkcs8_pem(Default::default())
            .map(|pem| pem.to_string())
            .map_err(|e| Error::Key(e.to_string()))
    }

    fn new(key: ed25519_dalek::SigningKey) -> Self {
        let jwk = Jwk {
            kty: "OKP".into(),
            crv: Some("Ed25519".into()),
            x: Some(URL_SAFE_NO_PAD.encode(key.verifying_key().as_bytes())),
            alg: Some(Algorithm::Ed25519.as_str().into()),
            ..Default::default()
        };
        let thumbprint = jwk.thumbprint().expect("an OKP JWK has a thumbprint");
        Self {
            key,
            jwk,
            thumbprint,
        }
    }

    /// RFC 7638 thumbprint of the public key.
    pub fn thumbprint(&self) -> &str {
        &self.thumbprint
    }

    /// The algorithm it signs with: always [`Algorithm::Ed25519`].
    pub fn algorithm(&self) -> Algorithm {
        Algorithm::Ed25519
    }

    /// The public key, with `alg`, as a `cnf.jwk` carries it.
    pub fn public_jwk(&self) -> Jwk {
        self.jwk.clone()
    }

    /// The public key as a one-key JWKS, its `kid` the thumbprint.
    pub fn jwks(&self) -> Jwks {
        Jwks {
            keys: vec![Jwk {
                kid: Some(self.thumbprint.clone()),
                use_: Some("sig".into()),
                ..self.jwk.clone()
            }],
        }
    }

    /// Sign `message`; the 64-byte signature.
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.key.sign(message).to_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pem_round_trips() {
        let key = SigningKey::generate();
        let again = SigningKey::from_pkcs8_pem(&key.to_pkcs8_pem().unwrap()).unwrap();
        assert_eq!(key.thumbprint(), again.thumbprint());
    }

    #[test]
    fn a_signature_verifies_under_the_public_jwk() {
        let key = SigningKey::generate();
        let signature = key.sign(b"hello");
        assert!(key
            .public_jwk()
            .verify(Algorithm::Ed25519, b"hello", &signature));
        assert!(!key
            .public_jwk()
            .verify(Algorithm::Ed25519, b"hellO", &signature));
    }
}
