//! Encrypted, atomically replaced credentials. One daemon owns refreshes per home.
use anda_core::BoxError;
use cose2::{Encrypt0Message, crypto::RingEncryptor, iana};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};
use zeroize::{Zeroize, Zeroizing};

#[cfg(test)]
use super::{random_id, unix_seconds};

#[derive(Clone, Default, Deserialize, Serialize)]
pub(super) struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub expires_at: u64,
    pub scopes: Vec<String>,
}
impl Drop for Tokens {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
        self.id_token.zeroize();
    }
}
#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Profile {
    pub id: String,
    pub label: String,
    pub issuer: String,
    pub subject: String,
    pub client_id: String,
    pub email: Option<String>,
    pub tokens: Option<Tokens>,
}
#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Accounts {
    pub version: u32,
    pub host_id: String,
    pub active: Option<String>,
    #[serde(default)]
    pub pending_client_id: Option<String>,
    pub profiles: BTreeMap<String, Profile>,
}
impl Default for Accounts {
    fn default() -> Self {
        Self {
            version: 1,
            host_id: host_id(),
            active: None,
            pending_client_id: None,
            profiles: BTreeMap::new(),
        }
    }
}
fn host_id() -> String {
    let mut bytes = rand::random::<[u8; 16]>();
    bytes[6] = (bytes[6] & 15) | 64;
    bytes[8] = (bytes[8] & 63) | 128;
    let hex: String = bytes.iter().map(|v| format!("{v:02x}")).collect();
    format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

pub(super) struct CredentialStore {
    path: PathBuf,
    key: Zeroizing<[u8; 32]>,
    _lock: File,
}
impl CredentialStore {
    pub fn open(home: &Path, secret: &[u8; 32]) -> Result<Self, BoxError> {
        let dir = home.join("chatgpt");
        std::fs::create_dir_all(&dir)?;
        crate::util::fs::restrict_secret_dir_permissions(&dir)?;
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join("session.lock"))?;
        lock.try_lock()
            .map_err(|_| "ChatGPT credentials are already owned by another Anda process")?;
        let mut key = Zeroizing::new([0u8; 32]);
        hkdf::Hkdf::<sha2::Sha256>::new(Some(b"anda.chatgpt.credentials.v1"), secret)
            .expand(b"oauth-session-encryption", key.as_mut())
            .map_err(|_| "credential key derivation failed")?;
        Ok(Self {
            path: dir.join("accounts.cose"),
            key,
            _lock: lock,
        })
    }
    pub fn load(&self) -> Result<Accounts, BoxError> {
        let data = match std::fs::read(&self.path) {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let accounts = Accounts::default();
                self.save(&accounts)?;
                return Ok(accounts);
            }
            Err(e) => return Err(e.into()),
        };
        let cipher = RingEncryptor::new(iana::AlgorithmA256GCM, &*self.key, None)?;
        let mut msg =
            Encrypt0Message::decrypt_and_decode(&cipher, &data, Some(b"anda.chatgpt.accounts.v1"))?;
        let plain = Zeroizing::new(msg.payload.take().ok_or("empty credential payload")?);
        let accounts: Accounts = serde_json::from_slice(&plain)?;
        if accounts.version != 1 {
            return Err("unsupported ChatGPT credential version".into());
        }
        Ok(accounts)
    }
    pub fn save(&self, accounts: &Accounts) -> Result<(), BoxError> {
        let cipher = RingEncryptor::new(iana::AlgorithmA256GCM, &*self.key, None)?;
        let mut msg = Encrypt0Message::new(Some(serde_json::to_vec(accounts)?));
        msg.unprotected.set_iv(rand::random::<[u8; 12]>().to_vec());
        let encrypted = msg.encrypt_and_encode(&cipher, Some(b"anda.chatgpt.accounts.v1"));
        if let Some(plain) = msg.payload.as_mut() {
            plain.zeroize();
        }
        atomic_write(&self.path, &encrypted?)
    }
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), BoxError> {
    let parent = path.parent().ok_or("missing parent directory")?;
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::Builder::new()
        .prefix(".anda-")
        .tempfile_in(parent)?;
    crate::util::fs::restrict_secret_file_permissions(temp.path())?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chatgpt_store_encrypts_and_locks_session() {
        let home = tempfile::tempdir().unwrap();
        let store = CredentialStore::open(home.path(), &[7; 32]).unwrap();
        let mut accounts = store.load().unwrap();
        let host = accounts.host_id.clone();
        accounts.profiles.insert(
            "test".into(),
            Profile {
                id: "test".into(),
                label: "Test".into(),
                issuer: "issuer".into(),
                subject: "subject".into(),
                client_id: "client".into(),
                email: None,
                tokens: Some(Tokens {
                    access_token: "secret-access-token".into(),
                    expires_at: unix_seconds() + 3600,
                    refresh_token: String::new(),
                    id_token: String::new(),
                    scopes: Vec::new(),
                }),
            },
        );
        store.save(&accounts).unwrap();
        assert!(
            !String::from_utf8_lossy(&std::fs::read(&store.path).unwrap())
                .contains("secret-access-token")
        );
        assert!(CredentialStore::open(home.path(), &[7; 32]).is_err());
        drop(store);
        let store = CredentialStore::open(home.path(), &[7; 32]).unwrap();
        assert_eq!(store.load().unwrap().host_id, host);
        drop(store);
        assert!(
            CredentialStore::open(home.path(), &[8; 32])
                .unwrap()
                .load()
                .is_err()
        );
        assert_ne!(random_id(), random_id());
    }
}
