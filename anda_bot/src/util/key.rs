use anda_core::{BoxError, Principal};
use anda_web3_client::client::{Identity, identity_from_secret};
use cose2::{Key as CoseKey, Label, Sign1Message};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use ic_auth_types::ByteBufB64;
use ic_ed25519::PublicKey;
use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

pub use cose2::{cwt::Claims, iana};

#[cfg(test)]
use cose2::Value;

pub const IDENTITY_KEYRING_SERVICE: &str = "com.ldclabs.anda-bot.identity";

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
    legacy_path: PathBuf,
}

impl IdentityKeyRef {
    pub fn account(&self) -> &str {
        &self.account
    }

    pub fn legacy_path(&self) -> &Path {
        &self.legacy_path
    }

    pub fn location(&self) -> String {
        format!("system keyring account: {}", self.account)
    }
}

pub trait IdentityKeyStore: Send + Sync {
    fn get_secret(&self, account: &str) -> Result<Option<[u8; 32]>, BoxError>;
    fn put_secret(&self, account: &str, secret: &[u8; 32], overwrite: bool)
    -> Result<(), BoxError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OsIdentityKeyStore;

impl IdentityKeyStore for OsIdentityKeyStore {
    fn get_secret(&self, account: &str) -> Result<Option<[u8; 32]>, BoxError> {
        let entry = keyring::Entry::new(IDENTITY_KEYRING_SERVICE, account)?;
        match entry.get_secret() {
            Ok(secret) => Ok(Some(secret.try_into().map_err(|secret: Vec<u8>| {
                format!(
                    "identity keyring entry {account} has invalid length {}; expected 32 bytes",
                    secret.len()
                )
            })?)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn put_secret(
        &self,
        account: &str,
        secret: &[u8; 32],
        overwrite: bool,
    ) -> Result<(), BoxError> {
        if !overwrite && self.get_secret(account)?.is_some() {
            return Err(format!("identity key already exists in system keyring: {account}").into());
        }

        let entry = keyring::Entry::new(IDENTITY_KEYRING_SERVICE, account)?;
        entry.set_secret(secret)?;
        let stored = self
            .get_secret(account)?
            .ok_or_else(|| format!("identity keyring write did not persist: {account}"))?;
        if stored != *secret {
            return Err(format!("identity keyring verification failed: {account}").into());
        }
        Ok(())
    }
}

pub fn os_identity_key_store() -> Arc<dyn IdentityKeyStore> {
    Arc::new(OsIdentityKeyStore)
}

pub fn daemon_identity_key(home: &Path) -> IdentityKeyRef {
    identity_key_ref(home, "daemon", home.join("keys").join("anda_bot.key"))
}

pub fn owner_identity_key(home: &Path) -> IdentityKeyRef {
    identity_key_ref(home, "owner", home.join("keys").join("user.key"))
}

pub fn trusted_user_identity_key(home: &Path, id: &str) -> IdentityKeyRef {
    identity_key_ref(
        home,
        &format!("user:{id}"),
        home.join("keys").join("users").join(format!("{id}.key")),
    )
}

fn identity_key_ref(home: &Path, name: &str, legacy_path: PathBuf) -> IdentityKeyRef {
    IdentityKeyRef {
        account: format!("v1:{}:{name}", identity_home_namespace(home)),
        legacy_path,
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

pub async fn load_or_init_identity_secret_with_store(
    key_ref: &IdentityKeyRef,
    store: Arc<dyn IdentityKeyStore>,
) -> Result<[u8; 32], BoxError> {
    let key_ref = key_ref.clone();
    tokio::task::spawn_blocking(move || load_or_init_identity_secret_blocking(&key_ref, store))
        .await?
}

pub async fn write_identity_secret_with_store(
    key_ref: &IdentityKeyRef,
    secret: &[u8; 32],
    overwrite: bool,
    store: Arc<dyn IdentityKeyStore>,
) -> Result<(), BoxError> {
    let key_ref = key_ref.clone();
    let secret = *secret;
    tokio::task::spawn_blocking(move || store.put_secret(key_ref.account(), &secret, overwrite))
        .await?
}

fn load_or_init_identity_secret_blocking(
    key_ref: &IdentityKeyRef,
    store: Arc<dyn IdentityKeyStore>,
) -> Result<[u8; 32], BoxError> {
    if let Some(secret) = store.get_secret(key_ref.account())? {
        return Ok(secret);
    }

    match std::fs::read_to_string(key_ref.legacy_path()) {
        Ok(content) => {
            let secret = parse_ed25519_privkey(content.trim())?;
            store.put_secret(key_ref.account(), &secret, false)?;
            log::warn!(
                name = "daemon";
                "migrated ED25519 private key from {:?} to {}",
                key_ref.legacy_path(),
                key_ref.location()
            );
            remove_legacy_identity_key(key_ref);
            Ok(secret)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            log::warn!(
                name = "daemon";
                "ED25519 private key not found in {}, generating a new one",
                key_ref.location()
            );
            let secret = random_ed25519_privkey();
            store.put_secret(key_ref.account(), &secret, false)?;
            Ok(secret)
        }
        Err(err) => Err(err.into()),
    }
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
            write_ed25519_secret_file(key_path, &secret, false).await?;
            Ok(secret)
        }
    }
}

pub async fn write_ed25519_secret_file(
    key_path: &Path,
    secret: &[u8; 32],
    overwrite: bool,
) -> Result<(), BoxError> {
    if let Some(parent) = key_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let encoded = encode_ed25519_privkey(secret)?;
    let key_path = key_path.to_path_buf();
    tokio::task::spawn_blocking(move || write_private_text_file(&key_path, &encoded, overwrite))
        .await??;
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
    secrets: std::sync::Mutex<std::collections::BTreeMap<String, [u8; 32]>>,
}

#[cfg(test)]
impl MemoryIdentityKeyStore {
    pub fn get_for_test(&self, account: &str) -> Option<[u8; 32]> {
        self.secrets.lock().unwrap().get(account).copied()
    }
}

#[cfg(test)]
impl IdentityKeyStore for MemoryIdentityKeyStore {
    fn get_secret(&self, account: &str) -> Result<Option<[u8; 32]>, BoxError> {
        Ok(self.secrets.lock().unwrap().get(account).copied())
    }

    fn put_secret(
        &self,
        account: &str,
        secret: &[u8; 32],
        overwrite: bool,
    ) -> Result<(), BoxError> {
        let mut secrets = self.secrets.lock().unwrap();
        if !overwrite && secrets.contains_key(account) {
            return Err(format!("identity key already exists in system keyring: {account}").into());
        }
        secrets.insert(account.to_string(), *secret);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let daemon = daemon_identity_key(dir.path());
        let owner = owner_identity_key(dir.path());
        let alice = trusted_user_identity_key(dir.path(), "alice");

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
    async fn identity_key_store_generates_and_reuses_secret() {
        let dir = tempfile::tempdir().unwrap();
        let key_ref = owner_identity_key(dir.path());
        let store = Arc::new(MemoryIdentityKeyStore::default());

        let first = load_or_init_identity_secret_with_store(&key_ref, store.clone())
            .await
            .unwrap();
        let second = load_or_init_identity_secret_with_store(&key_ref, store.clone())
            .await
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(store.get_for_test(key_ref.account()), Some(first));
        assert!(!key_ref.legacy_path().exists());
    }

    #[tokio::test]
    async fn identity_key_store_migrates_legacy_key_file() {
        let dir = tempfile::tempdir().unwrap();
        let key_ref = owner_identity_key(dir.path());
        write_ed25519_secret_file(key_ref.legacy_path(), &SECRET, false)
            .await
            .unwrap();
        let store = Arc::new(MemoryIdentityKeyStore::default());

        let secret = load_or_init_identity_secret_with_store(&key_ref, store.clone())
            .await
            .unwrap();

        assert_eq!(secret, SECRET);
        assert_eq!(store.get_for_test(key_ref.account()), Some(SECRET));
        assert!(!key_ref.legacy_path().exists());
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

        write_ed25519_secret_file(&key_path, &SECRET, false)
            .await
            .unwrap();

        assert_eq!(
            load_or_init_ed25519_secret(&key_path).await.unwrap(),
            SECRET
        );
        assert!(
            write_ed25519_secret_file(&key_path, &SECRET, false)
                .await
                .is_err()
        );
        write_ed25519_secret_file(&key_path, &[2; 32], true)
            .await
            .unwrap();
        assert_eq!(
            load_or_init_ed25519_secret(&key_path).await.unwrap(),
            [2; 32]
        );
    }
}
