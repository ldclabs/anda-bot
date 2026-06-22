use anda_core::{
    BoxError, ByteBufB64, ContentPart, RequestMeta, Resource, inline_data_from_data_url,
};
use futures_util::StreamExt;
use reqwest::header::CONTENT_TYPE;
use std::path::{Path, PathBuf};

use super::catalog::MediaKind;
use crate::util::file_uri::{is_file_uri, path_from_file_uri};

pub(super) const MAX_MEDIA_FILE_SIZE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct MediaSourceLoader {
    kind: MediaKind,
    workspaces: Vec<PathBuf>,
    http: reqwest::Client,
}

impl MediaSourceLoader {
    pub(super) fn new(kind: MediaKind, workspaces: Vec<PathBuf>, http: reqwest::Client) -> Self {
        Self {
            kind,
            workspaces,
            http,
        }
    }

    pub(super) async fn content_from_location(
        &self,
        meta: &RequestMeta,
        location: &str,
    ) -> Result<ContentPart, BoxError> {
        let location = location.trim();
        if location.is_empty() {
            return Err("media location cannot be empty".into());
        }

        if strip_data_url_scheme(location).is_some() {
            return self.content_from_data_url(location);
        }

        if let Ok(url) = reqwest::Url::parse(location) {
            return match url.scheme() {
                "http" | "https" => self.content_from_http_url(url).await,
                "file" => self.content_from_path(meta, location).await,
                scheme if location.contains("://") => {
                    Err(format!("unsupported media URL scheme: {scheme}").into())
                }
                _ => self.content_from_path(meta, location).await,
            };
        }

        self.content_from_path(meta, location).await
    }

    pub(super) async fn content_from_path(
        &self,
        meta: &RequestMeta,
        path: &str,
    ) -> Result<ContentPart, BoxError> {
        let resolved = resolve_media_path(meta, &self.workspaces, path).await?;
        let metadata = tokio::fs::metadata(&resolved).await?;
        if !metadata.is_file() {
            return Err(format!("media path is not a regular file: {}", resolved.display()).into());
        }
        if metadata.len() > MAX_MEDIA_FILE_SIZE_BYTES {
            return Err(format!(
                "media file is too large: {} bytes, max {MAX_MEDIA_FILE_SIZE_BYTES}",
                metadata.len()
            )
            .into());
        }

        let data = tokio::fs::read(&resolved).await?;
        let mime_type = mime_type_for_data_or_path(&data, &resolved, "application/octet-stream");
        let source = resolved.to_string_lossy();
        ensure_media_kind(self.kind, &mime_type, source.as_ref())?;

        Ok(ContentPart::InlineData {
            mime_type,
            data: ByteBufB64(data),
        })
    }

    pub(super) async fn content_from_http_url(
        &self,
        url: reqwest::Url,
    ) -> Result<ContentPart, BoxError> {
        let response = self.http.get(url.clone()).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("failed to fetch media URL {url}: {status}").into());
        }

        if let Some(content_length) = response.content_length()
            && content_length > MAX_MEDIA_FILE_SIZE_BYTES
        {
            return Err(format!(
                "media URL is too large: {content_length} bytes, max {MAX_MEDIA_FILE_SIZE_BYTES}"
            )
            .into());
        }

        let content_type = response_content_type(&response);
        let data = read_limited_response_bytes(response).await?;
        let mime_type = mime_type_for_data_or_name(
            &data,
            url.path(),
            content_type.as_deref(),
            "application/octet-stream",
        );
        ensure_media_kind(self.kind, &mime_type, url.as_str())?;

        Ok(ContentPart::InlineData {
            mime_type,
            data: ByteBufB64(data),
        })
    }

    pub(super) fn content_from_data_url(&self, data_url: &str) -> Result<ContentPart, BoxError> {
        let (data, mime_type) = inline_data_from_data_url(data_url)
            .ok_or_else(|| "invalid media data URL".to_string())?;
        if data.len() as u64 > MAX_MEDIA_FILE_SIZE_BYTES {
            return Err(format!(
                "media data URL is too large: {} bytes, max {MAX_MEDIA_FILE_SIZE_BYTES}",
                data.len()
            )
            .into());
        }

        ensure_media_kind(self.kind, &mime_type, "data URL")?;

        Ok(ContentPart::InlineData { mime_type, data })
    }

    pub(super) fn content_from_resource(
        &self,
        resource: Resource,
    ) -> Result<ContentPart, BoxError> {
        if MediaKind::from_resource(&resource) != Some(self.kind) {
            return Err(format!(
                "resource {} is not {} media",
                resource_label(&resource),
                self.kind.noun()
            )
            .into());
        }

        let Resource {
            name,
            mime_type,
            blob,
            uri,
            ..
        } = resource;

        if let Some(blob) = blob {
            if blob.0.len() as u64 > MAX_MEDIA_FILE_SIZE_BYTES {
                return Err(format!(
                    "media resource is too large: {} bytes, max {MAX_MEDIA_FILE_SIZE_BYTES}",
                    blob.0.len()
                )
                .into());
            }

            let mime_type = mime_type.unwrap_or_else(|| {
                infer2::get(&blob.0)
                    .map(|kind| kind.mime_type().to_string())
                    .or_else(|| mime_type_from_name(&name))
                    .unwrap_or_else(|| "application/octet-stream".to_string())
            });

            return Ok(ContentPart::InlineData {
                mime_type,
                data: blob,
            });
        }

        if let Some(file_uri) = uri.filter(|uri| !uri.trim().is_empty())
            && (file_uri.starts_with("https://")
                || file_uri.starts_with("http://")
                || file_uri.starts_with("data:"))
        {
            return Ok(ContentPart::FileData {
                file_uri,
                mime_type,
            });
        }

        Err(format!("media resource {} has no inline data or URI", name).into())
    }
}

