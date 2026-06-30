use anda_core::BoxError;
use std::{error::Error, sync::Arc};

pub const IDENTITY_KEYRING_SERVICE: &str = "anda.bot.identity";

pub trait IdentityKeyStore: Send + Sync {
    fn location(&self, account: &str) -> String {
        format!("credential store account: {account}")
    }

    fn get_secret_bytes(&self, account: &str) -> Result<Option<Vec<u8>>, BoxError>;
    fn put_secret_bytes(
        &self,
        account: &str,
        secret: &[u8],
        overwrite: bool,
    ) -> Result<(), BoxError>;

    fn get_secret(&self, account: &str) -> Result<Option<[u8; 32]>, BoxError> {
        self.get_secret_bytes(account)?
            .map(|secret| {
                secret.try_into().map_err(|secret: Vec<u8>| {
                    format!(
                        "identity keyring entry {account} has invalid length {}; expected 32 bytes",
                        secret.len()
                    )
                    .into()
                })
            })
            .transpose()
    }

    fn put_secret(
        &self,
        account: &str,
        secret: &[u8; 32],
        overwrite: bool,
    ) -> Result<(), BoxError> {
        self.put_secret_bytes(account, secret, overwrite)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OsIdentityKeyStore;

impl IdentityKeyStore for OsIdentityKeyStore {
    fn location(&self, account: &str) -> String {
        format!("system keyring account: {account}")
    }

    fn get_secret_bytes(&self, account: &str) -> Result<Option<Vec<u8>>, BoxError> {
        let entry = keyring_entry(account, "read identity key")?;
        match entry.get_secret() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(classify_keyring_error(err, "read identity key")),
        }
    }

    fn put_secret_bytes(
        &self,
        account: &str,
        secret: &[u8],
        overwrite: bool,
    ) -> Result<(), BoxError> {
        if !overwrite && self.get_secret_bytes(account)?.is_some() {
            return Err(format!("identity key already exists in system keyring: {account}").into());
        }

        let entry = keyring_entry(account, "write identity key")?;
        entry
            .set_secret(secret)
            .map_err(|err| classify_keyring_error(err, "write identity key"))?;
        let stored = self
            .get_secret_bytes(account)?
            .ok_or_else(|| format!("identity keyring write did not persist: {account}"))?;
        if stored != secret {
            return Err(format!("identity keyring verification failed: {account}").into());
        }
        Ok(())
    }
}

fn keyring_entry(account: &str, operation: &'static str) -> Result<keyring::Entry, BoxError> {
    keyring::Entry::new(IDENTITY_KEYRING_SERVICE, account)
        .map_err(|err| classify_keyring_error(err, operation))
}

fn classify_keyring_error(err: keyring::Error, operation: &'static str) -> BoxError {
    if is_secret_service_unavailable(&err) {
        return Box::new(IdentityKeyStoreUnavailable {
            operation,
            source: err.to_string(),
        });
    }

    err.into()
}

fn is_secret_service_unavailable(err: &keyring::Error) -> bool {
    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    ))]
    {
        matches!(
            err,
            keyring::Error::NoDefaultStore
                | keyring::Error::NoStorageAccess(_)
                | keyring::Error::PlatformFailure(_)
        )
    }

    #[cfg(not(all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    )))]
    {
        let _ = err;
        false
    }
}

#[derive(Debug)]
pub(super) struct IdentityKeyStoreUnavailable {
    pub(super) operation: &'static str,
    pub(super) source: String,
}

impl std::fmt::Display for IdentityKeyStoreUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OS secure credential store is unavailable while trying to {}: {}",
            self.operation, self.source
        )
    }
}

impl Error for IdentityKeyStoreUnavailable {}

pub(super) fn is_identity_key_store_unavailable(err: &(dyn Error + Send + Sync + 'static)) -> bool {
    err.downcast_ref::<IdentityKeyStoreUnavailable>().is_some()
}

pub fn os_identity_key_store() -> Arc<dyn IdentityKeyStore> {
    Arc::new(OsIdentityKeyStore)
}

#[cfg(test)]
#[derive(Debug, Default)]
pub struct MemoryIdentityKeyStore {
    secrets: std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>,
}

#[cfg(test)]
impl MemoryIdentityKeyStore {
    pub fn get_for_test(&self, account: &str) -> Option<[u8; 32]> {
        self.secrets
            .lock()
            .unwrap()
            .get(account)
            .and_then(|secret| secret.as_slice().try_into().ok())
    }

    pub fn get_bytes_for_test(&self, account: &str) -> Option<Vec<u8>> {
        self.secrets.lock().unwrap().get(account).cloned()
    }
}

#[cfg(test)]
impl IdentityKeyStore for MemoryIdentityKeyStore {
    fn get_secret_bytes(&self, account: &str) -> Result<Option<Vec<u8>>, BoxError> {
        Ok(self.secrets.lock().unwrap().get(account).cloned())
    }

    fn put_secret_bytes(
        &self,
        account: &str,
        secret: &[u8],
        overwrite: bool,
    ) -> Result<(), BoxError> {
        let mut secrets = self.secrets.lock().unwrap();
        if !overwrite && secrets.contains_key(account) {
            return Err(format!("identity key already exists in system keyring: {account}").into());
        }
        secrets.insert(account.to_string(), secret.to_vec());
        Ok(())
    }
}
