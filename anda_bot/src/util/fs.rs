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

/// Reduces an untrusted name (an IM attachment file name, a message id, a URL
/// segment) to a single path component that is safe to join onto a directory.
///
/// Keeps ASCII alphanumerics, `.`, `-` and `_`; every other character — path
/// separators and `..` included — collapses into a single `_`. The result is
/// capped at 96 characters and stripped of leading/trailing `.`, `-` and `_`,
/// so a sanitized name is never `.` or `..`.
///
/// `fallback` is returned verbatim when nothing usable survives, so pass a
/// literal. Passing `""` is the deliberate way to ask for "no usable
/// component" and does return an empty string — `stored_attachment_name`
/// relies on that; every other caller should pass a non-empty default.
pub fn sanitize_path_component(value: &str, fallback: &str) -> String {
    let mut sanitized = String::with_capacity(value.len().min(96));
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
            sanitized.push(ch);
        } else if !sanitized.ends_with('_') {
            sanitized.push('_');
        }
        if sanitized.len() >= 96 {
            break;
        }
    }

    let sanitized = sanitized.trim_matches(['.', '-', '_']).to_string();
    if sanitized.is_empty() {
        fallback.to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod path_component_tests {
    use super::sanitize_path_component;

    #[test]
    fn keeps_safe_ascii_and_collapses_separators() {
        assert_eq!(
            sanitize_path_component(" report-01.json ", "fallback"),
            "report-01.json"
        );
        assert_eq!(
            sanitize_path_component("hello world.txt", "media.bin"),
            "hello_world.txt"
        );
        assert_eq!(
            sanitize_path_component("../奇怪 文件?.png", "fallback"),
            "png"
        );
    }

    #[test]
    fn traversal_and_unusable_names_fall_back() {
        assert_eq!(sanitize_path_component("../../", "media.bin"), "media.bin");
        assert_eq!(
            sanitize_path_component("***", "fallback.bin"),
            "fallback.bin"
        );
        assert_eq!(sanitize_path_component("", "fallback.bin"), "fallback.bin");
        // `stored_attachment_name` passes an empty fallback to mean "no usable
        // component" and branches on the empty result.
        assert_eq!(sanitize_path_component("***", ""), "");
    }

    #[test]
    fn long_names_are_capped() {
        assert_eq!(
            sanitize_path_component(&"a".repeat(128), "fallback"),
            "a".repeat(96)
        );
    }
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
