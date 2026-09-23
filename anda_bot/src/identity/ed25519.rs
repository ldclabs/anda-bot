use anda_core::{BoxError, Principal};
use anda_web3_client::client::{Identity, identity_from_secret};
use cose2::{CoseMap, Key as CoseKey, Label, Sign1Message, Value};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey, ed25519::SignatureEncoding};
use ic_auth_types::{ByteBufB64, BytesB64};
use ic_ed25519::PublicKey;
use std::{
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use super::{Claims, iana};
use zeroize::{Zeroize, Zeroizing};

/// Creates CWT claims with an explicit issuance time and bounded lifetime.
pub fn expiring_claims(lifetime: Duration) -> Result<Claims, BoxError> {
    let issued_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    Ok(Claims {
        issued_at: Some(issued_at.into()),
        expiration: Some(issued_at.saturating_add(lifetime.as_secs()).into()),
        ..Default::default()
    })
}

#[derive(Clone)]
pub struct Ed25519Key {
    id: Principal,
    key: SigningKey,
    identity: Arc<dyn Identity>,
}

impl Ed25519Key {
    pub fn new(mut secret: [u8; 32]) -> Self {
        let key = SigningKey::from_bytes(&secret);
        let identity = identity_from_secret(key.to_bytes());
        secret.zeroize();
        Self {
            id: pubkey_to_principal(&(key.verifying_key())),
            identity: Arc::from(identity),
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
        if claims.expiration.is_none() {
            return Err("CWT expiration is required".into());
        }
        claims.subject = Some(self.id.to_string());
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
    let der_encoded_public_key = PublicKey::convert_raw32_to_der(*pubkey.as_bytes());
    Principal::self_authenticating(&der_encoded_public_key)
}

pub fn parse_ed25519_pubkey(input: &str) -> Result<[u8; 32], BoxError> {
    let data = Zeroizing::new(ByteBufB64::from_str(input)?.0);

    if data.len() == 32 {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&data);
        return Ok(bytes);
    }

    with_ed25519_cose_key(&data, |key| {
        let public_key = key
            .get_bytes(iana::OKPKeyParameterX)?
            .ok_or("missing public key")?;
        Ok(public_key.try_into().map_err(|_| "invalid key length")?)
    })
}

pub fn parse_ed25519_privkey(input: &str) -> Result<[u8; 32], BoxError> {
    let data = Zeroizing::new(ByteBufB64::from_str(input)?.0);

    if data.len() == 32 {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&data);
        return Ok(bytes);
    }

    with_ed25519_cose_key(&data, cose_private_key)
}

fn with_ed25519_cose_key<T>(
    data: &[u8],
    read: impl FnOnce(&CoseKey) -> Result<T, BoxError>,
) -> Result<T, BoxError> {
    // Validate after decoding so the private parameter is wiped even if the
    // key type, curve, algorithm or key material is invalid.
    let mut key = CoseKey(CoseMap::from_slice(data)?);
    let result = (|| {
        key.validate()?;
        ensure_ed25519_key(&key)?;
        read(&key)
    })();
    wipe_cose_secret(&mut key);
    result
}

fn ensure_ed25519_key(key: &CoseKey) -> Result<(), BoxError> {
    match key.kty()? {
        Some(Label::Int(iana::KeyTypeOKP)) => {}
        _ => return Err("invalid key type".into()),
    }
    match key.get_label(iana::OKPKeyParameterCrv)? {
        Some(Label::Int(iana::EllipticCurveEd25519)) => {}
        _ => return Err("invalid Ed25519 key curve".into()),
    }
    match key.alg()? {
        None | Some(Label::Int(iana::AlgorithmEdDSA)) => Ok(()),
        _ => Err("invalid Ed25519 key algorithm".into()),
    }
}

fn wipe_cose_secret(key: &mut CoseKey) {
    if let Some(Value::Bytes(mut secret)) = key.remove(iana::OKPKeyParameterD) {
        secret.zeroize();
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
    let encoded = cose_key.to_vec();
    wipe_cose_secret(&mut cose_key);
    let encoded = Zeroizing::new(encoded?);
    Ok(BytesB64::from_slice(encoded.as_slice()).to_string())
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
    let encoded = cose_key.to_vec();
    wipe_cose_secret(&mut cose_key);
    Ok(encoded?)
}

pub(super) fn decode_ed25519_privkey_cose_key(data: &[u8]) -> Result<[u8; 32], BoxError> {
    with_ed25519_cose_key(data, |key| {
        if key.alg()?.is_none() {
            return Err("missing Ed25519 key algorithm".into());
        }
        cose_private_key(key)
    })
}

fn cose_private_key(cose_key: &CoseKey) -> Result<[u8; 32], BoxError> {
    let secret = cose_key
        .get_bytes(iana::OKPKeyParameterD)?
        .ok_or("missing secret key")?;
    let secret = Zeroizing::new(<[u8; 32]>::try_from(secret).map_err(|_| "invalid key length")?);

    if let Some(public_key) = cose_key.get_bytes(iana::OKPKeyParameterX)? {
        let public_key: [u8; 32] = public_key
            .try_into()
            .map_err(|_err| "invalid public key length")?;
        let derived = SigningKey::from_bytes(&secret).verifying_key().to_bytes();
        if public_key != derived {
            return Err("Ed25519 public key does not match secret key".into());
        }
    }

    Ok(*secret)
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
