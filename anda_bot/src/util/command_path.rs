//! `PATH` for the commands the daemon spawns.
//!
//! A daemon started by launchd or systemd inherits a minimal `PATH`, which
//! usually lacks the directories where `npx`, `uvx`, `cargo` and friends live.
//! The shell tool and stdio MCP servers both extend it the same way.

#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;

/// Tool directories a daemon's minimal PATH (launchd, systemd) usually lacks.
#[cfg(not(target_os = "windows"))]
static TOOL_PATH_CANDIDATES: std::sync::LazyLock<Vec<PathBuf>> =
    std::sync::LazyLock::new(default_tool_path_candidates);

/// Returns `path` (the daemon's own `PATH` when `None`) with the usual tool
/// directories appended after its entries, or `None` when there is nothing to
/// set. Windows keeps the system `PATH` as it is.
#[cfg(not(target_os = "windows"))]
pub fn command_path(path: Option<&str>) -> Option<String> {
    let base_path = path
        .map(str::to_string)
        .or_else(|| std::env::var("PATH").ok());
    enriched_path_value(base_path.as_deref(), &TOOL_PATH_CANDIDATES)
}

#[cfg(target_os = "windows")]
pub fn command_path(_path: Option<&str>) -> Option<String> {
    None
}

#[cfg(not(target_os = "windows"))]
fn default_tool_path_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(current_exe) = std::env::current_exe()
        && let Some(dir) = current_exe.parent()
    {
        paths.push(dir.to_path_buf());
    }

    if let Some(home_dir) = std::env::home_dir() {
        paths.push(home_dir.join(".local").join("bin"));
        paths.push(home_dir.join(".cargo").join("bin"));
    }

    paths.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/opt/homebrew/sbin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/local/sbin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
    ]);

    paths
}

#[cfg(not(target_os = "windows"))]
fn enriched_path_value(base_path: Option<&str>, candidates: &[PathBuf]) -> Option<String> {
    let mut paths = base_path
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .filter(|path| !path.as_os_str().is_empty())
        .collect::<Vec<_>>();

    for candidate in candidates {
        if !candidate.as_os_str().is_empty() && !paths.contains(candidate) {
            paths.push(candidate.clone());
        }
    }

    std::env::join_paths(paths)
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn enriched_path_keeps_existing_entries_and_adds_tool_dirs() {
        let path = enriched_path_value(
            Some("/usr/bin:/bin"),
            &[
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/Users/example/.cargo/bin"),
            ],
        )
        .expect("path should join");

        let paths = std::env::split_paths(&path).collect::<Vec<_>>();
        assert_eq!(paths[0], PathBuf::from("/usr/bin"));
        assert_eq!(paths[1], PathBuf::from("/bin"));
        assert!(paths.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert!(paths.contains(&PathBuf::from("/Users/example/.cargo/bin")));
        assert_eq!(
            paths
                .iter()
                .filter(|path| path.as_path() == std::path::Path::new("/usr/bin"))
                .count(),
            1
        );
    }

    #[test]
    fn command_path_starts_from_the_given_path() {
        let path = command_path(Some("/custom/bin")).expect("path should join");
        let paths = std::env::split_paths(&path).collect::<Vec<_>>();
        assert_eq!(paths[0], PathBuf::from("/custom/bin"));
        assert!(paths.contains(&PathBuf::from("/usr/bin")));
    }
}
