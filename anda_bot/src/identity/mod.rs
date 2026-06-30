mod ed25519;
mod files;
mod local_store;
mod refs;
mod secrets;
mod store;

pub use cose2::{cwt::Claims, iana};
pub use ed25519::{Ed25519Key, Ed25519PubKey, encode_ed25519_pubkey, random_ed25519_privkey};
pub use files::write_ed25519_secret_file;
pub use local_store::local_encrypted_identity_key_store;
pub use refs::IdentityKeyRef;
pub use secrets::{
    LocalIdentitySecrets, load_identity_secret_with_location_with_store,
    load_or_init_local_identity_secrets_with_store, read_local_identity_secrets_from_stdin,
    write_identity_secret_with_store,
};
pub use store::{IdentityKeyStore, os_identity_key_store};

#[cfg(test)]
pub use ed25519::{encode_ed25519_privkey, parse_ed25519_privkey, parse_ed25519_pubkey};
#[cfg(test)]
pub use local_store::LocalEncryptedIdentityKeyStore;
#[cfg(test)]
pub use store::MemoryIdentityKeyStore;

const IDENTITY_KEY_STORE_UNAVAILABLE_HINT: &str = "On Linux, start and unlock a Secret Service provider in a user D-Bus session, for example `gnome-keyring-daemon --start --components=secrets`, make sure DBUS_SESSION_BUS_ADDRESS is set for Anda, then restart Anda to use the OS keyring.";

#[cfg(test)]
mod tests;

