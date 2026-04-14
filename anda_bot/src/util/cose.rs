use anda_core::BoxError;
use coset::{CoseKeyBuilder, RegisteredLabel, iana};
use ic_auth_types::ByteBufB64;
use ic_cose_types::cose::{
    CborSerializable, CoseKey, ed25519::SigningKey, get_cose_key_public, get_cose_key_secret,
};
use std::str::FromStr;

pub fn parse_ed25519_pubkey(input: &str) -> Result<[u8; 32], BoxError> {
    let data = ByteBufB64::from_str(input)?;

    if data.len() == 32 {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&data);
        return Ok(bytes);
    }

    let cose_key = CoseKey::from_slice(data.as_slice())?;
    if cose_key.kty != RegisteredLabel::Assigned(iana::KeyType::OKP) {
        return Err("invalid key type".into());
    }
    let public_key = get_cose_key_public(cose_key)?;
    let bytes: [u8; 32] = public_key.try_into().map_err(|_err| "invalid key length")?;
    Ok(bytes)
}

pub fn to_ed25519_pubkey(bytes: &[u8; 32]) -> [u8; 32] {
    let sk = SigningKey::from_bytes(bytes);
    sk.verifying_key().to_bytes()
}

pub fn parse_ed25519_privkey(input: &str) -> Result<[u8; 32], BoxError> {
    let data = ByteBufB64::from_str(input)?;

    if data.len() == 32 {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&data);
        return Ok(bytes);
    }

    let cose_key = CoseKey::from_slice(data.as_slice())?;
    if cose_key.kty != RegisteredLabel::Assigned(iana::KeyType::OKP) {
        return Err("invalid key type".into());
    }
    let secret = get_cose_key_secret(cose_key)?;
    let bytes: [u8; 32] = secret.try_into().map_err(|_err| "invalid key length")?;
    Ok(bytes)
}

pub fn encode_ed25519_privkey(secret: &[u8; 32]) -> Result<String, BoxError> {
    let cose_key = CoseKeyBuilder::new_okp_key()
        .algorithm(iana::Algorithm::EdDSA)
        .param(
            iana::OkpKeyParameter::Crv as i64,
            (iana::EllipticCurve::Ed25519 as i64).into(),
        )
        .param(iana::OkpKeyParameter::D as i64, secret.to_vec().into())
        .build();
    let cose_bytes = cose_key.to_vec()?;
    Ok(ByteBufB64(cose_bytes).to_string())
}

pub fn random_ed25519_privkey() -> [u8; 32] {
    let mut rng = rand::rng();
    let mut bytes = [0u8; 32];
    rand::Rng::fill_bytes(&mut rng, &mut bytes);
    bytes
}
