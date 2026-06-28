use anda_core::{BoxError, Principal};
use anda_web3_client::client::{Identity, identity_from_secret};
use cbor2::{Cbor, from_slice, to_canonical_vec};
use cose2::{Key as CoseKey, Label, Sign1Message};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use ic_auth_types::{ByteArrayB64, ByteBufB64};
use ic_ed25519::PublicKey;
use std::{
    error::Error,
    fmt::Write as _,
    io::Read as _,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

pub use cose2::{cwt::Claims, iana};

#[cfg(test)]
use cose2::Value;

pub const IDENTITY_KEYRING_SERVICE: &str = "anda.bot.identity";
const IDENTITY_KEY_STORE_UNAVAILABLE_HINT: &str = "On Linux, start and unlock a Secret Service provider in a user D-Bus session, for example `gnome-keyring-daemon --start --components=secrets`, make sure DBUS_SESSION_BUS_ADDRESS is set for Anda, then restart Anda to use the OS keyring.";

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

    pub fn sign_cwt(&self, mut claims: Claims) -> Result<String, BoxError> {
        claims.subject = self.identity.sender().map(|s| s.to_string()).ok();
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

    let cose_key = okp_cose_key(data.as_slice())?;
    let public_key = cose_key
        .get_bytes(iana::OKPKeyParameterX)?
        .ok_or("missing public key")?;
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

    let cose_key = okp_cose_key(data.as_slice())?;
    let secret = cose_key
        .get_bytes(iana::OKPKeyParameterD)?
        .ok_or("missing secret key")?;
    let bytes: [u8; 32] = secret.try_into().map_err(|_err| "invalid key length")?;
    Ok(bytes)
}

fn okp_cose_key(data: &[u8]) -> Result<CoseKey, BoxError> {
    let key = CoseKey::from_slice(data)?;
    ensure_okp_key(&key)?;
    Ok(key)
}

fn ensure_okp_key(key: &CoseKey) -> Result<(), BoxError> {
    match key.kty()? {
        Some(Label::Int(iana::KeyTypeOKP)) => Ok(()),
        _ => Err("invalid key type".into()),
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
    let cose_bytes = cose_key.to_vec()?;
    Ok(ByteBufB64(cose_bytes).to_string())
}

pub fn encode_ed25519_pubkey(pubkey: &Ed25519PubKey) -> String {
    ByteBufB64(pubkey.as_bytes().to_vec()).to_string()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityKeyRef {
    account: String,
    home: PathBuf,
    legacy_path: PathBuf,
}

impl IdentityKeyRef {
    pub fn daemon(home: &Path) -> Self {
        Self::new(home, "daemon", home.join("keys").join("anda_bot.key"))
    }

    pub fn owner(home: &Path) -> Self {
        Self::new(home, "owner", home.join("keys").join("user.key"))
    }

    pub fn bundle(home: &Path) -> Self {
        Self::new(
            home,
            "local-identities",
            home.join("keys").join("local-identities.bundle"),
        )
    }

    pub fn trusted_user(home: &Path, id: &str) -> Self {
        Self::new(
            home,
            &format!("user:{id}"),
            home.join("keys").join("users").join(format!("{id}.key")),
        )
    }

    fn new(home: &Path, name: &str, legacy_path: PathBuf) -> Self {
        IdentityKeyRef {
            home: home.to_path_buf(),
            account: format!("v1:{}:{name}", identity_home_namespace(home)),
            legacy_path,
        }
    }

    pub fn is_daemon(&self) -> bool {
        self.account.ends_with(":daemon")
    }

    pub fn is_owner(&self) -> bool {
        self.account.ends_with(":owner")
    }

    pub fn account(&self) -> &str {
        &self.account
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn legacy_path(&self) -> &Path {
        &self.legacy_path
    }

    pub fn location(&self) -> String {
        format!("system keyring account: {}", self.account)
    }

    pub fn fallback_location(&self) -> String {
        format!("private key file: {}", self.legacy_path.display())
    }
}

fn identity_home_namespace(home: &Path) -> String {
    let path = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    let digest = <sha2::Sha256 as sha2::Digest>::digest(path.to_string_lossy().as_bytes());
    let mut namespace = String::with_capacity(32);
    for byte in digest.iter().take(16) {
        let _ = write!(&mut namespace, "{byte:02x}");
    }
    namespace
}

pub trait IdentityKeyStore: Send + Sync {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedIdentitySecret {
    pub secret: [u8; 32],
    pub location: String,
}

impl LoadedIdentitySecret {
    fn new(secret: [u8; 32], location: String) -> Self {
        Self { secret, location }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OsIdentityKeyStore;

impl IdentityKeyStore for OsIdentityKeyStore {
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
struct IdentityKeyStoreUnavailable {
    operation: &'static str,
    source: String,
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

fn is_identity_key_store_unavailable(err: &(dyn Error + Send + Sync + 'static)) -> bool {
    err.downcast_ref::<IdentityKeyStoreUnavailable>().is_some()
}

pub fn os_identity_key_store() -> Arc<dyn IdentityKeyStore> {
    Arc::new(OsIdentityKeyStore)
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
        Ok(()) => Ok(key_ref.location()),
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
        Ok(Some(secret)) => return Ok(LoadedIdentitySecret::new(secret, key_ref.location())),
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
        Ok(Some(secret)) => return Ok(LoadedIdentitySecret::new(secret, key_ref.location())),
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
                        key_ref.location()
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
            Ok(LoadedIdentitySecret::new(secret, key_ref.location()))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            log::warn!(
                name = "daemon";
                "ED25519 private key not found in {}, generating a new one",
                key_ref.location()
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
            Ok(LoadedIdentitySecret::new(secret, key_ref.location()))
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

fn read_identity_secret_file(
    key_ref: &IdentityKeyRef,
) -> Result<LoadedIdentitySecret, std::io::Error> {
    match std::fs::read_to_string(key_ref.legacy_path()) {
        Ok(content) => parse_ed25519_privkey(content.trim())
            .map(|secret| LoadedIdentitySecret::new(secret, key_ref.fallback_location()))
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
        Err(err) => Err(err),
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

fn remove_legacy_identity_key(key_ref: &IdentityKeyRef) {
    match std::fs::remove_file(key_ref.legacy_path()) {
        Ok(()) => log::warn!(
            name = "daemon";
            "removed legacy ED25519 private key file {:?}",
            key_ref.legacy_path()
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => log::warn!(
            name = "daemon";
            "migrated ED25519 private key to {}, but failed to remove legacy file {:?}: {err}",
            key_ref.location(),
            key_ref.legacy_path()
        ),
    }
}

pub async fn write_ed25519_secret_file(key_path: &Path, secret: &[u8; 32]) -> Result<(), BoxError> {
    let key_path = key_path.to_path_buf();
    let secret = *secret;
    tokio::task::spawn_blocking(move || {
        write_ed25519_secret_file_blocking(&key_path, &secret, false)
    })
    .await?
}

fn write_ed25519_secret_file_blocking(
    key_path: &Path,
    secret: &[u8; 32],
    overwrite: bool,
) -> Result<(), BoxError> {
    create_parent_dir_if_needed(key_path)?;

    let encoded = encode_ed25519_privkey(secret)?;
    write_private_text_file(key_path, &encoded, overwrite)
}

fn create_parent_dir_if_needed(path: &Path) -> Result<(), BoxError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn write_private_text_file(path: &Path, content: &str, overwrite: bool) -> Result<(), BoxError> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    if overwrite {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    use std::io::Write;
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(content.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

pub fn random_ed25519_privkey() -> [u8; 32] {
    let mut rng = rand::rng();
    let mut bytes = [0u8; 32];
    rand::Rng::fill_bytes(&mut rng, &mut bytes);
    bytes
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

#[cfg(test)]
pub async fn load_or_init_ed25519_secret(key_path: &Path) -> Result<[u8; 32], BoxError> {
    match super::text::read_text_file(key_path).await {
        Ok(content) => {
            let secret = parse_ed25519_privkey(content.trim())?;
            Ok(secret)
        }
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                return Err(err.into());
            }
            log::warn!(
                name = "daemon";
                "ED25519 private key not found at {:?}, generating a new one",
                key_path
            );
            let secret = random_ed25519_privkey();
            write_ed25519_secret_file(key_path, &secret).await?;
            Ok(secret)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: [u8; 32] = [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
        26, 27, 28, 29, 30, 31, 32,
    ];

    #[derive(Debug, Default)]
    struct UnavailableIdentityKeyStore;

    impl IdentityKeyStore for UnavailableIdentityKeyStore {
        fn get_secret_bytes(&self, _account: &str) -> Result<Option<Vec<u8>>, BoxError> {
            Err(unavailable_identity_store_error("read identity key"))
        }

        fn put_secret_bytes(
            &self,
            _account: &str,
            _secret: &[u8],
            _overwrite: bool,
        ) -> Result<(), BoxError> {
            Err(unavailable_identity_store_error("write identity key"))
        }
    }

    fn unavailable_identity_store_error(operation: &'static str) -> BoxError {
        Box::new(IdentityKeyStoreUnavailable {
            operation,
            source: "test Secret Service is unavailable".to_string(),
        })
    }

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
        let raw = encode_ed25519_pubkey(&key.pubkey());

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
    fn identity_key_accounts_are_scoped_by_home_and_kind() {
        let dir = tempfile::tempdir().unwrap();

        let daemon = IdentityKeyRef::daemon(dir.path());
        let owner = IdentityKeyRef::owner(dir.path());
        let alice = IdentityKeyRef::trusted_user(dir.path(), "alice");

        assert_ne!(daemon.account(), owner.account());
        assert_ne!(owner.account(), alice.account());
        assert!(daemon.account().contains(":daemon"));
        assert!(owner.account().contains(":owner"));
        assert!(alice.account().contains(":user:alice"));
        assert!(daemon.legacy_path().ends_with("keys/anda_bot.key"));
        assert!(owner.legacy_path().ends_with("keys/user.key"));
        assert!(alice.legacy_path().ends_with("keys/users/alice.key"));
    }

    #[tokio::test]
    async fn identity_key_store_uses_existing_file_when_secure_store_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let key_ref = IdentityKeyRef::owner(dir.path());
        write_ed25519_secret_file(key_ref.legacy_path(), &SECRET)
            .await
            .unwrap();
        let store = Arc::new(UnavailableIdentityKeyStore);

        let secret = load_or_init_local_identity_secrets_with_store(dir.path(), store)
            .await
            .unwrap();

        assert_eq!(*secret.owner, SECRET);
    }

    #[tokio::test]
    async fn write_identity_key_store_falls_back_to_file_when_secure_store_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let key_ref = IdentityKeyRef::trusted_user(dir.path(), "alice");
        let store = Arc::new(UnavailableIdentityKeyStore);

        let location = write_identity_secret_with_store(&key_ref, &SECRET, store)
            .await
            .unwrap();

        assert_eq!(location, key_ref.fallback_location());
        assert_eq!(
            parse_ed25519_privkey(
                std::fs::read_to_string(key_ref.legacy_path())
                    .unwrap()
                    .trim()
            )
            .unwrap(),
            SECRET
        );
    }

    #[tokio::test]
    async fn identity_key_store_migrates_legacy_key_file() {
        let dir = tempfile::tempdir().unwrap();
        let key_ref = IdentityKeyRef::owner(dir.path());
        write_ed25519_secret_file(key_ref.legacy_path(), &SECRET)
            .await
            .unwrap();
        let store = Arc::new(MemoryIdentityKeyStore::default());

        let secrets = load_or_init_local_identity_secrets_with_store(dir.path(), store.clone())
            .await
            .unwrap();

        assert_eq!(*secrets.owner, SECRET);
        assert_eq!(store.get_for_test(key_ref.account()), Some(SECRET));
        assert!(!key_ref.legacy_path().exists());
    }

    #[tokio::test]
    async fn local_identity_bundle_migrates_and_reuses_existing_identities() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(MemoryIdentityKeyStore::default());
        let daemon_ref = IdentityKeyRef::daemon(dir.path());
        let owner_ref = IdentityKeyRef::owner(dir.path());
        let daemon_secret = [7u8; 32];
        let owner_secret = [8u8; 32];

        store
            .put_secret(daemon_ref.account(), &daemon_secret, false)
            .unwrap();
        store
            .put_secret(owner_ref.account(), &owner_secret, false)
            .unwrap();

        let first = load_or_init_local_identity_secrets_with_store(dir.path(), store.clone())
            .await
            .unwrap();

        assert_eq!(*first.daemon, daemon_secret);
        assert_eq!(*first.owner, owner_secret);
        assert_eq!(
            first.location,
            IdentityKeyRef::bundle(dir.path()).location()
        );

        assert!(
            store
                .get_bytes_for_test(IdentityKeyRef::bundle(dir.path()).account())
                .is_some()
        );
    }

    #[test]
    fn local_identity_bundle_round_trips_without_location() {
        let secrets = LocalIdentitySecrets {
            location: "runtime-only location".to_string(),
            daemon: [9u8; 32].into(),
            owner: [10u8; 32].into(),
        };

        let encoded = secrets.to_bytes().unwrap().to_string();
        let decoded = LocalIdentitySecrets::from_str(&encoded).unwrap();

        assert_eq!(*decoded.daemon, [9u8; 32]);
        assert_eq!(*decoded.owner, [10u8; 32]);
        assert!(decoded.location.is_empty());
    }

    #[tokio::test]
    async fn load_identity_key_reports_missing_file_when_secure_store_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let key_ref = IdentityKeyRef::owner(dir.path());
        let store = Arc::new(UnavailableIdentityKeyStore);

        let err = load_identity_secret_with_location_with_store(&key_ref, store)
            .await
            .map(|_| ())
            .unwrap_err();

        assert!(err.to_string().contains("identity key not found"));
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
            1 => iana::KeyTypeEC2,
            -1 => iana::EllipticCurveP_256,
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
            1 => iana::KeyTypeOKP,
            3 => iana::AlgorithmEdDSA,
            iana::OKPKeyParameterCrv => iana::EllipticCurveEd25519,
            iana::OKPKeyParameterX => Value::Bytes(key.pubkey().as_bytes().to_vec()),
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
        let claims = Claims::default();

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
        let payload: Value = cbor2::from_slice(arr[2].as_bytes().unwrap()).unwrap();
        assert!(matches!(payload, Value::Map(_)));
    }

    #[tokio::test]
    async fn key_file_writer_round_trips_private_key() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("user.key");

        write_ed25519_secret_file(&key_path, &SECRET).await.unwrap();

        assert_eq!(
            load_or_init_ed25519_secret(&key_path).await.unwrap(),
            SECRET
        );
        assert!(write_ed25519_secret_file(&key_path, &SECRET).await.is_err());
        assert_eq!(
            load_or_init_ed25519_secret(&key_path).await.unwrap(),
            SECRET
        );

        write_ed25519_secret_file_blocking(&key_path, &[2; 32], true).unwrap();
        assert_eq!(
            load_or_init_ed25519_secret(&key_path).await.unwrap(),
            [2; 32]
        );
    }

    #[test]
    fn key_file_writer_ignores_empty_parent_for_relative_file_name() {
        create_parent_dir_if_needed(Path::new("user.key")).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn key_file_writer_resets_permissions_when_overwriting() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("user.key");
        std::fs::write(&key_path, "old").unwrap();
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o644)).unwrap();

        write_ed25519_secret_file_blocking(&key_path, &SECRET, true).unwrap();

        let mode = std::fs::metadata(&key_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
