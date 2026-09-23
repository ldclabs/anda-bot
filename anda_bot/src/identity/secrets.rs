use anda_core::BoxError;
use cbor2::{Cbor, from_slice, to_canonical_vec};
use ic_auth_types::{ByteArrayB64, ByteBufB64, BytesB64};
use std::{io::Read as _, path::Path, str::FromStr, sync::Arc};
use zeroize::Zeroizing;

use super::{
    IDENTITY_KEY_STORE_UNAVAILABLE_HINT,
    ed25519::random_ed25519_privkey,
    files::{
        read_identity_secret_file, remove_legacy_identity_key, write_ed25519_secret_file_blocking,
    },
    refs::IdentityKeyRef,
    store::{IdentityKeyStore, is_identity_key_store_unavailable},
};

#[derive(Clone, PartialEq, Eq)]
pub struct LoadedIdentitySecret {
    pub secret: [u8; 32],
    pub location: String,
}

impl LoadedIdentitySecret {
    pub(super) fn new(secret: [u8; 32], location: String) -> Self {
        Self { secret, location }
    }
}

impl Drop for LoadedIdentitySecret {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.secret);
    }
}

impl Drop for LocalIdentitySecrets {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut *self.daemon);
        zeroize::Zeroize::zeroize(&mut *self.owner);
    }
}

#[derive(Clone, PartialEq, Eq, Cbor)]
pub struct LocalIdentitySecrets {
    #[serde(skip, default)]
    pub location: String,

    #[cbor(key = 1)]
    pub daemon: ByteArrayB64<32>,
    #[cbor(key = 2)]
    pub owner: ByteArrayB64<32>,
}

impl std::fmt::Debug for LoadedIdentitySecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedIdentitySecret")
            .field("location", &self.location)
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl std::fmt::Debug for LocalIdentitySecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalIdentitySecrets")
            .field("location", &self.location)
            .field("daemon", &"<redacted>")
            .field("owner", &"<redacted>")
            .finish()
    }
}

impl LocalIdentitySecrets {
    pub fn to_encoded(&self) -> Result<Zeroizing<String>, BoxError> {
        let data = Zeroizing::new(to_canonical_vec(self)?);
        Ok(Zeroizing::new(BytesB64::from_slice(&data).to_string()))
    }

    pub fn from_str(input: &str) -> Result<LocalIdentitySecrets, BoxError> {
        let data = Zeroizing::new(ByteBufB64::from_str(input.trim())?.0);
        Ok(from_slice(&data)?)
    }
}

fn decode_bundle(bundle: Vec<u8>, location: String) -> Result<LocalIdentitySecrets, BoxError> {
    let bundle = Zeroizing::new(bundle);
    let mut secrets: LocalIdentitySecrets = from_slice(&bundle)?;
    secrets.location = location;
    Ok(secrets)
}

pub async fn load_or_init_local_identity_secrets_with_store(
    home: &Path,
    store: Arc<dyn IdentityKeyStore>,
) -> Result<LocalIdentitySecrets, BoxError> {
    let home = home.to_path_buf();
    tokio::task::spawn_blocking(move || load_or_init_local_identity_secrets_blocking(&home, store))
        .await?
}

/// Explicit first-time initialization for installations without an OS keyring.
/// Ordinary reads must never infer that an unavailable store is empty.
pub async fn init_local_identity_files_with_store(
    home: &Path,
    store: Arc<dyn IdentityKeyStore>,
) -> Result<LocalIdentitySecrets, BoxError> {
    let home = home.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let daemon_ref = IdentityKeyRef::daemon(&home);
        let owner_ref = IdentityKeyRef::owner(&home);
        let mut existing = Vec::with_capacity(2);
        for key_ref in [&daemon_ref, &owner_ref] {
            match read_identity_secret_file(key_ref) {
                Ok(secret) => existing.push(Some(secret)),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => existing.push(None),
                Err(err) => return Err(err.into()),
            }
        }
        if existing.iter().any(Option::is_none) {
            // A populated installation needs recovery, not new root keys.
            for dir in [home.join("db"), home.join("credentials")] {
                match std::fs::read_dir(dir) {
                    Ok(mut entries) => {
                        if entries.next().transpose()?.is_some() {
                            return Err("existing local data found; restore the original daemon and owner keys instead of initializing replacements".into());
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => return Err(err.into()),
                }
            }
            for key_ref in [IdentityKeyRef::bundle(&home), daemon_ref.clone(), owner_ref.clone()] {
                match store.get_secret_bytes(key_ref.account()) {
                    Ok(Some(secret)) => {
                        let _secret = Zeroizing::new(secret);
                        return Err("existing identities found in the credential store; use `anda user export` to export the original daemon and owner keys".into());
                    }
                    Ok(None) => {}
                    Err(err) if is_identity_key_store_unavailable(err.as_ref()) => break,
                    Err(err) => return Err(err),
                }
            }
            for (key_ref, loaded) in [&daemon_ref, &owner_ref].into_iter().zip(&existing) {
                if loaded.is_none() {
                    let secret = Zeroizing::new(random_ed25519_privkey());
                    write_ed25519_secret_file_blocking(key_ref.legacy_path(), &secret, false)?;
                }
            }
        }
        load_local_identity_secrets_from_files(&home)
    }).await?
}

