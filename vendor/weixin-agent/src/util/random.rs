//! ID and filename generation utilities.

use std::time::{SystemTime, UNIX_EPOCH};

/// Generate a prefixed unique ID: `{prefix}:{timestamp}-{8hex}`.
pub fn generate_id(prefix: &str) -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let hex = random_hex(4);
    format!("{prefix}:{ts}-{hex}")
}

/// Generate a temporary file name: `{prefix}-{timestamp}-{8hex}{ext}`.
pub fn temp_file_name(prefix: &str, ext: &str) -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let hex = random_hex(4);
    format!("{prefix}-{ts}-{hex}{ext}")
}

/// Generate `n` random bytes as a hex string (2*n chars).
pub fn random_hex(n: usize) -> String {
    use rand::Rng;
    use std::fmt::Write;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..n).map(|_| rng.random()).collect();
    bytes
        .iter()
        .fold(String::with_capacity(n * 2), |mut acc, b| {
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_id_format() {
        let id = generate_id("test");
        assert!(id.starts_with("test:"));
        let rest = &id["test:".len()..];
        let parts: Vec<&str> = rest.splitn(2, '-').collect();
        assert_eq!(parts.len(), 2);
        assert!(parts[0].parse::<u128>().is_ok()); // timestamp
        assert_eq!(parts[1].len(), 8); // 4 bytes = 8 hex chars
    }

    #[test]
    fn random_hex_length() {
        assert_eq!(random_hex(0).len(), 0);
        assert_eq!(random_hex(1).len(), 2);
        assert_eq!(random_hex(4).len(), 8);
        assert_eq!(random_hex(16).len(), 32);
        assert!(random_hex(4).chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn temp_file_name_format() {
        let name = temp_file_name("upload", ".png");
        assert!(name.starts_with("upload-"));
        assert!(
            std::path::Path::new(&name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
        );
        let mid = &name["upload-".len()..name.len() - ".png".len()];
        let parts: Vec<&str> = mid.splitn(2, '-').collect();
        assert_eq!(parts.len(), 2);
    }
}