pub(super) async fn resolve_media_path(
    meta: &RequestMeta,
    defaults: &[PathBuf],
    user_path: &str,
) -> Result<PathBuf, BoxError> {
    let user_path = user_path.trim();
    let requested = if is_file_uri(user_path) {
        path_from_file_uri(user_path)?
    } else {
        PathBuf::from(user_path)
    };
    if requested.as_os_str().is_empty() {
        return Err("media path cannot be empty".into());
    }

    let workspaces = workspaces_from_meta(meta, defaults);
    if workspaces.is_empty() {
        return Err("no workspace is configured for media file access".into());
    }

    let mut errors = Vec::new();
    for workspace in workspaces {
        let workspace = match tokio::fs::canonicalize(&workspace).await {
            Ok(path) => path,
            Err(err) => {
                errors.push(format!("{}: {err}", workspace.display()));
                continue;
            }
        };
        let candidate = if requested.is_absolute() {
            requested.clone()
        } else {
            workspace.join(&requested)
        };

        match tokio::fs::canonicalize(&candidate).await {
            Ok(path) if path.starts_with(&workspace) => return Ok(path),
            Ok(path) => errors.push(format!(
                "{} resolves outside workspace {}",
                path.display(),
                workspace.display()
            )),
            Err(err) => errors.push(format!("{}: {err}", candidate.display())),
        }
    }

    Err(format!(
        "media path is not readable from configured workspaces: {} ({})",
        requested.display(),
        errors.join("; ")
    )
    .into())
}

pub(super) fn workspaces_from_meta(meta: &RequestMeta, defaults: &[PathBuf]) -> Vec<PathBuf> {
    let mut workspaces = Vec::new();
    if let Some(workspace) = meta.get_extra_as::<PathBuf>("workspace") {
        push_workspace(&mut workspaces, workspace);
    } else if let Some(extra_workspaces) = meta.get_extra_as::<Vec<PathBuf>>("workspace") {
        for workspace in extra_workspaces {
            push_workspace(&mut workspaces, workspace);
        }
    }

    if let Some(workspace) = meta.get_extra_as::<PathBuf>("workspaces") {
        push_workspace(&mut workspaces, workspace);
    } else if let Some(extra_workspaces) = meta.get_extra_as::<Vec<PathBuf>>("workspaces") {
        for workspace in extra_workspaces {
            push_workspace(&mut workspaces, workspace);
        }
    }

    for workspace in defaults {
        push_workspace(&mut workspaces, workspace.clone());
    }

    workspaces
}

fn push_workspace(workspaces: &mut Vec<PathBuf>, workspace: PathBuf) {
    if workspace.as_os_str().is_empty() {
        return;
    }
    if !workspaces.iter().any(|existing| existing == &workspace) {
        workspaces.push(workspace);
    }
}

pub(super) fn strip_data_url_scheme(url: &str) -> Option<&str> {
    let trimmed = url.trim();
    if trimmed
        .as_bytes()
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"data:"))
    {
        Some(&trimmed[5..])
    } else {
        None
    }
}

