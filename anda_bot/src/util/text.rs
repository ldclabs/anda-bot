use std::{io, path::Path};

pub async fn read_text_file(path: impl AsRef<Path>) -> io::Result<String> {
    let path = path.as_ref();
    let bytes = tokio::fs::read(path).await?;
    decode_text_bytes(&bytes).ok_or_else(|| invalid_text_data(path))
}

pub fn read_text_file_sync(path: impl AsRef<Path>) -> io::Result<String> {
    let path = path.as_ref();
    let bytes = std::fs::read(path)?;
    decode_text_bytes(&bytes).ok_or_else(|| invalid_text_data(path))
}

fn decode_text_bytes(bytes: &[u8]) -> Option<String> {
    anda_core::text_from_bytes(bytes).map(|text| text.into_owned())
}

fn invalid_text_data(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "{} is not valid text for UTF-8 or the platform text encoding",
            path.display()
        ),
    )
}

#[cfg(test)]
fn decode_text_bytes_with_windows_code_page(bytes: &[u8], code_page: u32) -> Option<String> {
    anda_core::text_from_bytes_with_encoding(
        bytes,
        anda_core::windows_code_page_encoding(code_page),
    )
    .map(|text| text.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_text() {
        assert_eq!(
            decode_text_bytes("hello 中文".as_bytes()).as_deref(),
            Some("hello 中文")
        );
    }

    #[test]
    fn decodes_legacy_windows_text_with_core_helper() {
        let gbk = [0xD6, 0xD0, 0xCE, 0xC4];

        assert_eq!(
            decode_text_bytes_with_windows_code_page(&gbk, 936).as_deref(),
            Some("中文")
        );
    }

    #[test]
    fn rejects_control_heavy_binary() {
        assert!(decode_text_bytes(&[0, 0, 0, 0, 0, 0]).is_none());
    }
}
