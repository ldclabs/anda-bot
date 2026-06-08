use anda_core::BoxError;
use std::{
    env,
    path::{Path, PathBuf},
};

#[allow(unused)]
#[derive(Clone, Copy)]
enum FileUriPlatform {
    Unix,
    Windows,
}

pub fn is_file_uri(value: &str) -> bool {
    let trimmed = value.trim_start();
    trimmed
        .as_bytes()
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"file://"))
}

pub fn file_uri_for_path(path: &Path) -> Result<String, BoxError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };
    Ok(file_uri_for_absolute_path_string(
        &absolute.to_string_lossy(),
    ))
}

pub fn file_uri_for_absolute_path_string(path: &str) -> String {
    let path = file_uri_path_from_absolute_path_string(path);
    format!("file://{}", percent_encode_uri_path(&path))
}

pub fn path_from_file_uri(uri: &str) -> Result<PathBuf, BoxError> {
    #[cfg(windows)]
    {
        Ok(PathBuf::from(local_path_string_from_file_uri_for_platform(
            uri,
            FileUriPlatform::Windows,
        )?))
    }
    #[cfg(not(windows))]
    {
        Ok(PathBuf::from(local_path_string_from_file_uri_for_platform(
            uri,
            FileUriPlatform::Unix,
        )?))
    }
}

pub fn path_from_file_uri_or_path(value: &str) -> Result<PathBuf, BoxError> {
    let value = value.trim();
    if is_file_uri(value) {
        path_from_file_uri(value)
    } else {
        Ok(PathBuf::from(value))
    }
}

pub fn user_path_string_for_path(path: &Path) -> String {
    user_path_string_from_local_path_string(&path.to_string_lossy())
}