pub async fn read_local_identity_secrets_from_stdin() -> Result<LocalIdentitySecrets, BoxError> {
    tokio::task::spawn_blocking(move || {
        let mut input = Zeroizing::new(String::new());
        std::io::stdin().read_to_string(&mut input)?;
        LocalIdentitySecrets::from_str(&input)
    })
    .await?
}

fn load_or_init_local_identity_secrets_blocking(
    home: &Path,
    store: Arc<dyn IdentityKeyStore>,
) -> Result<LocalIdentitySecrets, BoxError> {
    let bundle_ref = IdentityKeyRef::bundle(home);
    match store.get_secret_bytes(bundle_ref.account()) {
        Ok(Some(bundle)) => return decode_bundle(bundle, store.location(bundle_ref.account())),
        Ok(None) => {}
        Err(err) if is_identity_key_store_unavailable(err.as_ref()) => {
            warn_local_identity_key_file_fallback(home, &err);
            return load_local_identity_secrets_from_files(home);
        }
        Err(err) => return Err(err),
    }

    let daemon_ref = IdentityKeyRef::daemon(home);
    let owner_ref = IdentityKeyRef::owner(home);
    // Legacy entries are migration sources only. Generate missing keys in
    // memory and publish both identities together in a single bundle.
    let pending: Result<LocalIdentitySecrets, BoxError> = (|| {
        let (daemon, daemon_file) = local_secret_for_bundle(&daemon_ref, store.as_ref())?;
        let (owner, owner_file) = local_secret_for_bundle(&owner_ref, store.as_ref())?;
        let secrets = LocalIdentitySecrets {
            location: store.location(bundle_ref.account()),
            daemon: daemon.secret.into(),
            owner: owner.secret.into(),
        };
        let bundle = Zeroizing::new(to_canonical_vec(&secrets)?);
        store.put_secret_bytes(bundle_ref.account(), &bundle, false)?;
        // Stores verify persistence before returning success. Never remove a
        // migration source before the complete bundle has been saved.
        if daemon_file {
            remove_legacy_identity_key(&daemon_ref);
        }
        if owner_file {
            remove_legacy_identity_key(&owner_ref);
        }
        Ok(secrets)
    })();
    match pending {
        Ok(secrets) => Ok(secrets),
        Err(err) if is_identity_key_store_unavailable(err.as_ref()) => {
            warn_local_identity_key_file_fallback(home, &err);
            load_local_identity_secrets_from_files(home)
        }
        Err(err) => {
            // Another initializer may have published the bundle first.
            if let Some(bundle) = store.get_secret_bytes(bundle_ref.account())? {
                return decode_bundle(bundle, store.location(bundle_ref.account()));
            }
            Err(err)
        }
    }
}

fn load_local_identity_secrets_from_files(home: &Path) -> Result<LocalIdentitySecrets, BoxError> {
    // An unavailable store may still hold the real identities. Reading a
    // fallback must never generate replacements for inaccessible keys.
    let daemon = read_existing_identity_secret_file(&IdentityKeyRef::daemon(home))?;
    let owner = read_existing_identity_secret_file(&IdentityKeyRef::owner(home))?;
    Ok(LocalIdentitySecrets {
        daemon: daemon.secret.into(),
        owner: owner.secret.into(),
        location: format!("{}, {}", daemon.location, owner.location),
    })
}

