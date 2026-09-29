//! `anda install`: puts this binary at the one CLI location that the install
//! scripts, Homebrew and Anda Desktop share, so every entry point runs the
//! same `anda`. Anda Desktop runs its bundled copy with this command on every
//! start; releases only ever move forward, and Homebrew installs are left to
//! `brew upgrade`.

use anda_core::BoxError;
use clap::Args;
use serde::Serialize;
use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    auto_update::is_newer_release,
    autostart,
    cli::{
        launcher_retirement::{self, Retirement},
        updater,
    },
    daemon_protocol::current_version_tag,
    util::windows_process::suppress_console_window,
};

/// Curated skills shipped next to a bundled binary (Anda Desktop resources).
const BUNDLED_SKILLS_DIR: &str = "skills";
const INSTALL_DIR_ENV: &str = "ANDA_INSTALL_DIR";

#[derive(Args)]
pub struct InstallCommand {
    /// Install into this directory instead of the shared CLI location.
    #[arg(long)]
    dir: Option<PathBuf>,
    /// Print the install report as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallAction {
    /// No anda existed at the shared location; this binary was copied there.
    Installed,
    /// An older anda was replaced by this binary.
    Upgraded,
    /// The installed anda already is this release.
    Current,
    /// The installed anda is newer and was kept.
    KeptNewer,
    /// Homebrew owns the installed anda; update it with `brew upgrade anda`.
    Homebrew,
}

#[derive(Debug, Serialize)]
pub struct InstallReport {
    pub action: InstallAction,
    /// The anda every client should run from now on.
    pub path: PathBuf,
    /// Release tag of `path` after this command, when it could be read.
    pub version: Option<String>,
    /// Curated skills were copied into the Anda home.
    pub skills_installed: bool,
    /// A shell profile or the user PATH changed; new terminals pick it up.
    pub path_updated: bool,
    /// The retired tray launcher was removed from this install.
    pub launcher_retired: bool,
    /// The retired launcher used to start at login; the daemon's own login
    /// entry now does, and Anda Desktop may keep its tray at login.
    pub launcher_started_at_login: bool,
}

/// The side effects `install_from` has on the machine, replaceable in tests.
struct Host<'a> {
    probe_version: &'a dyn Fn(&Path) -> Option<String>,
    add_to_path: &'a dyn Fn(&Path) -> Result<bool, BoxError>,
    retire_launcher: &'a dyn Fn(&Path) -> Retirement,
    register_autostart: &'a dyn Fn(&Path, &Path) -> Result<(), BoxError>,
}

const SYSTEM: Host<'static> = Host {
    probe_version: &installed_version,
    add_to_path: &ensure_on_path,
    retire_launcher: &launcher_retirement::retire,
    register_autostart: &autostart::install_for,
};

pub async fn run(home: &Path, cmd: &InstallCommand) -> Result<(), BoxError> {
    let source = std::env::current_exe()?;
    let target = match &cmd.dir {
        Some(dir) => dir.join(executable_name()),
        None => locate_installed(default_install_dir()?, extra_install_candidates()),
    };
    let report = install_from(&source, &target, home, &current_version_tag(), &SYSTEM)?;
    if cmd.json {
        println!("{}", serde_json::to_string(&report)?);
    } else {
        print_report(&report);
    }
    Ok(())
}

fn print_report(report: &InstallReport) {
    let path = report.path.display();
    let version = report.version.as_deref().unwrap_or("unknown version");
    match report.action {
        InstallAction::Installed => println!("Installed anda {version} at {path}."),
        InstallAction::Upgraded => {
            println!("Upgraded anda at {path} to {version}. Run `anda restart` to use it.")
        }
        InstallAction::Current => println!("anda {version} is already installed at {path}."),
        InstallAction::KeptNewer => {
            println!("Kept the newer anda {version} installed at {path}.")
        }
        InstallAction::Homebrew => {
            println!("Using Homebrew's anda at {path}. Update it with `brew upgrade anda`.")
        }
    }
    if report.skills_installed {
        println!("Installed curated skills.");
    }
    if report.launcher_retired {
        println!("Removed the retired Anda Bot tray launcher; Anda Desktop provides the tray.");
    }
    if report.path_updated {
        println!("Added {} to PATH. Open a new terminal to use `anda`.", {
            report
                .path
                .parent()
                .map(|dir| dir.display().to_string())
                .unwrap_or_default()
        });
    }
}

