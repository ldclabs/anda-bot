use anda_core::BoxError;
use cbor2::{Cbor, to_canonical_vec};
use cose2::Encrypt0Message;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use super::{
    ed25519::{decode_ed25519_privkey_cose_key, encode_ed25519_privkey_cose_key},
    files::{read_identity_secret_file, remove_legacy_identity_key, write_private_binary_file},
    iana,
    refs::{IdentityKeyRef, hex_bytes, identity_home_namespace},
    store::{IdentityKeyStore, is_identity_key_store_unavailable},
};

const LOCAL_CREDENTIAL_STORE_SALT_PREFIX: &[u8] = b"anda.bot.local-credential-store.v1";
const LOCAL_CREDENTIAL_STORE_INFO_PREFIX: &[u8] = b"identity-ed25519-cose-key/v1\0";
const LOCAL_CREDENTIAL_STORE_AAD_PURPOSE: &str = "identity-ed25519-cose-key";
const LOCAL_CREDENTIAL_STORE_VERSION: u64 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Cbor)]
struct LocalCredentialAad {
    #[cbor(key = 1)]
    version: u64,
    #[cbor(key = 2)]
    purpose: String,
    #[cbor(key = 3)]
    account: String,
    #[cbor(key = 4)]
    namespace: String,
}

#[derive(Clone)]
pub struct LocalEncryptedIdentityKeyStore {
    pub(super) home: PathBuf,
    daemon_secret: [u8; 32],
    namespace: String,
    migration_store: Option<Arc<dyn IdentityKeyStore>>,
}

impl LocalEncryptedIdentityKeyStore {
    #[cfg(test)]
    pub(super) fn new(home: &Path, daemon_secret: [u8; 32]) -> Self {
        Self::with_migration_store(home, daemon_secret, None)
    }

    pub fn with_migration_store(
        home: &Path,
        daemon_secret: [u8; 32],
        migration_store: Option<Arc<dyn IdentityKeyStore>>,
    ) -> Self {
        Self {
            home: home.to_path_buf(),
            daemon_secret,
            namespace: identity_home_namespace(home),
            migration_store,
        }
    }

    pub(super) fn root(&self) -> PathBuf {
        self.home.join("credentials").join("v1").join("identity")
    }

    pub(super) fn credential_path(&self, account: &str) -> PathBuf {
        let digest = <sha2::Sha256 as sha2::Digest>::digest(
            [
                b"anda.local-credential.path.v1\0".as_slice(),
                account.as_bytes(),
            ]
            .concat(),
        );
        self.root().join(format!("{}.cose", hex_bytes(&digest)))
    }

    fn trusted_user_key_ref(&self, account: &str) -> Option<IdentityKeyRef> {
        let prefix = format!("v1:{}:user:", self.namespace);
        let id = account.strip_prefix(&prefix)?;
        (!id.is_empty()).then(|| IdentityKeyRef::trusted_user(&self.home, id))
    }

    fn derive_content_key(&self, account: &str) -> Result<[u8; 32], BoxError> {
        let mut salt =
            Vec::with_capacity(LOCAL_CREDENTIAL_STORE_SALT_PREFIX.len() + 1 + self.namespace.len());
        salt.extend_from_slice(LOCAL_CREDENTIAL_STORE_SALT_PREFIX);
        salt.push(0);
        salt.extend_from_slice(self.namespace.as_bytes());

        let mut info = Vec::with_capacity(LOCAL_CREDENTIAL_STORE_INFO_PREFIX.len() + account.len());
        info.extend_from_slice(LOCAL_CREDENTIAL_STORE_INFO_PREFIX);
        info.extend_from_slice(account.as_bytes());

        let hk = hkdf::Hkdf::<sha2::Sha256>::new(Some(&salt), &self.daemon_secret);
        let mut key = [0u8; 32];
        hk.expand(&info, &mut key)
            .map_err(|_err| "failed to derive local credential encryption key")?;
        Ok(key)
    }

    fn external_aad(&self, account: &str) -> Result<Vec<u8>, BoxError> {
        let aad = LocalCredentialAad {
            version: LOCAL_CREDENTIAL_STORE_VERSION,
            purpose: LOCAL_CREDENTIAL_STORE_AAD_PURPOSE.to_string(),
            account: account.to_string(),
            namespace: self.namespace.clone(),
        };
        Ok(to_canonical_vec(&aad)?)
    }