pub(super) async fn read_limited_response_bytes(
    response: reqwest::Response,
) -> Result<Vec<u8>, BoxError> {
    let mut data = Vec::new();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let next_len = data.len() + chunk.len();
        if next_len as u64 > MAX_MEDIA_FILE_SIZE_BYTES {
            return Err(format!(
                "media URL is too large: at least {next_len} bytes, max {MAX_MEDIA_FILE_SIZE_BYTES}"
            )
            .into());
        }
        data.extend_from_slice(&chunk);
    }

    Ok(data)
}

pub(super) fn response_content_type(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(normalize_mime_type)
}

pub(super) fn normalize_mime_type(value: &str) -> Option<String> {
    value
        .split(';')
        .next()
        .map(str::trim)
        .filter(|mime_type| !mime_type.is_empty())
        .map(str::to_ascii_lowercase)
}

pub(super) fn ensure_media_kind(
    kind: MediaKind,
    mime_type: &str,
    source_name: &str,
) -> Result<(), BoxError> {
    let detected = MediaKind::from_mime_type(mime_type);
    if detected == Some(kind) {
        return Ok(());
    }

    if detected.is_none()
        && extension_from_name(source_name).and_then(MediaKind::from_extension) == Some(kind)
    {
        return Ok(());
    }

    Err(format!(
        "media source does not look like {} media: {} ({mime_type})",
        kind.noun(),
        source_name
    )
    .into())
}

pub(super) fn mime_type_for_data_or_path(data: &[u8], path: &Path, fallback: &str) -> String {
    let name = path.to_string_lossy();
    mime_type_for_data_or_name(data, name.as_ref(), None, fallback)
}

pub(super) fn mime_type_for_data_or_name(
    data: &[u8],
    name: &str,
    preferred: Option<&str>,
    fallback: &str,
) -> String {
    let inferred = infer2::get(data).map(|kind| kind.mime_type().to_string());
    if let Some(mime_type) = inferred
        .as_deref()
        .filter(|mime_type| MediaKind::from_mime_type(mime_type).is_some())
    {
        return mime_type.to_string();
    }

    let preferred = preferred.and_then(normalize_mime_type);
    if let Some(mime_type) = preferred
        .as_deref()
        .filter(|mime_type| *mime_type != "application/octet-stream")
    {
        return mime_type.to_string();
    }

    if let Some(mime_type) = mime_type_from_name(name) {
        return mime_type;
    }

    if let Some(mime_type) = inferred {
        return mime_type;
    }

    preferred.unwrap_or_else(|| fallback.to_string())
}

pub(super) fn mime_type_from_name(name: &str) -> Option<String> {
    infer2::get_from_filename(name).map(|kind| kind.mime_type().to_string())
}

pub(super) fn extension_from_name(name: &str) -> Option<&str> {
    Path::new(name).extension().and_then(|ext| ext.to_str())
}

