//! AES-128-ECB encryption/decryption with PKCS7 padding.

use aes::Aes128;
use cipher::{BlockDecryptMut, BlockEncryptMut, KeyInit, block_padding::Pkcs7};

use crate::error::{Error, Result};

type Aes128EcbEnc = ecb::Encryptor<Aes128>;
type Aes128EcbDec = ecb::Decryptor<Aes128>;

/// Encrypt data with AES-128-ECB + PKCS7 padding.
pub fn encrypt(plaintext: &[u8], key: &[u8; 16]) -> Result<Vec<u8>> {
    let padded_len = padded_size(plaintext.len());
    let mut buf = vec![0u8; padded_len];
    buf[..plaintext.len()].copy_from_slice(plaintext);
    let ct = Aes128EcbEnc::new(key.into())
        .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
        .map_err(|e| Error::Crypto(format!("AES-ECB encrypt: {e}")))?;
    Ok(ct.to_vec())
}

/// Decrypt AES-128-ECB + PKCS7 padded data.
pub fn decrypt(ciphertext: &[u8], key: &[u8; 16]) -> Result<Vec<u8>> {
    let mut buf = ciphertext.to_vec();
    let pt = Aes128EcbDec::new(key.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| Error::Crypto(format!("AES-ECB decrypt: {e}")))?;
    Ok(pt.to_vec())
}

/// Compute ciphertext size after PKCS7 padding to 16-byte boundary.
pub fn padded_size(plaintext_size: usize) -> usize {
    (plaintext_size + 1).div_ceil(16) * 16
}

/// Parse a base64-encoded AES key into 16 raw bytes.
///
/// Two encodings exist:
/// - base64 → 16 raw bytes (images)
/// - base64 → 32 hex chars → 16 bytes (file/voice/video)
pub fn parse_aes_key(aes_key_base64: &str) -> Result<[u8; 16]> {
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(aes_key_base64)
        .map_err(|e| Error::Crypto(format!("base64 decode aes_key: {e}")))?;

    if decoded.len() == 16 {
        let mut key = [0u8; 16];
        key.copy_from_slice(&decoded);
        return Ok(key);
    }

    if decoded.len() == 32 {
        let hex_str = std::str::from_utf8(&decoded)
            .map_err(|_| Error::Crypto("aes_key hex not valid UTF-8".into()))?;
        if hex_str.len() == 32 && hex_str.chars().all(|c| c.is_ascii_hexdigit()) {
            let bytes = hex_to_bytes(hex_str)?;
            let mut key = [0u8; 16];
            key.copy_from_slice(&bytes);
            return Ok(key);
        }
    }

    Err(Error::Crypto(format!(
        "aes_key must decode to 16 raw bytes or 32-char hex string, got {} bytes",
        decoded.len()
    )))
}

pub(crate) fn hex_to_bytes(hex: &str) -> Result<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return Err(Error::Crypto("hex string must have even length".into()));
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|e| Error::Crypto(format!("hex parse: {e}")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = [0xABu8; 16];
        let plaintext = b"hello world!";
        let ct = encrypt(plaintext, &key).unwrap();
        let pt = decrypt(&ct, &key).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn padded_size_values() {
        assert_eq!(padded_size(0), 16);
        assert_eq!(padded_size(1), 16);
        assert_eq!(padded_size(15), 16);
        assert_eq!(padded_size(16), 32);
        assert_eq!(padded_size(17), 32);
        assert_eq!(padded_size(31), 32);
        assert_eq!(padded_size(32), 48);
    }

    #[test]
    fn parse_aes_key_16_raw_bytes() {
        use base64::Engine;
        let raw = [0x01u8; 16];
        let b64 = base64::engine::general_purpose::STANDARD.encode(raw);
        let key = parse_aes_key(&b64).unwrap();
        assert_eq!(key, raw);
    }

    #[test]
    fn parse_aes_key_32_hex_format() {
        use base64::Engine;
        let hex_str = "0123456789abcdef0123456789abcdef";
        let b64 = base64::engine::general_purpose::STANDARD.encode(hex_str.as_bytes());
        let key = parse_aes_key(&b64).unwrap();
        let expected = hex_to_bytes(hex_str).unwrap();
        assert_eq!(key, expected.as_slice());
    }

    #[test]
    fn parse_aes_key_invalid() {
        use base64::Engine;
        let bad = base64::engine::general_purpose::STANDARD.encode([0u8; 5]);
        assert!(parse_aes_key(&bad).is_err());
    }

    #[test]
    fn encrypt_empty() {
        let key = [0u8; 16];
        let ct = encrypt(b"", &key).unwrap();
        assert_eq!(ct.len(), 16); // one full padding block
        let pt = decrypt(&ct, &key).unwrap();
        assert!(pt.is_empty());
    }

    #[test]
    fn test_hex_to_bytes() {
        assert_eq!(hex_to_bytes("00ff0a").unwrap(), vec![0x00, 0xff, 0x0a]);
        assert_eq!(hex_to_bytes("").unwrap(), Vec::<u8>::new());
        assert!(hex_to_bytes("abc").is_err()); // odd length
    }
}