pub fn user_path_string_from_local_path_string(path: &str) -> String {
    if let Some(rest) = path
        .strip_prefix(r"\\?\UNC\")
        .or_else(|| path.strip_prefix(r"\\.\UNC\"))
    {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = path
        .strip_prefix(r"\\?\")
        .or_else(|| path.strip_prefix(r"\\.\"))
    {
        return rest.to_string();
    }
    path.to_string()
}

fn file_uri_path_from_absolute_path_string(path: &str) -> String {
    let mut path = strip_windows_verbatim_uri_path_prefix(&path.replace('\\', "/"));
    if windows_drive_uri_path_needs_leading_slash(&path) {
        path.insert(0, '/');
    }
    path
}

fn strip_windows_verbatim_uri_path_prefix(path: &str) -> String {
    if let Some(rest) = path
        .strip_prefix("//?/UNC/")
        .or_else(|| path.strip_prefix("//./UNC/"))
    {
        return format!("//{rest}");
    }
    if let Some(rest) = path
        .strip_prefix("//?/")
        .or_else(|| path.strip_prefix("//./"))
    {
        return rest.to_string();
    }
    path.to_string()
}

fn windows_drive_uri_path_needs_leading_slash(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

fn local_path_string_from_file_uri_for_platform(
    uri: &str,
    platform: FileUriPlatform,
) -> Result<String, BoxError> {
    let payload = file_uri_payload(uri)?;
    let (authority, raw_path) = split_file_uri_authority(payload);
    let authority = percent_decode_uri_path(authority)?;
    let path = percent_decode_uri_path(raw_path)?;

    match platform {
        FileUriPlatform::Unix => unix_path_string_from_file_uri_parts(&authority, &path),
        FileUriPlatform::Windows => Ok(windows_path_string_from_file_uri_parts(&authority, &path)),
    }
}

fn file_uri_payload(uri: &str) -> Result<&str, BoxError> {
    let trimmed = uri.trim();
    if !is_file_uri(trimmed) {
        return Err("local file URI must start with file://".into());
    }
    Ok(&trimmed[7..])
}

fn split_file_uri_authority(payload: &str) -> (&str, &str) {
    if payload.starts_with('/')
        || payload.starts_with('\\')
        || starts_with_windows_drive_spec(payload)
    {
        return ("", payload);
    }

    if let Some(index) = payload.find('/') {
        (&payload[..index], &payload[index..])
    } else {
        (payload, "")
    }
}

fn starts_with_windows_drive_spec(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

fn unix_path_string_from_file_uri_parts(authority: &str, path: &str) -> Result<String, BoxError> {
    if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
        return Err(format!("remote file URI authority is not supported: {authority}").into());
    }
    Ok(path.to_string())
}

fn windows_path_string_from_file_uri_parts(authority: &str, path: &str) -> String {
    if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
        let share_path = path.trim_start_matches(['/', '\\']).replace('/', "\\");
        if share_path.is_empty() {
            return format!(r"\\{authority}");
        }
        return format!(r"\\{authority}\{share_path}");
    }

    let path = path.replace('/', "\\");
    if path.starts_with(r"\\?\") || path.starts_with(r"\\.\") || path.starts_with(r"\\") {
        return path;
    }

    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'\\' && bytes[2] == b':' && bytes[1].is_ascii_alphabetic() {
        return path[1..].to_string();
    }

    path
}

fn percent_encode_uri_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                encoded.push(*byte as char)
            }
            byte => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn percent_decode_uri_path(path: &str) -> Result<String, BoxError> {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("file URI has an incomplete percent escape".into());
            }
            let hi = hex_value(bytes[index + 1])?;
            let lo = hex_value(bytes[index + 2])?;
            decoded.push((hi << 4) | lo);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| "file URI path is not valid UTF-8".into())
}

fn hex_value(byte: u8) -> Result<u8, BoxError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("file URI has an invalid percent escape".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_path_uses_file_uri_with_percent_encoding() {
        assert_eq!(
            file_uri_for_absolute_path_string("/tmp/hello world/中文.txt"),
            "file:///tmp/hello%20world/%E4%B8%AD%E6%96%87.txt"
        );
    }

    #[test]
    fn windows_drive_path_uses_file_uri_path_form() {
        assert_eq!(
            file_uri_for_absolute_path_string(r"C:\Users\me\hello world\中文.txt"),
            "file:///C:/Users/me/hello%20world/%E4%B8%AD%E6%96%87.txt"
        );
    }

    #[test]
    fn windows_extended_drive_path_strips_verbatim_prefix_for_uri() {
        assert_eq!(
            file_uri_for_absolute_path_string(r"\\?\D:\test\少有人走的路.pdf"),
            "file:///D:/test/%E5%B0%91%E6%9C%89%E4%BA%BA%E8%B5%B0%E7%9A%84%E8%B7%AF.pdf"
        );
    }

    #[test]
    fn windows_unc_paths_use_network_share_uri_path() {
        assert_eq!(
            file_uri_for_absolute_path_string(r"\\server\share\中文.txt"),
            "file:////server/share/%E4%B8%AD%E6%96%87.txt"
        );
        assert_eq!(
            file_uri_for_absolute_path_string(r"\\?\UNC\server\share\中文.txt"),
            "file:////server/share/%E4%B8%AD%E6%96%87.txt"
        );
    }

    #[test]
    fn windows_file_uri_decodes_drive_and_unc_forms() {
        assert_eq!(
            local_path_string_from_file_uri_for_platform(
                "file:///C:/Users/me/hello%20world.txt",
                FileUriPlatform::Windows
            )
            .unwrap(),
            r"C:\Users\me\hello world.txt"
        );
        assert_eq!(
            local_path_string_from_file_uri_for_platform(
                "file://localhost/C:/Users/me/%E4%B8%AD%E6%96%87.txt",
                FileUriPlatform::Windows
            )
            .unwrap(),
            r"C:\Users\me\中文.txt"
        );
        assert_eq!(
            local_path_string_from_file_uri_for_platform(
                "file:////server/share/a%20b.txt",
                FileUriPlatform::Windows
            )
            .unwrap(),
            r"\\server\share\a b.txt"
        );
        assert_eq!(
            local_path_string_from_file_uri_for_platform(
                "file://server/share/a%20b.txt",
                FileUriPlatform::Windows
            )
            .unwrap(),
            r"\\server\share\a b.txt"
        );
    }

    #[test]
    fn unix_file_uri_decodes_localhost_without_dropping_root_slash() {
        assert_eq!(
            local_path_string_from_file_uri_for_platform(
                "file://localhost/tmp/hello%20world.txt",
                FileUriPlatform::Unix
            )
            .unwrap(),
            "/tmp/hello world.txt"
        );
    }

    #[test]
    fn user_path_string_hides_windows_verbatim_prefix() {
        assert_eq!(
            user_path_string_from_local_path_string(r"\\?\D:\test\少有人走的路.pdf"),
            r"D:\test\少有人走的路.pdf"
        );
        assert_eq!(
            user_path_string_from_local_path_string(r"\\?\UNC\server\share\中文.txt"),
            r"\\server\share\中文.txt"
        );
    }
}