pub(super) fn resource_label(resource: &Resource) -> String {
    if !resource.name.trim().is_empty() {
        resource.name.trim().to_string()
    } else if let Some(uri) = resource.uri.as_deref().filter(|uri| !uri.trim().is_empty()) {
        uri.to_string()
    } else if resource._id > 0 {
        format!("resource-{}", resource._id)
    } else {
        "unnamed resource".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];

    async fn spawn_media_http_server(body: Vec<u8>, content_type: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test server should bind");
        let addr = listener
            .local_addr()
            .expect("test server address should be available");

        tokio::spawn(async move {
            let (mut socket, _) = listener
                .accept()
                .await
                .expect("test server should accept one request");
            let mut request = [0; 1024];
            let _ = socket.read(&mut request).await;
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket
                .write_all(headers.as_bytes())
                .await
                .expect("test response headers should write");
            socket
                .write_all(&body)
                .await
                .expect("test response body should write");
        });

        format!("http://{addr}/media.png")
    }

    #[test]
    fn resource_label_falls_back_from_name_to_uri_to_id() {
        let named = Resource {
            name: "  cat.png  ".to_string(),
            uri: Some("file:///tmp/cat.png".to_string()),
            _id: 7,
            ..Default::default()
        };
        let uri_only = Resource {
            name: "   ".to_string(),
            uri: Some("file:///tmp/cat.png".to_string()),
            _id: 7,
            ..Default::default()
        };
        let id_only = Resource {
            name: "   ".to_string(),
            _id: 7,
            ..Default::default()
        };

        assert_eq!(resource_label(&named), "cat.png");
        assert_eq!(resource_label(&uri_only), "file:///tmp/cat.png");
        assert_eq!(resource_label(&id_only), "resource-7");
        assert_eq!(resource_label(&Resource::default()), "unnamed resource");
    }

    #[test]
    fn workspaces_from_meta_merges_and_deduplicates() {
        let workspace1 = PathBuf::from("/tmp/workspace-1");
        let workspace2 = PathBuf::from("/tmp/workspace-2");
        let workspace3 = PathBuf::from("/tmp/workspace-3");
        let mut meta = RequestMeta::default();
        meta.extra
            .insert("workspace".to_string(), json!(workspace1.clone()));
        meta.extra.insert(
            "workspaces".to_string(),
            json!([workspace1.clone(), workspace2.clone(), ""]),
        );

        let workspaces = workspaces_from_meta(&meta, &[workspace2.clone(), workspace3.clone()]);

        assert_eq!(workspaces, vec![workspace1, workspace2, workspace3]);
    }

    #[test]
    fn content_from_resource_infers_inline_blob_mime_type() {
        let loader = MediaSourceLoader::new(
            MediaKind::Image,
            Vec::new(),
            crate::util::http_client::new_reqwest_client(),
        );
        let resource = Resource {
            name: "photo.bin".to_string(),
            blob: Some(ByteBufB64(PNG_SIGNATURE.to_vec())),
            ..Default::default()
        };

        let content = loader
            .content_from_resource(resource)
            .expect("image blob should be accepted");

        match content {
            ContentPart::InlineData { mime_type, data } => {
                assert_eq!(mime_type, "image/png");
                assert_eq!(data.0, PNG_SIGNATURE.to_vec());
            }
            other => panic!("expected inline data, got {other:?}"),
        }
    }

    #[test]
    fn content_from_resource_uses_file_uri_when_present() {
        let loader = MediaSourceLoader::new(
            MediaKind::Video,
            Vec::new(),
            crate::util::http_client::new_reqwest_client(),
        );
        let resource = Resource {
            name: "clip.mp4".to_string(),
            uri: Some("https://example.com/clip.mp4".to_string()),
            mime_type: Some("video/mp4".to_string()),
            ..Default::default()
        };

        let content = loader
            .content_from_resource(resource)
            .expect("video uri should be accepted");

        match content {
            ContentPart::FileData {
                file_uri,
                mime_type,
            } => {
                assert_eq!(file_uri, "https://example.com/clip.mp4");
                assert_eq!(mime_type.as_deref(), Some("video/mp4"));
            }
            other => panic!("expected file data, got {other:?}"),
        }
    }

    #[test]
    fn content_from_resource_rejects_mismatched_media_kind() {
        let loader = MediaSourceLoader::new(
            MediaKind::Image,
            Vec::new(),
            crate::util::http_client::new_reqwest_client(),
        );
        let resource = Resource {
            name: "speech.mp3".to_string(),
            mime_type: Some("audio/mpeg".to_string()),
            ..Default::default()
        };

        let err = loader
            .content_from_resource(resource)
            .expect_err("audio resource should be rejected by image agent");

        assert!(err.to_string().contains("is not image media"));
    }

    #[tokio::test]
    async fn content_from_path_reads_workspace_file() {
        let dir = tempdir().expect("tempdir should be created");
        let file = dir.path().join("images/cat.png");
        fs::create_dir_all(file.parent().expect("parent path should exist"))
            .expect("image directory should be created");
        fs::write(&file, PNG_SIGNATURE).expect("image file should be written");

        let loader = MediaSourceLoader::new(
            MediaKind::Image,
            vec![dir.path().to_path_buf()],
            crate::util::http_client::new_reqwest_client(),
        );
        let content = loader
            .content_from_path(&RequestMeta::default(), "images/cat.png")
            .await
            .expect("workspace image should resolve");

        match content {
            ContentPart::InlineData { mime_type, data } => {
                assert_eq!(mime_type, "image/png");
                assert_eq!(data.0, PNG_SIGNATURE.to_vec());
            }
            other => panic!("expected inline data, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn content_from_location_fetches_http_url() {
        let url = spawn_media_http_server(PNG_SIGNATURE.to_vec(), "image/png").await;
        let loader = MediaSourceLoader::new(
            MediaKind::Image,
            Vec::new(),
            reqwest::Client::builder()
                .no_proxy()
                .build()
                .expect("test HTTP client should build"),
        );

        let content = loader
            .content_from_location(&RequestMeta::default(), &url)
            .await
            .expect("HTTP image URL should be accepted");

        match content {
            ContentPart::InlineData { mime_type, data } => {
                assert_eq!(mime_type, "image/png");
                assert_eq!(data.0, PNG_SIGNATURE.to_vec());
            }
            other => panic!("expected inline data, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn content_from_location_decodes_base64_data_url() {
        let data_url = format!(
            "data:image/png;base64,{}",
            BASE64_STANDARD.encode(PNG_SIGNATURE)
        );
        let loader = MediaSourceLoader::new(
            MediaKind::Image,
            Vec::new(),
            crate::util::http_client::new_reqwest_client(),
        );

        let content = loader
            .content_from_location(&RequestMeta::default(), &data_url)
            .await
            .expect("base64 image data URL should be accepted");

        match content {
            ContentPart::InlineData { mime_type, data } => {
                assert_eq!(mime_type, "image/png");
                assert_eq!(data.0, PNG_SIGNATURE.to_vec());
            }
            other => panic!("expected inline data, got {other:?}"),
        }
    }

    #[test]
    fn content_from_data_url_decodes_percent_encoded_payload() {
        let loader = MediaSourceLoader::new(
            MediaKind::Image,
            Vec::new(),
            crate::util::http_client::new_reqwest_client(),
        );
        let content = loader
            .content_from_data_url("data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%2F%3E")
            .expect("percent encoded SVG data URL should be accepted");

        match content {
            ContentPart::InlineData { mime_type, data } => {
                assert_eq!(mime_type, "image/svg+xml");
                assert_eq!(
                    data.0,
                    br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#.to_vec()
                );
            }
            other => panic!("expected inline data, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolve_media_path_accepts_file_uri_within_workspace() {
        let dir = tempdir().expect("tempdir should be created");
        let file = dir.path().join("cat.png");
        fs::write(&file, PNG_SIGNATURE).expect("image file should be written");
        let file = file.canonicalize().expect("file should canonicalize");

        let resolved = resolve_media_path(
            &RequestMeta::default(),
            &[dir.path().to_path_buf()],
            &crate::util::file_uri::file_uri_for_path(&file).expect("file URI should be generated"),
        )
        .await
        .expect("file uri inside workspace should resolve");

        assert_eq!(resolved, file);
    }

    #[tokio::test]
    async fn resolve_media_path_rejects_absolute_path_outside_workspace() {
        let dir = tempdir().expect("tempdir should be created");
        let workspace = dir.path().join("workspace");
        let outside = dir.path().join("outside.png");
        fs::create_dir_all(&workspace).expect("workspace should be created");
        fs::write(&outside, PNG_SIGNATURE).expect("outside file should be written");

        let err = resolve_media_path(
            &RequestMeta::default(),
            &[workspace],
            outside.to_str().expect("path should be utf-8"),
        )
        .await
        .expect_err("outside file should be rejected");

        assert!(err.to_string().contains("resolves outside workspace"));
    }

    #[test]
    fn pure_mime_and_path_helpers() {
        assert_eq!(
            normalize_mime_type(" Text/Plain; charset=utf-8 ").as_deref(),
            Some("text/plain")
        );
        assert_eq!(normalize_mime_type("   "), None);

        assert_eq!(extension_from_name("dir/file.RS"), Some("RS"));
        assert_eq!(extension_from_name("noext"), None);
        assert_eq!(mime_type_from_name("a.png").as_deref(), Some("image/png"));

        assert_eq!(strip_data_url_scheme(" DATA:abc"), Some("abc"));
        assert_eq!(strip_data_url_scheme("http://x"), None);
    }

    #[test]
    fn ensure_media_kind_accepts_mime_or_extension() {
        assert!(ensure_media_kind(MediaKind::Image, "image/png", "x.png").is_ok());
        // Unknown mime but matching extension passes.
        assert!(ensure_media_kind(MediaKind::Audio, "application/octet-stream", "x.mp3").is_ok());
        let err = ensure_media_kind(MediaKind::Image, "audio/mpeg", "x.mp3")
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("does not look like image media"));
    }

    #[test]
    fn mime_type_for_data_or_name_priority() {
        let png = [0x89u8, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
        // Inferred recognized media type wins.
        assert_eq!(
            mime_type_for_data_or_name(&png, "x.bin", None, "application/octet-stream"),
            "image/png"
        );
        // Preferred (non octet-stream) wins when inference is not media.
        assert_eq!(
            mime_type_for_data_or_name(b"plain", "x.bin", Some("text/markdown"), "fallback"),
            "text/markdown"
        );
        // Falls back to fallback when nothing else resolves.
        assert_eq!(
            mime_type_for_data_or_name(b"plain", "noext", None, "fallback/type"),
            "fallback/type"
        );
        assert_eq!(
            mime_type_for_data_or_path(&png, Path::new("x.bin"), "fallback"),
            "image/png"
        );
    }
}
