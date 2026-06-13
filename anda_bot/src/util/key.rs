use anda_core::{BoxError, Principal};
use anda_web3_client::client::{Identity, identity_from_secret};
use ic_auth_types::ByteBufB64;
use ic_cose_types::cose::{
    CborSerializable, CoseKey,
    ed25519::{Signer, SigningKey, VerifyingKey},
    get_cose_key_public, get_cose_key_secret,
    sign1::cose_sign1,
};
use ic_ed25519::PublicKey;
use std::{str::FromStr, sync::Arc};

pub use ic_cose_types::cose::cwt::{ClaimsSet, ClaimsSetBuilder};
pub use ic_cose_types::cose::iana;

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

    pub fn sign_cwt(&self, mut claims: ClaimsSet) -> Result<String, BoxError> {
        claims.subject = self.identity.sender().map(|s| s.to_string()).ok();
        let mut sign1 = cose_sign1(claims.to_vec()?, iana::Algorithm::EdDSA, None)?;
        let tbs_data = sign1.tbs_data(&[]);
        let sig = self.key.sign(&tbs_data);
        sign1.signature = sig.to_vec();
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

    ensure_okp_key(data.as_slice())?;
    let cose_key = CoseKey::from_slice(data.as_slice())?;
    let public_key = get_cose_key_public(cose_key)?;
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

    ensure_okp_key(data.as_slice())?;
    let cose_key = CoseKey::from_slice(data.as_slice())?;
    let secret = get_cose_key_secret(cose_key)?;
    let bytes: [u8; 32] = secret.try_into().map_err(|_err| "invalid key length")?;
    Ok(bytes)
}

fn ensure_okp_key(data: &[u8]) -> Result<(), BoxError> {
    let value: cbor2::Value = cbor2::from_slice(data)?;
    let map = value.as_map().ok_or("invalid COSE key")?;
    for (k, v) in map {
        if k.as_integer() == Some(1.into()) {
            if v.as_integer() == Some((iana::KeyType::OKP as i64).into()) {
                return Ok(());
            }
            return Err("invalid key type".into());
        }
    }
    Err("invalid key type".into())
}

pub fn encode_ed25519_privkey(secret: &[u8; 32]) -> Result<String, BoxError> {
    use cbor2::Value;

    // COSE Key: {1: kty, 3: alg, -1: crv, -4: d}
    let cose_key = cbor2::cbor!({
        1 => iana::KeyType::OKP as i64,
        3 => iana::Algorithm::EdDSA as i64,
        -1 => iana::EllipticCurve::Ed25519 as i64,
        -4 => Value::Bytes(secret.to_vec()),
    })?;
    let cose_bytes = cbor2::to_vec(&cose_key)?;
    Ok(ByteBufB64(cose_bytes).to_string())
}

