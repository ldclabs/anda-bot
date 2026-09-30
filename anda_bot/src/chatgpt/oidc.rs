//! Strict OIDC ID-token validation using the project's existing crypto provider.
use anda_core::BoxError;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use cose2::{Verifier, crypto::RingVerifier, iana};
use serde::Deserialize;

use super::{ISSUER, unix_seconds};

#[derive(Deserialize)]
pub(super) struct Jwks {
    pub keys: Vec<Jwk>,
}
#[derive(Deserialize)]
pub(super) struct Jwk {
    kid: String,
    kty: String,
    #[serde(default)]
    alg: Option<String>,
    #[serde(default, rename = "use")]
    usage: Option<String>,
    #[serde(default)]
    n: String,
    #[serde(default)]
    e: String,
}
#[derive(Deserialize)]
struct Header {
    alg: String,
    kid: String,
    #[serde(default)]
    crit: Vec<String>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}
#[derive(Deserialize)]
pub(super) struct Identity {
    pub iss: String,
    pub sub: String,
    aud: Audience,
    exp: u64,
    #[serde(default)]
    nbf: Option<u64>,
    #[serde(default)]
    iat: Option<u64>,
    #[serde(default)]
    azp: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

pub(super) fn verify(
    token: &str,
    jwks: &Jwks,
    client_id: &str,
    nonce: Option<&str>,
) -> Result<Identity, BoxError> {
    if token.len() > 64 * 1024 {
        return Err("ID token is too large".into());
    }
    let parts: Vec<_> = token.split('.').collect();
    if parts.len() != 3 {
        return Err("invalid ID token".into());
    }
    let header: Header = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0])?)?;
    if header.alg != "RS256" || !header.crit.is_empty() {
        return Err("unsupported ID token algorithm or critical header".into());
    }
    let keys: Vec<_> = jwks
        .keys
        .iter()
        .filter(|k| {
            k.kid == header.kid
                && k.kty == "RSA"
                && k.alg.as_deref().is_none_or(|v| v == "RS256")
                && k.usage.as_deref().is_none_or(|v| v == "sig")
        })
        .collect();
    if keys.len() != 1 {
        return Err("ID token signing key is unknown or ambiguous".into());
    }
    let key = keys[0];
    RingVerifier::rsa_components(
        iana::AlgorithmRS256,
        &URL_SAFE_NO_PAD.decode(&key.n)?,
        &URL_SAFE_NO_PAD.decode(&key.e)?,
        None,
    )?
    .verify(
        format!("{}.{}", parts[0], parts[1]).as_bytes(),
        &URL_SAFE_NO_PAD.decode(parts[2])?,
    )
    .map_err(|_| "ID token signature is invalid")?;
    let identity: Identity = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1])?)?;
    identity.validate(client_id, nonce, unix_seconds())?;
    Ok(identity)
}
impl Identity {
    fn validate(&self, client_id: &str, nonce: Option<&str>, now: u64) -> Result<(), BoxError> {
        let (matches, multiple) = match &self.aud {
            Audience::One(a) => (a == client_id, false),
            Audience::Many(a) => (a.iter().any(|v| v == client_id), a.len() > 1),
        };
        if self.iss != ISSUER
            || self.sub.is_empty()
            || !matches
            || self.exp.saturating_add(30) <= now
            || self.nbf.is_some_and(|v| v > now + 30)
            || self.iat.is_some_and(|v| v > now + 30)
            || self.azp.as_deref().is_some_and(|v| v != client_id)
            || (multiple && self.azp.as_deref() != Some(client_id))
            || nonce.is_some_and(|v| self.nonce.as_deref() != Some(v))
        {
            return Err("ID token identity, audience, lifetime, or nonce is invalid".into());
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chatgpt_oidc_claims_bind_identity_to_attempt() {
        let value = serde_json::json!({"iss":ISSUER,"sub":"user","aud":"client","exp":120,"nonce":"attempt"});
        let valid: Identity = serde_json::from_value(value.clone()).unwrap();
        assert!(valid.validate("client", Some("attempt"), 100).is_ok());
        for (key, replacement) in [
            ("iss", serde_json::json!("https://evil.invalid")),
            ("sub", serde_json::json!("")),
            ("aud", serde_json::json!("other")),
            ("exp", serde_json::json!(1)),
            ("nonce", serde_json::json!("other")),
        ] {
            let mut bad = value.clone();
            bad[key] = replacement;
            assert!(
                serde_json::from_value::<Identity>(bad)
                    .unwrap()
                    .validate("client", Some("attempt"), 100)
                    .is_err()
            );
        }
        assert!(
            verify(
                "eyJhbGciOiJub25lIiwia2lkIjoieCJ9.e30.",
                &Jwks { keys: vec![] },
                "client",
                Some("attempt")
            )
            .is_err()
        );
    }
}
