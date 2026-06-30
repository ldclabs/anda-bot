use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};

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

pub(super) fn identity_home_namespace(home: &Path) -> String {
    let path = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    let digest = <sha2::Sha256 as sha2::Digest>::digest(path.to_string_lossy().as_bytes());
    hex_bytes(&digest[..16])
}

pub(super) fn hex_bytes(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}
