use super::*;
use super::{
    files::{
        create_parent_dir_if_needed, load_or_init_ed25519_secret,
        write_ed25519_secret_file_blocking, write_private_binary_file,
    },
    store::IdentityKeyStoreUnavailable,
};
use anda_core::BoxError;
use cbor2::Value;
use ed25519_dalek::VerifyingKey;
use ic_auth_types::ByteBufB64;
use std::{path::Path, str::FromStr, sync::Arc};

const SECRET: [u8; 32] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32,
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
fn local_encrypted_identity_store_round_trips_trusted_user_secret() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalEncryptedIdentityKeyStore::new(dir.path(), [9u8; 32]);
    let alice = IdentityKeyRef::trusted_user(dir.path(), "alice");

    store.put_secret(alice.account(), &SECRET, false).unwrap();

    assert_eq!(store.get_secret(alice.account()).unwrap(), Some(SECRET));
    assert!(store.credential_path(alice.account()).exists());
    assert!(!alice.legacy_path().exists());
}

#[test]
fn local_encrypted_identity_store_requires_matching_daemon_secret() {
    let dir = tempfile::tempdir().unwrap();
    let alice = IdentityKeyRef::trusted_user(dir.path(), "alice");
    let writer = LocalEncryptedIdentityKeyStore::new(dir.path(), [9u8; 32]);
    let reader = LocalEncryptedIdentityKeyStore::new(dir.path(), [8u8; 32]);

    writer.put_secret(alice.account(), &SECRET, false).unwrap();

    assert!(reader.get_secret(alice.account()).is_err());
}

#[test]
fn local_encrypted_identity_store_binds_ciphertext_to_account_aad() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalEncryptedIdentityKeyStore::new(dir.path(), [9u8; 32]);
    let alice = IdentityKeyRef::trusted_user(dir.path(), "alice");
    let bob = IdentityKeyRef::trusted_user(dir.path(), "bob");

    store.put_secret(alice.account(), &SECRET, false).unwrap();
    std::fs::copy(
        store.credential_path(alice.account()),
        store.credential_path(bob.account()),
    )
    .unwrap();

    assert!(store.get_secret(bob.account()).is_err());
}

#[test]
fn local_encrypted_identity_store_migrates_from_existing_store() {
    let dir = tempfile::tempdir().unwrap();
    let legacy_store = Arc::new(MemoryIdentityKeyStore::default());
    let alice = IdentityKeyRef::trusted_user(dir.path(), "alice");
    legacy_store
        .put_secret(alice.account(), &SECRET, false)
        .unwrap();
    let store = LocalEncryptedIdentityKeyStore::with_migration_store(
        dir.path(),
        [9u8; 32],
        Some(legacy_store),
    );

    assert_eq!(store.get_secret(alice.account()).unwrap(), Some(SECRET));
    assert!(store.credential_path(alice.account()).exists());
}

#[test]
fn local_encrypted_identity_store_migrates_legacy_key_file() {
    let dir = tempfile::tempdir().unwrap();
    let alice = IdentityKeyRef::trusted_user(dir.path(), "alice");
    write_ed25519_secret_file_blocking(alice.legacy_path(), &SECRET, false).unwrap();
    let store = LocalEncryptedIdentityKeyStore::new(dir.path(), [9u8; 32]);

    assert_eq!(store.get_secret(alice.account()).unwrap(), Some(SECRET));
    assert!(store.credential_path(alice.account()).exists());
    assert!(!alice.legacy_path().exists());
}

#[test]
fn local_encrypted_identity_store_fails_closed_when_local_file_is_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let legacy_store = Arc::new(MemoryIdentityKeyStore::default());
    let alice = IdentityKeyRef::trusted_user(dir.path(), "alice");
    legacy_store
        .put_secret(alice.account(), &SECRET, false)
        .unwrap();
    let store = LocalEncryptedIdentityKeyStore::with_migration_store(
        dir.path(),
        [9u8; 32],
        Some(legacy_store),
    );
    write_private_binary_file(
        &store.credential_path(alice.account()),
        b"not cose",
        false,
        &store.home,
    )
    .unwrap();

    assert!(store.get_secret(alice.account()).is_err());
}

#[cfg(unix)]
#[test]
fn local_encrypted_identity_store_uses_private_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let store = LocalEncryptedIdentityKeyStore::new(dir.path(), [9u8; 32]);
    let alice = IdentityKeyRef::trusted_user(dir.path(), "alice");

    store.put_secret(alice.account(), &SECRET, false).unwrap();

    let file_mode = std::fs::metadata(store.credential_path(alice.account()))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    let dir_mode = std::fs::metadata(store.root())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(file_mode, 0o600);
    assert_eq!(dir_mode, 0o700);
}

#[cfg(unix)]
#[test]
fn local_encrypted_identity_store_does_not_chmod_home_boundary() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let store = LocalEncryptedIdentityKeyStore::new(dir.path(), [9u8; 32]);
    let alice = IdentityKeyRef::trusted_user(dir.path(), "alice");

    store.put_secret(alice.account(), &SECRET, false).unwrap();

    let home_mode = std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777;
    let credentials_mode = std::fs::metadata(dir.path().join("credentials"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    let root_mode = std::fs::metadata(store.root())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(home_mode, 0o755);
    assert_eq!(credentials_mode, 0o700);
    assert_eq!(root_mode, 0o700);
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