fn local_secret_for_bundle(
    key_ref: &IdentityKeyRef,
    store: &dyn IdentityKeyStore,
) -> Result<(LoadedIdentitySecret, bool), BoxError> {
    if let Some(secret) = store.get_secret(key_ref.account())? {
        return Ok((
            LoadedIdentitySecret::new(secret, store.location(key_ref.account())),
            false,
        ));
    }
    match read_identity_secret_file(key_ref) {
        Ok(secret) => Ok((secret, true)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok((
            LoadedIdentitySecret::new(random_ed25519_privkey(), String::new()),
            false,
        )),
        Err(err) => Err(err.into()),
    }
}

pub async fn load_identity_secret_with_location_with_store(
    key_ref: &IdentityKeyRef,
    store: Arc<dyn IdentityKeyStore>,
) -> Result<LoadedIdentitySecret, BoxError> {
    let key_ref = key_ref.clone();
    tokio::task::spawn_blocking(move || {
        if key_ref.is_daemon() || key_ref.is_owner() {
            load_local_identity_secret_blocking(&key_ref, store)
        } else {
            load_identity_secret_blocking(&key_ref, store)
        }
    })
    .await?
}

/// With `overwrite: false` an existing secret is an error. `anda user create`
/// passes `overwrite: true` so an orphan key left behind by an earlier run
/// whose config write failed does not block retries with "already exists".
pub async fn write_identity_secret_with_store(
    key_ref: &IdentityKeyRef,
    secret: &[u8; 32],
    overwrite: bool,
    store: Arc<dyn IdentityKeyStore>,
) -> Result<String, BoxError> {
    if key_ref.is_daemon() || key_ref.is_owner() {
        return Err("cannot write daemon or owner identity keys".into());
    }

    let key_ref = key_ref.clone();
    let secret = Zeroizing::new(*secret);
    tokio::task::spawn_blocking(move || {
        write_identity_secret_blocking(&key_ref, &secret, overwrite, store)
    })
    .await?
}

fn write_identity_secret_blocking(
    key_ref: &IdentityKeyRef,
    secret: &[u8; 32],
    overwrite: bool,
    store: Arc<dyn IdentityKeyStore>,
) -> Result<String, BoxError> {
    match store.put_secret(key_ref.account(), secret, overwrite) {
        Ok(()) => Ok(store.location(key_ref.account())),
        Err(err) if is_identity_key_store_unavailable(err.as_ref()) => {
            warn_identity_key_file_fallback(key_ref, &err);
            write_ed25519_secret_file_blocking(key_ref.legacy_path(), secret, overwrite)?;
            Ok(key_ref.fallback_location())
        }
        Err(err) => Err(err),
    }
}

fn load_identity_secret_blocking(
    key_ref: &IdentityKeyRef,
    store: Arc<dyn IdentityKeyStore>,
) -> Result<LoadedIdentitySecret, BoxError> {
    match store.get_secret(key_ref.account()) {
        Ok(Some(secret)) => {
            return Ok(LoadedIdentitySecret::new(
                secret,
                store.location(key_ref.account()),
            ));
        }
        Ok(None) => {}
        Err(err) if is_identity_key_store_unavailable(err.as_ref()) => {
            warn_identity_key_file_fallback(key_ref, &err);
            return read_existing_identity_secret_file(key_ref);
        }
        Err(err) => return Err(err),
    }

    read_existing_identity_secret_file(key_ref)
}

fn load_local_identity_secret_blocking(
    key_ref: &IdentityKeyRef,
    store: Arc<dyn IdentityKeyStore>,
) -> Result<LoadedIdentitySecret, BoxError> {
    let bundle_ref = IdentityKeyRef::bundle(key_ref.home());
    match store.get_secret_bytes(bundle_ref.account()) {
        Ok(Some(bundle)) => {
            let secrets = decode_bundle(bundle, store.location(bundle_ref.account()))?;
            let secret = if key_ref.is_daemon() {
                *secrets.daemon
            } else {
                *secrets.owner
            };
            return Ok(LoadedIdentitySecret::new(secret, secrets.location.clone()));
        }
        Ok(None) => {}
        Err(err) if is_identity_key_store_unavailable(err.as_ref()) => {
            warn_identity_key_file_fallback(key_ref, &err);
            return read_existing_identity_secret_file(key_ref);
        }
        Err(err) => return Err(err),
    }

    load_identity_secret_blocking(key_ref, store)
}

fn read_existing_identity_secret_file(
    key_ref: &IdentityKeyRef,
) -> Result<LoadedIdentitySecret, BoxError> {
    match read_identity_secret_file(key_ref) {
        Ok(secret) => Ok(secret),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(format!(
            "identity key not found in {} or {}; unlock the credential store or restore the original keys. For a NEW installation without an OS keyring, run `anda user init-file-keys`; create trusted-user keys before exporting",
            key_ref.location(),
            key_ref.fallback_location()
        )
        .into()),
        Err(err) => Err(err.into()),
    }
}

fn warn_identity_key_file_fallback(key_ref: &IdentityKeyRef, err: &BoxError) {
    let key_path = key_ref.legacy_path();
    log::warn!(
        name = "daemon";
        "{}; using {}. {}",
        err,
        key_ref.fallback_location(),
        IDENTITY_KEY_STORE_UNAVAILABLE_HINT
    );
    eprintln!(
        "warning: {err}; using private key file {}.\nwarning: {}",
        key_path.display(),
        IDENTITY_KEY_STORE_UNAVAILABLE_HINT
    );
}

fn warn_local_identity_key_file_fallback(home: &Path, err: &BoxError) {
    let daemon_ref = IdentityKeyRef::daemon(home);
    let owner_ref = IdentityKeyRef::owner(home);
    log::warn!(
        name = "daemon";
        "{}; using {} and {}. {}",
        err,
        daemon_ref.fallback_location(),
        owner_ref.fallback_location(),
        IDENTITY_KEY_STORE_UNAVAILABLE_HINT
    );
    eprintln!(
        "warning: {err}; using private key files {} and {}.\nwarning: {}",
        daemon_ref.legacy_path().display(),
        owner_ref.legacy_path().display(),
        IDENTITY_KEY_STORE_UNAVAILABLE_HINT
    );
}