fn executable_name() -> &'static str {
    if cfg!(windows) { "anda.exe" } else { "anda" }
}

/// Where the install scripts put `anda` (`ANDA_INSTALL_DIR` overrides it).
fn default_install_dir() -> Result<PathBuf, BoxError> {
    if let Some(dir) = std::env::var_os(INSTALL_DIR_ENV).filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is not set")?;
        Ok(PathBuf::from(base).join("Programs").join("AndaBot"))
    }
    #[cfg(not(windows))]
    {
        let home = std::env::home_dir().ok_or("could not detect the user home directory")?;
        Ok(home.join(".local").join("bin"))
    }
}

/// Other places an existing install may live, after the default directory.
fn extra_install_candidates() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        // `install.sh` under Git Bash used `%USERPROFILE%\bin`.
        std::env::var_os("USERPROFILE")
            .map(|home| vec![PathBuf::from(home).join("bin").join("anda.exe")])
            .unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        [
            "/opt/homebrew/bin/anda",
            "/usr/local/bin/anda",
            "/home/linuxbrew/.linuxbrew/bin/anda",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    }
}

/// The existing install to keep using, else the default location.
fn locate_installed(default_dir: PathBuf, extra: Vec<PathBuf>) -> PathBuf {
    let default = default_dir.join(executable_name());
    std::iter::once(default.clone())
        .chain(extra)
        .find(|candidate| candidate.is_file())
        .unwrap_or(default)
}

/// Reads the release tag of an installed binary from `anda --version`.
fn installed_version(exe: &Path) -> Option<String> {
    let mut command = Command::new(exe);
    command.arg("--version");
    suppress_console_window(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_version_output(&String::from_utf8_lossy(&output.stdout))
}

fn parse_version_output(output: &str) -> Option<String> {
    let version = output.split_whitespace().last()?.trim_start_matches('v');
    version
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit())
        .then(|| format!("v{version}"))
}

fn install_from(
    source: &Path,
    target: &Path,
    home: &Path,
    source_tag: &str,
    host: &Host,
) -> Result<InstallReport, BoxError> {
    let (action, version) = if !target.exists() {
        place_binary(source, target)?;
        (InstallAction::Installed, Some(source_tag.to_string()))
    } else if same_file(source, target) {
        (InstallAction::Current, Some(source_tag.to_string()))
    } else if updater::is_homebrew_managed(target) {
        (InstallAction::Homebrew, (host.probe_version)(target))
    } else {
        match (host.probe_version)(target) {
            Some(tag) if tag == source_tag => (InstallAction::Current, Some(tag)),
            Some(tag) if is_newer_release(&tag, source_tag) => {
                (InstallAction::KeptNewer, Some(tag))
            }
            // Older or unreadable: this release replaces it.
            _ => {
                place_binary(source, target)?;
                (InstallAction::Upgraded, Some(source_tag.to_string()))
            }
        }
    };

    let placed = matches!(action, InstallAction::Installed | InstallAction::Upgraded);
    let skills_dir = updater::bundled_skills_dir(home);
    let skills_installed = if placed || !has_entries(&skills_dir) {
        install_bundled_skills(source, home)?
    } else {
        false
    };

    let dir = target
        .parent()
        .ok_or("the install target has no parent directory")?;
    let path_updated = action != InstallAction::Homebrew
        && (host.add_to_path)(dir).unwrap_or_else(|err| {
            eprintln!("Warning: could not add {} to PATH: {err}", dir.display());
            false
        });

    let retirement = (host.retire_launcher)(dir);
    if retirement.started_at_login
        && let Err(err) = (host.register_autostart)(target, home)
    {
        eprintln!("Warning: could not register Anda to start at login: {err}");
    }

    Ok(InstallReport {
        action,
        path: target.to_path_buf(),
        version,
        skills_installed,
        path_updated,
        launcher_retired: retirement.retired,
        launcher_started_at_login: retirement.started_at_login,
    })
}

fn same_file(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn has_entries(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some())
}

