use anda_core::BoxError;
use cbor2::{Cbor, from_slice, to_canonical_vec};
use ic_auth_types::{ByteArrayB64, ByteBufB64};
use std::{io::Read as _, path::Path, str::FromStr, sync::Arc};

use super::{
    IDENTITY_KEY_STORE_UNAVAILABLE_HINT,
    ed25519::{parse_ed25519_privkey, random_ed25519_privkey},
    files::{
        read_identity_secret_file, remove_legacy_identity_key, write_ed25519_secret_file_blocking,
    },
    refs::IdentityKeyRef,
    store::{IdentityKeyStore, is_identity_key_store_unavailable},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedIdentitySecret {
    pub secret: [u8; 32],
    pub location: String,
}

impl LoadedIdentitySecret {
    pub(super) fn new(secret: [u8; 32], location: String) -> Self {
        Self { secret, location }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Cbor)]
pub struct LocalIdentitySecrets {
    #[serde(skip, default)]
    pub location: String,

    #[cbor(key = 1)]
    pub daemon: ByteArrayB64<32>,
    #[cbor(key = 2)]
    pub owner: ByteArrayB64<32>,
}

impl LocalIdentitySecrets {
    pub fn to_bytes(&self) -> Result<ByteBufB64, BoxError> {
        let data = to_canonical_vec(&self)?;
        Ok(data.into())
    }

    pub fn from_str(input: &str) -> Result<LocalIdentitySecrets, BoxError> {
        let data = ByteBufB64::from_str(input.trim())?;
        let secrets: LocalIdentitySecrets = from_slice(data.as_slice())?;
        Ok(secrets)
    }
}

pub async fn load_or_init_local_identity_secrets_with_store(
    home: &Path,
    store: Arc<dyn IdentityKeyStore>,
) -> Result<LocalIdentitySecrets, BoxError> {
    let home = home.to_path_buf();
    tokio::task::spawn_blocking(move || load_or_init_local_identity_secrets_blocking(&home, store))
        .await?
}

pub async fn read_local_identity_secrets_from_stdin() -> Result<LocalIdentitySecrets, BoxError> {
    tokio::task::spawn_blocking(move || {
        let mut input = String::new();
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
        Ok(Some(bundle)) => {
            let mut secrets: LocalIdentitySecrets = from_slice(&bundle)?;
            secrets.location = bundle_ref.location();
            return Ok(secrets);
        }
        Ok(None) => {}
        Err(err) if is_identity_key_store_unavailable(err.as_ref()) => {
            warn_local_identity_key_file_fallback(home, &err);
            return load_or_init_local_identity_secrets_from_files(home);
        }
        Err(err) => return Err(err),
    }

    let daemon_ref = IdentityKeyRef::daemon(home);
    let owner_ref = IdentityKeyRef::owner(home);
    let daemon = load_or_init_identity_secret_blocking(&daemon_ref, store.clone())?;
    let owner = load_or_init_identity_secret_blocking(&owner_ref, store.clone())?;
    let mut secrets = LocalIdentitySecrets {
        location: bundle_ref.location(),
        daemon: daemon.secret.into(),
        owner: owner.secret.into(),
    };
    let bundle = to_canonical_vec(&secrets)?;
    match store.put_secret_bytes(bundle_ref.account(), &bundle, false) {
        Ok(()) => {
            log::warn!(
                name = "daemon";
                "migrated local daemon and owner identities to {}",
                bundle_ref.location()
            );
            Ok(secrets)
        }
        Err(err) if is_identity_key_store_unavailable(err.as_ref()) => {
            warn_local_identity_key_file_fallback(home, &err);
            secrets.location = format!("{}, {}", daemon.location, owner.location);
            Ok(secrets)
        }
        Err(err) => {
            if let Ok(Some(bundle)) = store.get_secret_bytes(bundle_ref.account()) {
                let mut secrets: LocalIdentitySecrets = from_slice(&bundle)?;
                secrets.location = bundle_ref.location();
                return Ok(secrets);
            }
            Err(err)
        }
    }
}

fn load_or_init_local_identity_secrets_from_files(
    home: &Path,
) -> Result<LocalIdentitySecrets, BoxError> {
    let daemon_ref = IdentityKeyRef::daemon(home);
    let owner_ref = IdentityKeyRef::owner(home);
    let daemon = load_or_init_identity_secret_file(&daemon_ref)?;
    let owner = load_or_init_identity_secret_file(&owner_ref)?;
    Ok(LocalIdentitySecrets {
        daemon: daemon.secret.into(),
        owner: owner.secret.into(),
        location: format!("{}, {}", daemon.location, owner.location),
    })
}

pub async fn load_identity_secret_with_location_with_store(
    key_ref: &IdentityKeyRef,
    store: Arc<dyn IdentityKeyStore>,
) -> Result<LoadedIdentitySecret, BoxError> {
    if key_ref.is_daemon() || key_ref.is_owner() {
        let key_ref = key_ref.clone();
        return tokio::task::spawn_blocking(move || {
            load_local_identity_secret_blocking(&key_ref, store)
        })
        .await?;
    }

    let key_ref = key_ref.clone();
    tokio::task::spawn_blocking(move || load_identity_secret_blocking(&key_ref, store)).await?
}

pub async fn write_identity_secret_with_store(
    key_ref: &IdentityKeyRef,
    secret: &[u8; 32],
    store: Arc<dyn IdentityKeyStore>,
) -> Result<String, BoxError> {
    if key_ref.is_daemon() || key_ref.is_owner() {
        return Err("cannot write daemon or owner identity keys".into());
    }

    let key_ref = key_ref.clone();
    let secret = *secret;
    tokio::task::spawn_blocking(move || {
        write_identity_secret_blocking(&key_ref, &secret, false, store)
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
            let secrets: LocalIdentitySecrets = from_slice(&bundle)?;
            if key_ref.is_daemon() {
                return Ok(LoadedIdentitySecret::new(
                    *secrets.daemon,
                    bundle_ref.location(),
                ));
            }
            return Ok(LoadedIdentitySecret::new(
                *secrets.owner,
                bundle_ref.location(),
            ));
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

fn load_or_init_identity_secret_blocking(
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
            return load_or_init_identity_secret_file(key_ref);
        }
        Err(err) => return Err(err),
    }

    match std::fs::read_to_string(key_ref.legacy_path()) {
        Ok(content) => {
            let secret = parse_ed25519_privkey(content.trim())?;
            match store.put_secret(key_ref.account(), &secret, false) {
                Ok(()) => {
                    log::warn!(
                        name = "daemon";
                        "migrated ED25519 private key from {:?} to {}",
                        key_ref.legacy_path(),
                        store.location(key_ref.account())
                    );
                    remove_legacy_identity_key(key_ref);
                }
                Err(err) if is_identity_key_store_unavailable(err.as_ref()) => {
                    warn_identity_key_file_fallback(key_ref, &err);
                    return Ok(LoadedIdentitySecret::new(
                        secret,
                        key_ref.fallback_location(),
                    ));
                }
                Err(err) => return Err(err),
            }
            Ok(LoadedIdentitySecret::new(
                secret,
                store.location(key_ref.account()),
            ))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            log::warn!(
                name = "daemon";
                "ED25519 private key not found in {}, generating a new one",
                store.location(key_ref.account())
            );
            let secret = random_ed25519_privkey();
            match store.put_secret(key_ref.account(), &secret, false) {
                Ok(()) => {}
                Err(err) if is_identity_key_store_unavailable(err.as_ref()) => {
                    warn_identity_key_file_fallback(key_ref, &err);
                    write_ed25519_secret_file_blocking(key_ref.legacy_path(), &secret, false)?;
                    return Ok(LoadedIdentitySecret::new(
                        secret,
                        key_ref.fallback_location(),
                    ));
                }
                Err(err) => return Err(err),
            }
            Ok(LoadedIdentitySecret::new(
                secret,
                store.location(key_ref.account()),
            ))
        }
        Err(err) => Err(err.into()),
    }
}

fn load_or_init_identity_secret_file(
    key_ref: &IdentityKeyRef,
) -> Result<LoadedIdentitySecret, BoxError> {
    match read_identity_secret_file(key_ref) {
        Ok(secret) => Ok(secret),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            log::warn!(
                name = "daemon";
                "ED25519 private key not found at {:?}, generating a new fallback file key",
                key_ref.legacy_path()
            );
            let secret = random_ed25519_privkey();
            write_ed25519_secret_file_blocking(key_ref.legacy_path(), &secret, false)?;
            Ok(LoadedIdentitySecret::new(
                secret,
                key_ref.fallback_location(),
            ))
        }
        Err(err) => Err(err.into()),
    }
}

fn read_existing_identity_secret_file(
    key_ref: &IdentityKeyRef,
) -> Result<LoadedIdentitySecret, BoxError> {
    match read_identity_secret_file(key_ref) {
        Ok(secret) => Ok(secret),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(format!(
            "identity key not found in {} or {}; start Anda first for daemon/owner identities, or create the trusted-user private key before exporting",
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
