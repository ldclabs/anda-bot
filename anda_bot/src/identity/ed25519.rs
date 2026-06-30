use anda_core::{BoxError, Principal};
use anda_web3_client::client::{Identity, identity_from_secret};
use cose2::{Key as CoseKey, Label, Sign1Message};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use ic_auth_types::ByteBufB64;
use ic_ed25519::PublicKey;
use std::{str::FromStr, sync::Arc};

use super::{Claims, iana};

#[derive(Clone)]
pub struct Ed25519Key {
    id: Principal,
    key: SigningKey,
    identity: Arc<dyn Identity>,
}

impl Ed25519Key {
    pub fn new(secret: [u8; 32]) -> Self {
        let key = SigningKey::from_bytes(&secret);
        let identity = identity_from_secret(key.to_bytes());
        Self {
            id: pubkey_to_principal(&(key.verifying_key())),
            identity: Arc::new(identity),
            key,
        }
    }

    #[allow(unused)]
    pub fn id(&self) -> Principal {
        self.id
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        self.key.as_bytes()
    }

    pub fn pubkey(&self) -> Ed25519PubKey {
        Ed25519PubKey {
            id: self.id,
            key: self.key.verifying_key(),
        }
    }

    pub fn identity(&self) -> Arc<dyn Identity> {
        self.identity.clone()
    }

    pub fn sign_cwt(&self, mut claims: Claims) -> Result<String, BoxError> {
        claims.subject = self.identity.sender().map(|s| s.to_string()).ok();
        let tagged_payload = claims.to_vec()?;
        let payload = cose2::tag::skip_tag(cose2::tag::CWT_PREFIX, &tagged_payload).to_vec();
        let mut sign1 = Sign1Message::new(Some(payload));
        let tbs_data = sign1.prepare_signature(Some(iana::AlgorithmEdDSA.into()), None, None)?;
        let sig = self.key.sign(&tbs_data);
        sign1.set_signature(sig.to_vec())?;
        let cose_bytes = sign1.to_vec()?;
        Ok(ByteBufB64(cose_bytes).to_string())
    }
}

#[derive(Clone)]
pub struct Ed25519PubKey {
    id: Principal,
    key: VerifyingKey,
}

impl Ed25519PubKey {
    pub fn new(key: [u8; 32]) -> Result<Self, BoxError> {
        let key = VerifyingKey::from_bytes(&key)?;
        Ok(Self {
            id: pubkey_to_principal(&key),
            key,
        })
    }

    pub fn id(&self) -> Principal {
        self.id
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        self.key.as_bytes()
    }
}

impl From<Ed25519PubKey> for VerifyingKey {
    fn from(pubkey: Ed25519PubKey) -> Self {
        pubkey.key
    }
}

impl FromStr for Ed25519Key {
    type Err = BoxError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let secret_bytes = parse_ed25519_privkey(s)?;
        Ok(Self::new(secret_bytes))
    }
}

impl FromStr for Ed25519PubKey {
    type Err = BoxError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let key = parse_ed25519_pubkey(s)?;
        Self::new(key)
    }
}

pub fn pubkey_to_principal(pubkey: &VerifyingKey) -> Principal {
    let public_key = PublicKey::deserialize_raw(pubkey.as_bytes()).unwrap();
    let der_encoded_public_key = public_key.serialize_rfc8410_der();
    Principal::self_authenticating(&der_encoded_public_key)
}

pub fn parse_ed25519_pubkey(input: &str) -> Result<[u8; 32], BoxError> {
    let data = ByteBufB64::from_str(input)?;

    if data.len() == 32 {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&data);
        return Ok(bytes);
    }

    let cose_key = okp_cose_key(data.as_slice())?;
    let public_key = cose_key
        .get_bytes(iana::OKPKeyParameterX)?
        .ok_or("missing public key")?;
    let bytes: [u8; 32] = public_key.try_into().map_err(|_err| "invalid key length")?;
    Ok(bytes)
}

