use anda_core::BoxError;
use std::path::Path;
use zeroize::Zeroizing;

use super::{
    ed25519::{encode_ed25519_privkey, parse_ed25519_privkey},
    refs::IdentityKeyRef,
    secrets::LoadedIdentitySecret,
};

pub(super) fn read_identity_secret_file(
    key_ref: &IdentityKeyRef,
) -> Result<LoadedIdentitySecret, std::io::Error> {
    match std::fs::read_to_string(key_ref.legacy_path()) {
        Ok(content) => parse_ed25519_privkey(Zeroizing::new(content).trim())
            .map(|secret| LoadedIdentitySecret::new(secret, key_ref.fallback_location()))
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
        Err(err) => Err(err),
    }
}

pub(super) fn remove_legacy_identity_key(key_ref: &IdentityKeyRef) {
    match std::fs::remove_file(key_ref.legacy_path()) {
        Ok(()) => log::warn!(
            name = "daemon";
            "removed legacy ED25519 private key file {:?}",
            key_ref.legacy_path()
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => log::warn!(
            name = "daemon";
            "migrated ED25519 private key, but failed to remove legacy file {:?}: {err}",
            key_ref.legacy_path()
        ),
    }
}

pub async fn write_ed25519_secret_file(key_path: &Path, secret: &[u8; 32]) -> Result<(), BoxError> {
    let key_path = key_path.to_path_buf();
    let secret = Zeroizing::new(*secret);
    tokio::task::spawn_blocking(move || {
        write_ed25519_secret_file_blocking(&key_path, &secret, false)
    })
    .await?
}

pub(super) fn write_ed25519_secret_file_blocking(
    key_path: &Path,
    secret: &[u8; 32],
    overwrite: bool,
) -> Result<(), BoxError> {
    create_parent_dir_if_needed(key_path)?;

    let mut encoded = Zeroizing::new(encode_ed25519_privkey(secret)?);
    encoded.push('\n');
    write_private_file(key_path, encoded.as_bytes(), overwrite)
}

pub(super) fn create_parent_dir_if_needed(path: &Path) -> Result<(), BoxError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub(super) fn write_private_binary_file(
    path: &Path,
    content: &[u8],
    overwrite: bool,
    private_boundary: &Path,
) -> Result<(), BoxError> {
    create_private_parent_dir_if_needed(path, private_boundary)?;
    write_private_file(path, content, overwrite)
}

fn write_private_file(path: &Path, content: &[u8], overwrite: bool) -> Result<(), BoxError> {
    use std::io::Write;

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    // NamedTempFile removes unpublished files on every error path. Keeping it
    // beside the destination makes publication atomic on the same filesystem.
    let mut temp = tempfile::Builder::new()
        .prefix(".identity-")
        .tempfile_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    temp.write_all(content)?;
    temp.as_file().sync_all()?;
    let result = if overwrite {
        temp.persist(path)
    } else {
        temp.persist_noclobber(path)
    };
    // Retain the original error kind (including AlreadyExists) and let the
    // temporary file in PersistError drop, rather than masking all I/O errors.
    result.map_err(|err| err.error)?;
    Ok(())
}

fn create_private_parent_dir_if_needed(
    path: &Path,
    private_boundary: &Path,
) -> Result<(), BoxError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        // Create new directories private from the start, so there is no
        // window between create_dir_all and the chmod below where the
        // umask-mode directory is visible to other local users.
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)?;
        }
        #[cfg(not(unix))]
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let private_mode = std::fs::Permissions::from_mode(0o700);
            let boundary = std::fs::canonicalize(private_boundary)?;
            let mut dir = std::fs::canonicalize(parent)?;
            if !dir.starts_with(&boundary) {
                return Err(format!(
                    "local credential directory {} is outside private boundary {}",
                    dir.display(),
                    boundary.display()
                )
                .into());
            }

            // Keep credential directories private without chmod'ing the existing
            // home directory or system temp roots that may not be owned by us.
            while dir != boundary {
                match std::fs::metadata(&dir) {
                    Ok(meta) if meta.permissions().mode() & 0o777 != 0o700 => {
                        std::fs::set_permissions(&dir, private_mode.clone())?;
                    }
                    Ok(_) => {}
                    Err(err) => return Err(err.into()),
                }
                if !dir.pop() {
                    break;
                }
            }
        }
    }
    Ok(())
}
