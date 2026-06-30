use anda_core::BoxError;
use std::path::{Path, PathBuf};

use super::{
    ed25519::{encode_ed25519_privkey, parse_ed25519_privkey},
    refs::{IdentityKeyRef, hex_bytes},
    secrets::LoadedIdentitySecret,
};

pub(super) fn read_identity_secret_file(
    key_ref: &IdentityKeyRef,
) -> Result<LoadedIdentitySecret, std::io::Error> {
    match std::fs::read_to_string(key_ref.legacy_path()) {
        Ok(content) => parse_ed25519_privkey(content.trim())
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
    let secret = *secret;
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

    let encoded = encode_ed25519_privkey(secret)?;
    write_private_text_file(key_path, &encoded, overwrite)
}

pub(super) fn create_parent_dir_if_needed(path: &Path) -> Result<(), BoxError> {
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

pub(super) fn write_private_binary_file(
    path: &Path,
    content: &[u8],
    overwrite: bool,
    private_boundary: &Path,
) -> Result<(), BoxError> {
    create_private_parent_dir_if_needed(path, private_boundary)?;

    let temp_path = private_temp_path(path)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    use std::io::Write;
    let mut file = options.open(&temp_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(content)?;
    file.sync_all()?;
    drop(file);

    if overwrite {
        std::fs::rename(&temp_path, path)?;
    } else {
        match std::fs::hard_link(&temp_path, path) {
            Ok(()) => {
                let _ = std::fs::remove_file(&temp_path);
            }
            Err(_) => {
                let _ = std::fs::remove_file(&temp_path);
                return Err(
                    format!("local credential file already exists: {}", path.display()).into(),
                );
            }
        }
    }
    Ok(())
}

fn private_temp_path(path: &Path) -> Result<PathBuf, BoxError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("invalid local credential path: {}", path.display()))?
        .to_string_lossy();
    let mut rng = rand::rng();
    for _ in 0..16 {
        let mut random = [0u8; 8];
        rand::Rng::fill_bytes(&mut rng, &mut random);
        let candidate = parent.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            hex_bytes(&random)
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "failed to allocate temporary local credential path for {}",
        path.display()
    )
    .into())
}

fn create_private_parent_dir_if_needed(
    path: &Path,
    private_boundary: &Path,
) -> Result<(), BoxError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
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

#[cfg(test)]
pub async fn load_or_init_ed25519_secret(key_path: &Path) -> Result<[u8; 32], BoxError> {
    use super::ed25519::random_ed25519_privkey;

    match crate::util::text::read_text_file(key_path).await {
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