pub fn parse_ed25519_privkey(input: &str) -> Result<[u8; 32], BoxError> {
    let data = ByteBufB64::from_str(input)?;

    if data.len() == 32 {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&data);
        return Ok(bytes);
    }

    let cose_key = okp_cose_key(data.as_slice())?;
    let secret = cose_key
        .get_bytes(iana::OKPKeyParameterD)?
        .ok_or("missing secret key")?;
    let bytes: [u8; 32] = secret.try_into().map_err(|_err| "invalid key length")?;
    Ok(bytes)
}

fn okp_cose_key(data: &[u8]) -> Result<CoseKey, BoxError> {
    let key = CoseKey::from_slice(data)?;
    ensure_okp_key(&key)?;
    Ok(key)
}

fn ensure_okp_key(key: &CoseKey) -> Result<(), BoxError> {
    match key.kty()? {
        Some(Label::Int(iana::KeyTypeOKP)) => Ok(()),
        _ => Err("invalid key type".into()),
    }
}

pub fn encode_ed25519_privkey(secret: &[u8; 32]) -> Result<String, BoxError> {
    // COSE Key: {1: kty, 3: alg, -1: crv, -4: d}
    let mut cose_key = CoseKey::new();
    cose_key
        .set_kty(iana::KeyTypeOKP)
        .set_alg(iana::AlgorithmEdDSA);
    cose_key.insert(iana::OKPKeyParameterCrv, iana::EllipticCurveEd25519);
    cose_key.insert(iana::OKPKeyParameterD, secret.to_vec());
    let cose_bytes = cose_key.to_vec()?;
    Ok(ByteBufB64(cose_bytes).to_string())
}

pub(super) fn encode_ed25519_privkey_cose_key(secret: &[u8; 32]) -> Result<Vec<u8>, BoxError> {
    // COSE Key: {1: kty, 3: alg, -1: crv, -2: x, -4: d}
    let key = SigningKey::from_bytes(secret);
    let mut cose_key = CoseKey::new();
    cose_key
        .set_kty(iana::KeyTypeOKP)
        .set_alg(iana::AlgorithmEdDSA);
    cose_key.insert(iana::OKPKeyParameterCrv, iana::EllipticCurveEd25519);
    cose_key.insert(
        iana::OKPKeyParameterX,
        key.verifying_key().to_bytes().to_vec(),
    );
    cose_key.insert(iana::OKPKeyParameterD, secret.to_vec());
    Ok(cose_key.to_vec()?)
}

pub(super) fn decode_ed25519_privkey_cose_key(data: &[u8]) -> Result<[u8; 32], BoxError> {
    let cose_key = okp_cose_key(data)?;
    match cose_key.alg()? {
        Some(Label::Int(iana::AlgorithmEdDSA)) => {}
        _ => return Err("invalid Ed25519 key algorithm".into()),
    }
    match cose_key.get_label(iana::OKPKeyParameterCrv)? {
        Some(Label::Int(iana::EllipticCurveEd25519)) => {}
        _ => return Err("invalid Ed25519 key curve".into()),
    }

    let secret = cose_key
        .get_bytes(iana::OKPKeyParameterD)?
        .ok_or("missing secret key")?;
    let secret: [u8; 32] = secret.try_into().map_err(|_err| "invalid key length")?;

    if let Some(public_key) = cose_key.get_bytes(iana::OKPKeyParameterX)? {
        let public_key: [u8; 32] = public_key
            .try_into()
            .map_err(|_err| "invalid public key length")?;
        let derived = SigningKey::from_bytes(&secret).verifying_key().to_bytes();
        if public_key != derived {
            return Err("Ed25519 public key does not match secret key".into());
        }
    }

    Ok(secret)
}

pub fn encode_ed25519_pubkey(pubkey: &Ed25519PubKey) -> String {
    ByteBufB64(pubkey.as_bytes().to_vec()).to_string()
}

pub fn random_ed25519_privkey() -> [u8; 32] {
    let mut rng = rand::rng();
    let mut bytes = [0u8; 32];
    rand::Rng::fill_bytes(&mut rng, &mut bytes);
    bytes
}
