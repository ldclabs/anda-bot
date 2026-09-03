//! OS locale detection shared by the `anda` daemon and the `anda_launcher`
//! tray app.
//!
//! Both binaries compile this same source file — `util.rs` declares
//! `pub mod locale;` and `anda_launcher.rs` includes it via `#[path]` — so the
//! tags one side reads and the tags the other side reads cannot drift.
//!
//! Callers own the mapping from a locale tag to their own language enum,
//! because the two binaries ship different sets of translated strings: the
//! launcher menu is localized into six languages while the daemon's native
//! dialogs are localized into two. What is shared is *where* tags come from
//! (platform preference list first, then `LC_ALL`/`LC_MESSAGES`/`LANG`), the
//! order they are tried in, and how a raw tag is normalized before matching.

/// Locale tags in decreasing order of user preference.
///
/// The platform's own preference list comes first, then the POSIX environment
/// variables. Tags are raw: pass each through [`normalize_tag`] before
/// matching, or use [`first_match`], which does it for you. The list is empty
/// when the platform exposes no preference and the environment is unset, so
/// every caller needs a fallback language.
pub fn system_locale_tags() -> Vec<String> {
    let mut tags = platform_locale_tags();
    tags.extend(environment_locale_tags());
    tags
}

/// Returns the first language `map` recognizes, trying `tags` in order.
///
/// Each tag is normalized before it reaches `map`, so `map` only ever sees a
/// lowercase, dash-separated tag with any encoding suffix and surrounding
/// quotes removed: `"zh_CN.UTF-8"` arrives as `zh-cn`. Returns `None` when no
/// tag is recognized; callers supply their own default language.
pub fn first_match<T, I>(tags: I, map: impl Fn(&str) -> Option<T>) -> Option<T>
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    tags.into_iter()
        .find_map(|tag| map(&normalize_tag(tag.as_ref())))
}

/// Normalizes a raw locale tag for prefix matching.
///
/// Trims whitespace and quotes (macOS `defaults read` emits quoted values),
/// drops any `.UTF-8`-style encoding suffix, converts POSIX underscores to
/// dashes, and lowercases: `"zh_CN.UTF-8"` becomes `zh-cn`.
pub fn normalize_tag(tag: &str) -> String {
    tag.trim()
        .trim_matches('"')
        .split('.')
        .next()
        .unwrap_or_default()
        .replace('_', "-")
        .to_ascii_lowercase()
}

#[cfg(target_os = "macos")]
fn platform_locale_tags() -> Vec<String> {
    let mut tags = macos_defaults_languages();
    if let Some(locale) = macos_defaults_value("AppleLocale") {
        tags.push(locale);
    }
    tags
}

#[cfg(target_os = "macos")]
fn macos_defaults_languages() -> Vec<String> {
    let Some(output) = macos_defaults_value("AppleLanguages") else {
        return Vec::new();
    };

    output
        .lines()
        .map(|line| {
            line.trim()
                .trim_start_matches('(')
                .trim_end_matches(')')
                .trim_end_matches(',')
                .trim()
                .trim_matches('"')
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .collect()
}

#[cfg(target_os = "macos")]
fn macos_defaults_value(key: &str) -> Option<String> {
    let output = std::process::Command::new("defaults")
        .arg("read")
        .arg("-g")
        .arg(key)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(windows)]
fn platform_locale_tags() -> Vec<String> {
    let mut buffer = [0u16; 85];
    let len = unsafe {
        windows_sys::Win32::Globalization::GetUserDefaultLocaleName(
            buffer.as_mut_ptr(),
            buffer.len() as i32,
        )
    };
    if len <= 1 {
        return Vec::new();
    }
    vec![String::from_utf16_lossy(&buffer[..(len as usize - 1)])]
}

#[cfg(not(any(target_os = "macos", windows)))]
fn platform_locale_tags() -> Vec<String> {
    Vec::new()
}

fn environment_locale_tags() -> Vec<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .filter(|value| !value.trim().is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn language(tag: &str) -> Option<&'static str> {
        if tag.starts_with("zh") {
            Some("zh")
        } else if tag.starts_with("en") {
            Some("en")
        } else {
            None
        }
    }

    #[test]
    fn normalize_tag_strips_quotes_encoding_and_case() {
        assert_eq!(normalize_tag("  \"zh_CN.UTF-8\" "), "zh-cn");
        assert_eq!(normalize_tag("en-US"), "en-us");
        assert_eq!(normalize_tag(""), "");
    }

    #[test]
    fn first_match_returns_the_earliest_recognized_tag() {
        assert_eq!(
            first_match(["fr-FR", "zh-Hans", "en-US"], language),
            Some("zh")
        );
        assert_eq!(first_match(["zh_CN.UTF-8"], language), Some("zh"));
        assert_eq!(first_match(["fr-FR", "de-DE"], language), None);
        assert_eq!(first_match(Vec::<String>::new(), language), None);
    }

    #[test]
    fn system_locale_tags_is_readable_without_panicking() {
        let _ = system_locale_tags();
    }
}
