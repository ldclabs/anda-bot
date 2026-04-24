use anda_core::BoxError;
use ic_auth_types::ByteBufB64;
use ic_cose_types::cose::{CborSerializable, CoseKey, ed25519::VerifyingKey, get_cose_key_public};
use std::str::FromStr;

pub mod agents;
pub mod handler;
pub mod payload;
pub mod space;
pub mod types;

pub fn parse_ed25519_pubkeys(input: &str) -> Result<Vec<VerifyingKey>, BoxError> {
    if input.is_empty() {
        return Ok(vec![]);
    }

    input
        .split(',')
        .map(|item| match parse_ed25519_pubkey(item.trim()) {
            Some(key) => Ok(key),
            None => Err("invalid ED25519_PUBKEYS entry".into()),
        })
        .collect::<Result<Vec<_>, _>>()
}

fn parse_ed25519_pubkey(input: &str) -> Option<VerifyingKey> {
    let data = ByteBufB64::from_str(input).ok()?;

    if data.len() == 32 {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&data);
        return VerifyingKey::from_bytes(&bytes).ok();
    }

    let cose_key = CoseKey::from_slice(data.as_slice()).ok()?;
    let public_key = get_cose_key_public(cose_key).ok()?;
    let bytes: [u8; 32] = public_key.try_into().ok()?;
    VerifyingKey::from_bytes(&bytes).ok()
}