    fn read_encrypted_secret(&self, account: &str) -> Result<Option<[u8; 32]>, BoxError> {
        let path = self.credential_path(account);
        let data = match std::fs::read(&path) {
            Ok(data) => data,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };

        let key = self.derive_content_key(account)?;
        let encryptor = cose2::crypto::RingEncryptor::new(iana::AlgorithmA256GCM, &key, None)?;
        let aad = self.external_aad(account)?;
        let msg = Encrypt0Message::decrypt_and_decode(&encryptor, &data, Some(&aad))?;
        let plaintext = msg
            .payload
            .as_deref()
            .ok_or("local credential payload missing after decryption")?;
        Ok(Some(decode_ed25519_privkey_cose_key(plaintext)?))
    }

    fn write_encrypted_secret(
        &self,
        account: &str,
        secret: &[u8; 32],
        overwrite: bool,
    ) -> Result<(), BoxError> {
        if self.trusted_user_key_ref(account).is_none() {
            return Err(format!(
                "local encrypted identity store only supports trusted-user accounts: {account}"
            )
            .into());
        }

        let mut rng = rand::rng();
        let mut iv = [0u8; 12];
        rand::Rng::fill_bytes(&mut rng, &mut iv);

        let plaintext = encode_ed25519_privkey_cose_key(secret)?;
        let key = self.derive_content_key(account)?;
        let encryptor = cose2::crypto::RingEncryptor::new(iana::AlgorithmA256GCM, &key, None)?;
        let aad = self.external_aad(account)?;
        let mut msg = Encrypt0Message::new(Some(plaintext));
        msg.unprotected.set_iv(iv.to_vec());
        let encrypted = msg.encrypt_and_encode(&encryptor, Some(&aad))?;
        write_private_binary_file(
            &self.credential_path(account),
            &encrypted,
            overwrite,
            &self.home,
        )?;

        match self.read_encrypted_secret(account)? {
            Some(stored) if stored == *secret => Ok(()),
            Some(_) => Err(format!("local credential verification failed: {account}").into()),
            None => Err(format!("local credential write did not persist: {account}").into()),
        }
    }

    fn migrate_from_legacy_sources(&self, account: &str) -> Result<Option<Vec<u8>>, BoxError> {
        let Some(key_ref) = self.trusted_user_key_ref(account) else {
            return Ok(None);
        };

        if let Some(store) = &self.migration_store {
            match store.get_secret(account) {
                Ok(Some(secret)) => {
                    self.write_encrypted_secret(account, &secret, false)?;
                    log::warn!(
                        name = "daemon";
                        "migrated trusted-user ED25519 private key from {} to {}",
                        store.location(account),
                        self.location(account)
                    );
                    return Ok(Some(secret.to_vec()));
                }
                Ok(None) => {}
                Err(err) if is_identity_key_store_unavailable(err.as_ref()) => {}
                Err(err) => return Err(err),
            }
        }

        match read_identity_secret_file(&key_ref) {
            Ok(loaded) => {
                self.write_encrypted_secret(account, &loaded.secret, false)?;
                log::warn!(
                    name = "daemon";
                    "migrated trusted-user ED25519 private key from {} to {}",
                    loaded.location,
                    self.location(account)
                );
                remove_legacy_identity_key(&key_ref);
                Ok(Some(loaded.secret.to_vec()))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}

impl IdentityKeyStore for LocalEncryptedIdentityKeyStore {
    fn location(&self, account: &str) -> String {
        format!(
            "local encrypted credential file: {}",
            self.credential_path(account).display()
        )
    }

    fn get_secret_bytes(&self, account: &str) -> Result<Option<Vec<u8>>, BoxError> {
        match self.read_encrypted_secret(account)? {
            Some(secret) => Ok(Some(secret.to_vec())),
            None => self.migrate_from_legacy_sources(account),
        }
    }

    fn put_secret_bytes(
        &self,
        account: &str,
        secret: &[u8],
        overwrite: bool,
    ) -> Result<(), BoxError> {
        if secret.len() != 32 {
            return Err(format!(
                "local credential entry {account} has invalid length {}; expected 32 bytes",
                secret.len()
            )
            .into());
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(secret);
        self.write_encrypted_secret(account, &key, overwrite)
    }
}

pub fn local_encrypted_identity_key_store(
    home: &Path,
    daemon_secret: [u8; 32],
    migration_store: Option<Arc<dyn IdentityKeyStore>>,
) -> Arc<dyn IdentityKeyStore> {
    Arc::new(LocalEncryptedIdentityKeyStore::with_migration_store(
        home,
        daemon_secret,
        migration_store,
    ))
}