pub fn random_ed25519_privkey() -> [u8; 32] {
    let mut rng = rand::rng();
    let mut bytes = [0u8; 32];
    rand::Rng::fill_bytes(&mut rng, &mut bytes);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use cbor2::Value;

    const SECRET: [u8; 32] = [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
        26, 27, 28, 29, 30, 31, 32,
    ];

    #[test]
    fn private_key_round_trips_between_raw_and_cose_formats() {
        let raw = ByteBufB64(SECRET.to_vec()).to_string();
        assert_eq!(parse_ed25519_privkey(&raw).unwrap(), SECRET);

        let encoded = encode_ed25519_privkey(&SECRET).unwrap();
        assert_eq!(parse_ed25519_privkey(&encoded).unwrap(), SECRET);
    }

    #[test]
    fn public_key_parser_accepts_raw_public_key_bytes() {
        let key = Ed25519Key::new(SECRET);
        let raw = ByteBufB64(key.pubkey().as_bytes().to_vec()).to_string();

        assert_eq!(
            parse_ed25519_pubkey(&raw).unwrap(),
            *key.pubkey().as_bytes()
        );
    }

    #[test]
    fn key_constructors_derive_matching_principals_and_identity() {
        let key = Ed25519Key::new(SECRET);
        let pubkey = key.pubkey();

        assert_eq!(key.id(), pubkey.id());
        assert_eq!(key.as_bytes(), &SECRET);
        assert_eq!(key.identity().sender().unwrap(), key.id());
    }

    #[test]
    fn invalid_key_input_returns_error() {
        assert!(parse_ed25519_privkey("not base64").is_err());
        assert!(parse_ed25519_pubkey("not base64").is_err());

        let short = ByteBufB64(vec![1, 2, 3]).to_string();
        assert!(parse_ed25519_privkey(&short).is_err());
        assert!(parse_ed25519_pubkey(&short).is_err());
    }

    #[test]
    fn random_private_key_has_expected_length_and_is_not_all_zeroes() {
        let key = random_ed25519_privkey();

        assert_eq!(key.len(), 32);
        assert!(key.iter().any(|byte| *byte != 0));
    }

    #[test]
    fn keys_parse_from_raw_and_cose_strings() {
        let raw = ByteBufB64(SECRET.to_vec()).to_string();
        let key = Ed25519Key::from_str(&raw).unwrap();
        assert_eq!(key.as_bytes(), &SECRET);

        let encoded = encode_ed25519_privkey(&SECRET).unwrap();
        let key = Ed25519Key::from_str(&encoded).unwrap();
        assert_eq!(key.as_bytes(), &SECRET);

        let pub_raw = ByteBufB64(key.pubkey().as_bytes().to_vec()).to_string();
        let pubkey = Ed25519PubKey::from_str(&pub_raw).unwrap();
        assert_eq!(pubkey.id(), key.id());

        let verifying: VerifyingKey = pubkey.into();
        assert_eq!(verifying.as_bytes(), key.pubkey().as_bytes());
    }

    #[test]
    fn cose_keys_with_wrong_key_type_are_rejected() {
        // EC2 public key: {1: 2(EC2), -1: 1(P-256), -2: x, -3: y}
        let ec2 = cbor2::cbor!({
            1 => iana::KeyType::EC2 as i64,
            -1 => iana::EllipticCurve::P_256 as i64,
            -2 => Value::Bytes(vec![1u8; 32]),
            -3 => Value::Bytes(vec![2u8; 32]),
        })
        .unwrap();
        let encoded = ByteBufB64(cbor2::to_vec(&ec2).unwrap()).to_string();

        assert!(parse_ed25519_privkey(&encoded).is_err());
        assert!(parse_ed25519_pubkey(&encoded).is_err());
    }

    #[test]
    fn cose_public_key_round_trips() {
        let key = Ed25519Key::new(SECRET);
        // OKP public key: {1: 1(OKP), 3: -8(EdDSA), -1: 6(Ed25519), -2: x}
        let cose_key = cbor2::cbor!({
            1 => iana::KeyType::OKP as i64,
            3 => iana::Algorithm::EdDSA as i64,
            iana::OkpKeyParameter::Crv as i64 => iana::EllipticCurve::Ed25519 as i64,
            iana::OkpKeyParameter::X as i64 => Value::Bytes(key.pubkey().as_bytes().to_vec()),
        })
        .unwrap();
        let encoded = ByteBufB64(cbor2::to_vec(&cose_key).unwrap()).to_string();

        assert_eq!(
            parse_ed25519_pubkey(&encoded).unwrap(),
            *key.pubkey().as_bytes()
        );
    }

    #[test]
    fn sign_cwt_produces_decodable_cose_sign1() {
        let key = Ed25519Key::new(SECRET);
        let claims = ClaimsSet::default();

        let token = key.sign_cwt(claims).unwrap();
        let bytes = ByteBufB64::from_str(&token).unwrap();
        // COSE_Sign1 = #6.18([protected, unprotected, payload, signature])
        let value: Value = cbor2::from_slice(&bytes).unwrap();
        let arr = match value {
            Value::Tag(18, inner) => inner.into_array().unwrap(),
            other => other.into_array().unwrap(),
        };
        let sig = arr[3].as_bytes().unwrap();
        assert_eq!(sig.len(), 64);
    }
}