/// Copies `source` over `target` through a staged file in the same directory,
/// so readers see either the old or the new binary.
fn place_binary(source: &Path, target: &Path) -> Result<(), BoxError> {
    let dir = target
        .parent()
        .ok_or("the install target has no parent directory")?;
    std::fs::create_dir_all(dir)?;
    let staged = updater::staged_update_path(dir, target);
    std::fs::copy(source, &staged)
        .map_err(|err| format!("Could not copy anda to {}: {err}", staged.display()))?;
    let result = finish_placement(&staged, target);
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    result
}

#[cfg(unix)]
fn finish_placement(staged: &Path, target: &Path) -> Result<(), BoxError> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(staged, std::fs::Permissions::from_mode(0o755))?;
    #[cfg(target_os = "macos")]
    clear_quarantine(staged);
    // Renaming over a running binary is safe on Unix: the daemon keeps the old
    // inode until it restarts.
    std::fs::rename(staged, target)
        .map_err(|err| format!("Could not replace {}: {err}", target.display()).into())
}

#[cfg(windows)]
fn finish_placement(staged: &Path, target: &Path) -> Result<(), BoxError> {
    remove_retired_binaries(target);
    // A running executable cannot be overwritten, but it can be renamed:
    // move the old one aside (the daemon keeps running from it) and put the
    // new one in place.
    let retired = target.with_file_name(format!(
        ".{}.old-{}",
        target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("anda.exe"),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let moved_aside = target.exists();
    if moved_aside {
        std::fs::rename(target, &retired)
            .map_err(|err| format!("Could not move {} aside: {err}", target.display()))?;
    }
    if let Err(err) = std::fs::rename(staged, target) {
        if moved_aside {
            let _ = std::fs::rename(&retired, target);
        }
        return Err(format!("Could not replace {}: {err}", target.display()).into());
    }
    Ok(())
}

/// Deletes binaries earlier installs moved aside; ones still running stay.
#[cfg(windows)]
fn remove_retired_binaries(target: &Path) {
    let (Some(dir), Some(name)) = (target.parent(), target.file_name()) else {
        return;
    };
    let prefix = format!(".{}.old-", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// A copy taken out of a downloaded app bundle inherits its quarantine flag;
/// the app was already approved, and script installs are never quarantined.
#[cfg(target_os = "macos")]
fn clear_quarantine(path: &Path) {
    use std::os::unix::ffi::OsStrExt;

    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return;
    };
    unsafe {
        libc::removexattr(path.as_ptr(), c"com.apple.quarantine".as_ptr(), 0);
    }
}

/// Installs the curated skills bundled next to `source`, when present.
fn install_bundled_skills(source: &Path, home: &Path) -> Result<bool, BoxError> {
    let Some(bundle) = source
        .parent()
        .map(|dir| dir.join(BUNDLED_SKILLS_DIR))
        .filter(|dir| has_entries(dir))
    else {
        return Ok(false);
    };
    std::fs::create_dir_all(home)?;
    let staging = updater::StagedDir::new(updater::staged_skills_dir_path(home));
    copy_dir_all(&bundle, staging.path())?;
    updater::install_skills_from_staging(staging.path(), &updater::bundled_skills_dir(home))?;
    Ok(true)
}

fn copy_dir_all(source: &Path, destination: &Path) -> io::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Makes `dir` reachable from new terminals, like the install scripts do.
#[cfg(not(windows))]
fn ensure_on_path(dir: &Path) -> Result<bool, BoxError> {
    let home = std::env::home_dir().ok_or("could not detect the user home directory")?;
    let shell = std::env::var("SHELL").unwrap_or_default();
    let shell = Path::new(&shell)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("sh");
    let profile = shell_profile(&home, shell);
    append_path_to_profile(&profile, dir, &home, shell == "fish")
}

/// The profile `install.sh` writes for `shell`.
#[cfg(not(windows))]
fn shell_profile(home: &Path, shell: &str) -> PathBuf {
    match shell {
        "zsh" => home.join(".zshrc"),
        "bash" => home.join(".bashrc"),
        "fish" => home.join(".config/fish/config.fish"),
        _ if cfg!(target_os = "macos") => home.join(".zshrc"),
        _ => home.join(".profile"),
    }
}

#[cfg(not(windows))]
fn append_path_to_profile(
    profile: &Path,
    dir: &Path,
    home: &Path,
    fish: bool,
) -> Result<bool, BoxError> {
    let default_dir = home.join(".local").join("bin");
    let existing = std::fs::read_to_string(profile).unwrap_or_default();
    let dir_text = dir.to_string_lossy();
    if existing.contains(dir_text.as_ref())
        || (dir == default_dir && existing.contains(".local/bin"))
    {
        return Ok(false);
    }
    let line = match (fish, dir == default_dir) {
        (true, true) => r#"fish_add_path -g "$HOME/.local/bin""#.to_string(),
        (true, false) => format!("fish_add_path -g \"{dir_text}\""),
        (false, true) => r#"export PATH="$HOME/.local/bin:$PATH""#.to_string(),
        (false, false) => format!("export PATH=\"{dir_text}:$PATH\""),
    };
    if let Some(parent) = profile.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut block = String::new();
    if !existing.is_empty() {
        if !existing.ends_with('\n') {
            block.push('\n');
        }
        block.push('\n');
    }
    block.push_str("# anda-bot\n");
    block.push_str(&line);
    block.push('\n');
    use std::io::Write;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(profile)?
        .write_all(block.as_bytes())?;
    Ok(true)
}

/// Prepends `dir` to the user PATH, like `install.ps1`.
#[cfg(windows)]
fn ensure_on_path(dir: &Path) -> Result<bool, BoxError> {
    let dir = dir.to_string_lossy().replace('\'', "''");
    let script = format!(
        r#"$dir = '{dir}'
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$parts = @()
if (-not [string]::IsNullOrWhiteSpace($userPath)) {{ $parts = @($userPath -split ';' | Where-Object {{ $_ }}) }}
$normalized = [Environment]::ExpandEnvironmentVariables($dir).TrimEnd('\')
foreach ($part in $parts) {{
  if ([Environment]::ExpandEnvironmentVariables($part).TrimEnd('\').Equals($normalized, [StringComparison]::OrdinalIgnoreCase)) {{ 'unchanged'; exit 0 }}
}}
[Environment]::SetEnvironmentVariable('Path', ((@($dir) + $parts) -join ';'), 'User')
'changed'
"#
    );
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &script,
    ]);
    suppress_console_window(&mut command);
    let output = command.output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr)
            .trim()
            .to_string()
            .into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim() == "changed")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(version: Option<&'static str>) -> Host<'static> {
        // Leak one closure per call; tests are short-lived.
        let probe: &'static dyn Fn(&Path) -> Option<String> =
            Box::leak(Box::new(move |_: &Path| version.map(str::to_string)));
        Host {
            probe_version: probe,
            add_to_path: &|_| Ok(false),
            retire_launcher: &|_| Retirement::default(),
            register_autostart: &|_, _| Ok(()),
        }
    }

    fn write_executable(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("bundle").join("anda");
        write_executable(&source, "new");
        let skills = source.parent().unwrap().join(BUNDLED_SKILLS_DIR);
        write_executable(&skills.join("pdf").join("SKILL.md"), "pdf skill");
        let target = temp.path().join("bin").join("anda");
        let home = temp.path().join("home");
        (temp, source, target, home)
    }

    #[test]
    fn parses_clap_version_output() {
        assert_eq!(
            parse_version_output("anda 0.13.0\n"),
            Some("v0.13.0".into())
        );
        assert_eq!(parse_version_output("anda v0.14.1"), Some("v0.14.1".into()));
        assert_eq!(parse_version_output("garbage"), None);
        assert_eq!(parse_version_output(""), None);
    }

    #[test]
    fn locate_prefers_the_default_directory_then_known_installs() {
        let temp = tempfile::tempdir().unwrap();
        let default_dir = temp.path().join("default");
        let brew = temp.path().join("brew").join("anda");
        assert_eq!(
            locate_installed(default_dir.clone(), vec![brew.clone()]),
            default_dir.join(executable_name())
        );
        write_executable(&brew, "brew");
        assert_eq!(
            locate_installed(default_dir.clone(), vec![brew.clone()]),
            brew
        );
        write_executable(&default_dir.join(executable_name()), "script");
        assert_eq!(
            locate_installed(default_dir.clone(), vec![brew]),
            default_dir.join(executable_name())
        );
    }

    #[test]
    fn fresh_install_copies_binary_and_bundled_skills() {
        let (_temp, source, target, home) = fixture();
        let report = install_from(&source, &target, &home, "v0.13.0", &host(None)).unwrap();
        assert_eq!(report.action, InstallAction::Installed);
        assert_eq!(report.version.as_deref(), Some("v0.13.0"));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        assert!(report.skills_installed);
        assert!(home.join("bundled-skills/pdf/SKILL.md").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755);
        }
    }

    #[test]
    fn replaces_older_installs_and_never_downgrades() {
        let (_temp, source, target, home) = fixture();
        write_executable(&target, "old");
        let report =
            install_from(&source, &target, &home, "v0.13.0", &host(Some("v0.12.0"))).unwrap();
        assert_eq!(report.action, InstallAction::Upgraded);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");

        write_executable(&target, "newer");
        let report =
            install_from(&source, &target, &home, "v0.13.0", &host(Some("v0.14.0"))).unwrap();
        assert_eq!(report.action, InstallAction::KeptNewer);
        assert_eq!(report.version.as_deref(), Some("v0.14.0"));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "newer");
        // Skills were already installed by the upgrade; a kept install leaves
        // them to its own updater.
        assert!(!report.skills_installed);

        let report =
            install_from(&source, &target, &home, "v0.14.0", &host(Some("v0.14.0"))).unwrap();
        assert_eq!(report.action, InstallAction::Current);
    }

    #[test]
    fn unreadable_install_is_replaced_and_missing_skills_are_restored() {
        let (_temp, source, target, home) = fixture();
        write_executable(&target, "broken");
        let report = install_from(&source, &target, &home, "v0.13.0", &host(None)).unwrap();
        assert_eq!(report.action, InstallAction::Upgraded);

        std::fs::remove_dir_all(home.join("bundled-skills")).unwrap();
        let report =
            install_from(&source, &target, &home, "v0.13.0", &host(Some("v0.13.0"))).unwrap();
        assert_eq!(report.action, InstallAction::Current);
        assert!(report.skills_installed);
    }

    #[test]
    fn retiring_a_launcher_that_started_at_login_registers_the_daemon() {
        let (_temp, source, target, home) = fixture();
        let registered = std::sync::Mutex::new(None);
        let register = |exe: &Path, _: &Path| {
            *registered.lock().unwrap() = Some(exe.to_path_buf());
            Ok(())
        };
        let host = Host {
            register_autostart: &register,
            retire_launcher: &|_| Retirement {
                retired: true,
                started_at_login: true,
            },
            ..host(None)
        };
        let report = install_from(&source, &target, &home, "v0.13.0", &host).unwrap();
        assert!(report.launcher_retired && report.launcher_started_at_login);
        assert_eq!(
            registered.lock().unwrap().as_deref(),
            Some(target.as_path())
        );
    }

    #[cfg(unix)]
    #[test]
    fn homebrew_installs_are_left_to_brew() {
        let (temp, source, _target, home) = fixture();
        let cellar = temp.path().join("Cellar/anda/0.12.0/bin/anda");
        write_executable(&cellar, "brew");
        let link = temp.path().join("brew-bin").join("anda");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&cellar, &link).unwrap();

        let report =
            install_from(&source, &link, &home, "v0.13.0", &host(Some("v0.12.0"))).unwrap();
        assert_eq!(report.action, InstallAction::Homebrew);
        assert!(!report.path_updated);
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(&cellar).unwrap(), "brew");
    }

    #[cfg(not(windows))]
    #[test]
    fn profile_path_entry_is_added_once() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let profile = shell_profile(home, "zsh");
        let default_dir = home.join(".local").join("bin");
        std::fs::write(&profile, "alias ll='ls -l'").unwrap();

        assert!(append_path_to_profile(&profile, &default_dir, home, false).unwrap());
        assert!(!append_path_to_profile(&profile, &default_dir, home, false).unwrap());
        let content = std::fs::read_to_string(&profile).unwrap();
        assert_eq!(
            content,
            "alias ll='ls -l'\n\n# anda-bot\nexport PATH=\"$HOME/.local/bin:$PATH\"\n"
        );

        let custom = home.join("tools");
        let fish = shell_profile(home, "fish");
        assert!(append_path_to_profile(&fish, &custom, home, true).unwrap());
        assert_eq!(
            std::fs::read_to_string(&fish).unwrap(),
            format!("# anda-bot\nfish_add_path -g \"{}\"\n", custom.display())
        );
    }
}
