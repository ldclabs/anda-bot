//! Extension ↔ MIME type mappings.

use std::path::Path;

/// Get MIME type from a filename extension. Returns `"application/octet-stream"` for unknown.
pub fn get_mime_from_filename(filename: &str) -> &'static str {
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext.to_ascii_lowercase().as_str() {
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" => "application/gzip",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => "application/octet-stream",
    }
}

/// Get file extension from MIME type. Returns `".bin"` for unknown.
pub fn get_extension_from_mime(mime_type: &str) -> &'static str {
    let ct = mime_type.split(';').next().unwrap_or("").trim();
    match ct.to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => ".jpg",
        "image/png" => ".png",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "image/bmp" => ".bmp",
        "video/mp4" => ".mp4",
        "video/quicktime" => ".mov",
        "video/webm" => ".webm",
        "video/x-matroska" => ".mkv",
        "video/x-msvideo" => ".avi",
        "audio/mpeg" => ".mp3",
        "audio/ogg" => ".ogg",
        "audio/wav" => ".wav",
        "application/pdf" => ".pdf",
        "application/zip" => ".zip",
        "application/x-tar" => ".tar",
        "application/gzip" => ".gz",
        "text/plain" => ".txt",
        "text/csv" => ".csv",
        _ => ".bin",
    }
}

/// Get extension from Content-Type header or URL path. Returns `".bin"` for unknown.
pub fn get_extension_from_content_type_or_url(
    content_type: Option<&str>,
    url: &str,
) -> &'static str {
    if let Some(ct) = content_type {
        let ext = get_extension_from_mime(ct);
        if ext != ".bin" {
            return ext;
        }
    }
    if let Ok(parsed) = url::Url::parse(url) {
        let path = parsed.path();
        if let Some(dot_pos) = path.rfind('.') {
            let url_ext = &path[dot_pos..];
            // Check if it's a known extension
            let test_name = format!("test{url_ext}");
            let mime = get_mime_from_filename(&test_name);
            if mime != "application/octet-stream" {
                return get_extension_from_mime(mime);
            }
        }
    }
    ".bin"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_from_known_extensions() {
        assert_eq!(get_mime_from_filename("photo.png"), "image/png");
        assert_eq!(get_mime_from_filename("doc.PDF"), "application/pdf");
        assert_eq!(get_mime_from_filename("video.mp4"), "video/mp4");
        assert_eq!(get_mime_from_filename("song.mp3"), "audio/mpeg");
    }

    #[test]
    fn mime_from_unknown_extension() {
        assert_eq!(
            get_mime_from_filename("file.xyz"),
            "application/octet-stream"
        );
        assert_eq!(get_mime_from_filename("noext"), "application/octet-stream");
    }

    #[test]
    fn extension_from_known_mime() {
        assert_eq!(get_extension_from_mime("image/png"), ".png");
        assert_eq!(get_extension_from_mime("video/mp4"), ".mp4");
        assert_eq!(get_extension_from_mime("image/jpeg; charset=utf-8"), ".jpg");
    }

    #[test]
    fn extension_from_unknown_mime() {
        assert_eq!(get_extension_from_mime("application/x-custom"), ".bin");
    }

    #[test]
    fn extension_from_content_type_or_url() {
        // Content-Type takes priority
        assert_eq!(
            get_extension_from_content_type_or_url(Some("image/png"), "https://x.com/f.jpg"),
            ".png"
        );
        // Falls back to URL extension
        assert_eq!(
            get_extension_from_content_type_or_url(None, "https://x.com/file.mp4"),
            ".mp4"
        );
        // Unknown both
        assert_eq!(
            get_extension_from_content_type_or_url(None, "https://x.com/file"),
            ".bin"
        );
        // Unknown content-type, known URL
        assert_eq!(
            get_extension_from_content_type_or_url(
                Some("application/x-custom"),
                "https://x.com/f.pdf"
            ),
            ".pdf"
        );
    }
}
