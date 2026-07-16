use std::io;
use std::path::Path;

/// Tightens a secrets-bearing file (config.yaml, backups, …) to owner-only
/// access. No-op when the file already has no group/other bits, so repeated
/// calls on startup are cheap.
#[cfg(unix)]
pub fn restrict_secret_file_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::metadata(path)?;
    let mut permissions = metadata.permissions();
    if permissions.mode() & 0o077 != 0 {
        permissions.set_mode(0o600);
        std::fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

/// Windows ACLs default to per-user profile protection under the home
/// directory; there is no direct mode-bits equivalent to tighten.
#[cfg(not(unix))]
pub fn restrict_secret_file_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Tightens a secrets-bearing directory (mcp_credentials/, …) to owner-only
/// access. No-op when the directory already has no group/other bits.
#[cfg(unix)]
pub fn restrict_secret_dir_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::metadata(path)?;
    let mut permissions = metadata.permissions();
    if permissions.mode() & 0o077 != 0 {
        permissions.set_mode(0o700);
        std::fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

/// Windows ACLs default to per-user profile protection under the home
/// directory; there is no direct mode-bits equivalent to tighten.
#[cfg(not(unix))]
pub fn restrict_secret_dir_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

// Every test here exercises the Unix permission-bit path; the Windows
// implementation is a no-op, so the whole module is Unix-only.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn restrict_secret_file_permissions_removes_group_and_other_bits() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, "model:\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        restrict_secret_file_permissions(&path).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);

        // Idempotent on an already-tight file.
        restrict_secret_file_permissions(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    // The Windows no-op returns Ok(()) even for a missing path, so expecting
    // an error here is Unix-only (see the module cfg above).
    #[test]
    fn restrict_secret_file_permissions_errors_on_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.yaml");
        assert!(restrict_secret_file_permissions(&missing).is_err());
    }
}
