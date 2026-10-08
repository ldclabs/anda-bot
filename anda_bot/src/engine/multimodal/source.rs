use anda_core::{
    BoxError, ByteBufB64, ContentPart, RequestMeta, Resource, inline_data_from_data_url,
};
use futures::StreamExt;
use reqwest::header::CONTENT_TYPE;
use std::path::PathBuf;

use super::catalog::{MediaKind, extension_from_name};
use crate::util::file_uri::{file_uri_for_path, is_file_uri, path_from_file_uri};
use crate::util::http_client::PublicUrlPolicy;
use crate::util::request_meta::keys;

pub(super) const MAX_MEDIA_FILE_SIZE_BYTES: u64 = 10 * 1024 * 1024;
/// Name given to a download whose URL path ends without a file name.
pub(super) const DEFAULT_URL_FILE_NAME: &str = "attachment";
const OCTET_STREAM: &str = "application/octet-stream";

/// Bytes read from a workspace path, an http(s) URL, or a data URL.
pub(super) struct LoadedSource {
    /// Where the bytes came from, for messages: the resolved path or the URL.
    pub(super) label: String,
    /// The file name, which type detection falls back to.
    pub(super) name: String,
    /// The `file://` or http(s) URI. A data URL is not kept: its bytes are
    /// already decoded.
    pub(super) uri: Option<String>,
    pub(super) mime_type: String,
    pub(super) data: Vec<u8>,
}

/// Reads media and attachments from the workspaces a caller may access and
/// from the URLs the public URL policy allows.
#[derive(Clone)]
pub(super) struct SourceLoader {
    workspaces: Vec<PathBuf>,
    public_url_policy: PublicUrlPolicy,
}

impl SourceLoader {
    pub(super) fn new(workspaces: Vec<PathBuf>, public_url_policy: PublicUrlPolicy) -> Self {
        Self {
            workspaces,
            public_url_policy,
        }
    }

    /// Loads a workspace path or a `file`, `http(s)` or `data` URL.
    pub(super) async fn load(
        &self,
        meta: &RequestMeta,
        location: &str,
    ) -> Result<LoadedSource, BoxError> {
        let location = location.trim();
        if location.is_empty() {
            return Err("location cannot be empty".into());
        }

        if is_data_url(location) {
            return load_data_url(location);
        }

        if let Ok(url) = reqwest::Url::parse(location) {
            match url.scheme() {
                "http" | "https" => return self.load_http_url(url).await,
                "file" => {}
                scheme if location.contains("://") => {
                    return Err(format!("unsupported URL scheme: {scheme}").into());
                }
                // A Windows drive path parses as a one-letter scheme.
                _ => {}
            }
        }

        self.load_path(meta, location).await
    }

    pub(super) async fn load_path(
        &self,
        meta: &RequestMeta,
        path: &str,
    ) -> Result<LoadedSource, BoxError> {
        let resolved = self.resolve_path(meta, path).await?;
        let metadata = tokio::fs::metadata(&resolved).await?;
        if !metadata.is_file() {
            return Err(format!("path is not a regular file: {}", resolved.display()).into());
        }
        ensure_size("file", metadata.len())?;

        let data = tokio::fs::read(&resolved).await?;
        let name = resolved
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(LoadedSource {
            label: resolved.to_string_lossy().into_owned(),
            uri: Some(file_uri_for_path(&resolved)?),
            mime_type: mime_type_for_data_or_name(&data, &name, None),
            name,
            data,
        })
    }

    pub(super) async fn load_http_url(&self, url: reqwest::Url) -> Result<LoadedSource, BoxError> {
        let response =
            crate::util::http_client::fetch_public_url(url.clone(), self.public_url_policy).await?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("failed to fetch {url}: {status}").into());
        }
        if let Some(content_length) = response.content_length() {
            ensure_size("URL", content_length)?;
        }

        let content_type = response_content_type(&response);
        let data = read_limited_response_bytes(response).await?;
        let name = url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(DEFAULT_URL_FILE_NAME)
            .to_string();
        Ok(LoadedSource {
            label: url.to_string(),
            uri: Some(url.to_string()),
            mime_type: mime_type_for_data_or_name(&data, &name, content_type.as_deref()),
            name,
            data,
        })
    }

    /// Resolves `path` to a file inside one of the caller's workspaces.
    pub(super) async fn resolve_path(
        &self,
        meta: &RequestMeta,
        path: &str,
    ) -> Result<PathBuf, BoxError> {
        resolve_media_path(meta, &self.workspaces, path).await
    }
}

fn load_data_url(data_url: &str) -> Result<LoadedSource, BoxError> {
    let (data, declared) = inline_data_from_data_url(data_url).ok_or("invalid data URL")?;
    ensure_size("data URL", data.len() as u64)?;
    Ok(LoadedSource {
        label: "data URL".to_string(),
        name: "data-url".to_string(),
        uri: None,
        mime_type: mime_type_for_data_or_name(&data, "", Some(&declared)),
        data: data.0,
    })
}

/// Message content for media loaded from a location, once it matches `kind`.
pub(super) fn media_content(
    kind: MediaKind,
    source: LoadedSource,
) -> Result<ContentPart, BoxError> {
    let matches = match MediaKind::from_mime_type(&source.mime_type) {
        Some(detected) => detected == kind,
        // An unrecognized type passes when the file extension names the kind.
        None => extension_from_name(&source.name).and_then(MediaKind::from_extension) == Some(kind),
    };
    if !matches {
        return Err(format!(
            "{} does not look like {} media ({})",
            source.label,
            kind.noun(),
            source.mime_type
        )
        .into());
    }

    Ok(ContentPart::InlineData {
        mime_type: source.mime_type,
        data: ByteBufB64(source.data),
    })
}

/// Message content for a stored attachment of `kind`: its bytes, or a URL the
/// model provider fetches itself.
pub(super) fn content_from_resource(
    kind: MediaKind,
    resource: Resource,
) -> Result<ContentPart, BoxError> {
    if MediaKind::from_resource(&resource) != kind {
        return Err(format!(
            "resource {} is not {} media",
            resource_label(&resource),
            kind.noun()
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
        ensure_size("media resource", blob.len() as u64)?;
        // The bytes name the exact format (a "PNG" that is really a BMP), but
        // a container sniffed as another kind, such as audio-only MP4, keeps
        // its declared type.
        let sniffed = mime_type_for_data_or_name(&blob, &name, mime_type.as_deref());
        let mime_type = match mime_type {
            Some(declared) if MediaKind::from_mime_type(&sniffed) != Some(kind) => declared,
            _ => sniffed,
        };
        return Ok(ContentPart::InlineData {
            mime_type,
            data: blob,
        });
    }

    if let Some(file_uri) = uri.filter(|uri| {
        uri.starts_with("https://") || uri.starts_with("http://") || uri.starts_with("data:")
    }) {
        return Ok(ContentPart::FileData {
            file_uri,
            mime_type,
        });
    }

    Err(format!("media resource {name} has no inline data or URI").into())
}

async fn resolve_media_path(
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
    let mut allowed_roots = Vec::new();
    for root in defaults {
        if let Ok(root) = tokio::fs::canonicalize(root).await {
            allowed_roots.push(root);
        }
    }
    if allowed_roots.is_empty() {
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
        if !allowed_roots.iter().any(|root| workspace.starts_with(root)) {
            errors.push(format!(
                "{} is not an authorized workspace",
                workspace.display()
            ));
            continue;
        }
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

fn workspaces_from_meta(meta: &RequestMeta, defaults: &[PathBuf]) -> Vec<PathBuf> {
    let mut workspaces = Vec::new();
    if let Some(workspace) = meta.get_extra_as::<PathBuf>(keys::WORKSPACE) {
        push_workspace(&mut workspaces, workspace);
    } else if let Some(extra_workspaces) = meta.get_extra_as::<Vec<PathBuf>>(keys::WORKSPACE) {
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

pub(super) fn is_data_url(url: &str) -> bool {
    url.trim()
        .as_bytes()
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"data:"))
}

fn ensure_size(what: &str, len: u64) -> Result<(), BoxError> {
    if len > MAX_MEDIA_FILE_SIZE_BYTES {
        return Err(
            format!("{what} is too large: {len} bytes, max {MAX_MEDIA_FILE_SIZE_BYTES}").into(),
        );
    }
    Ok(())
}

async fn read_limited_response_bytes(response: reqwest::Response) -> Result<Vec<u8>, BoxError> {
    let capacity = response
        .content_length()
        .unwrap_or_default()
        .min(MAX_MEDIA_FILE_SIZE_BYTES);
    let mut data = Vec::with_capacity(capacity as usize);
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        ensure_size("URL", (data.len() + chunk.len()) as u64)?;
        data.extend_from_slice(&chunk);
    }

    Ok(data)
}

fn response_content_type(response: &reqwest::Response) -> Option<String> {
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

/// The MIME type of `data`: a sniffed media type, else the declared type, else
/// one from the file name, else any sniffed type.
pub(super) fn mime_type_for_data_or_name(
    data: &[u8],
    name: &str,
    declared: Option<&str>,
) -> String {
    let inferred = infer2::get(data).map(|kind| kind.mime_type());
    if let Some(mime_type) =
        inferred.filter(|mime_type| MediaKind::from_mime_type(mime_type).is_some())
    {
        return mime_type.to_string();
    }

    if let Some(mime_type) = declared
        .and_then(normalize_mime_type)
        .filter(|mime_type| mime_type != OCTET_STREAM)
    {
        return mime_type;
    }

    mime_type_from_name(name)
        .or_else(|| inferred.map(str::to_string))
        .unwrap_or_else(|| OCTET_STREAM.to_string())
}

fn mime_type_from_name(name: &str) -> Option<String> {
    // infer2 matches extensions exactly, and camera files are often `IMG_0001.JPG`.
    infer2::get_from_filename(&name.to_ascii_lowercase()).map(|kind| kind.mime_type().to_string())
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

    fn loader(workspaces: Vec<PathBuf>) -> SourceLoader {
        SourceLoader::new(workspaces, PublicUrlPolicy::PublicOnly)
    }

    fn loaded(name: &str, mime_type: &str) -> LoadedSource {
        LoadedSource {
            label: format!("/workspace/{name}"),
            name: name.to_string(),
            uri: None,
            mime_type: mime_type.to_string(),
            data: Vec::new(),
        }
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
        let resource = Resource {
            name: "photo.bin".to_string(),
            blob: Some(ByteBufB64(PNG_SIGNATURE.to_vec())),
            ..Default::default()
        };

        let content = content_from_resource(MediaKind::Image, resource)
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
    fn content_from_resource_lets_bytes_correct_the_declared_format() {
        let mislabeled = Resource {
            name: "scan.png".to_string(),
            mime_type: Some("image/png".to_string()),
            blob: Some(ByteBufB64(b"BM\x00\x00\x00\x00\x00\x00\x00\x00".to_vec())),
            ..Default::default()
        };
        let content = content_from_resource(MediaKind::Image, mislabeled).unwrap();
        assert!(matches!(
            content,
            ContentPart::InlineData { mime_type, .. } if mime_type == "image/bmp"
        ));

        // The bytes do not move an attachment to another kind: an MP4
        // container declared as audio stays audio.
        let mp4 = [
            0, 0, 0, 0x18, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm', 0, 0, 0, 0, b'i', b's',
            b'o', b'm', b'm', b'p', b'4', b'2',
        ];
        assert_eq!(
            infer2::get(&mp4).map(|kind| kind.mime_type()),
            Some("video/mp4")
        );
        let audio = Resource {
            name: "voice.m4a".to_string(),
            mime_type: Some("audio/mp4".to_string()),
            blob: Some(ByteBufB64(mp4.to_vec())),
            ..Default::default()
        };
        let content = content_from_resource(MediaKind::Audio, audio).unwrap();
        assert!(matches!(
            content,
            ContentPart::InlineData { mime_type, .. } if mime_type == "audio/mp4"
        ));
    }

    #[test]
    fn content_from_resource_uses_file_uri_when_present() {
        let resource = Resource {
            name: "clip.mp4".to_string(),
            uri: Some("https://example.com/clip.mp4".to_string()),
            mime_type: Some("video/mp4".to_string()),
            ..Default::default()
        };

        let content = content_from_resource(MediaKind::Video, resource)
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
        let resource = Resource {
            name: "speech.mp3".to_string(),
            mime_type: Some("audio/mpeg".to_string()),
            ..Default::default()
        };

        let err = content_from_resource(MediaKind::Image, resource)
            .expect_err("audio resource should be rejected by image agent");

        assert!(err.to_string().contains("is not image media"));
    }

    #[tokio::test]
    async fn load_path_reads_a_workspace_file() {
        let dir = tempdir().expect("tempdir should be created");
        let file = dir.path().join("images/cat.png");
        fs::create_dir_all(file.parent().expect("parent path should exist"))
            .expect("image directory should be created");
        fs::write(&file, PNG_SIGNATURE).expect("image file should be written");

        let source = loader(vec![dir.path().to_path_buf()])
            .load_path(&RequestMeta::default(), "images/cat.png")
            .await
            .expect("workspace image should resolve");

        assert_eq!(source.name, "cat.png");
        assert!(source.label.ends_with("cat.png"));
        assert!(source.uri.as_deref().is_some_and(is_file_uri));
        match media_content(MediaKind::Image, source).unwrap() {
            ContentPart::InlineData { mime_type, data } => {
                assert_eq!(mime_type, "image/png");
                assert_eq!(data.0, PNG_SIGNATURE.to_vec());
            }
            other => panic!("expected inline data, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn load_path_rejects_directories() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("subdir")).unwrap();
        let err = loader(vec![dir.path().to_path_buf()])
            .load_path(&RequestMeta::default(), "subdir")
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("not a regular file"));
    }

    #[tokio::test]
    async fn load_fetches_http_url() {
        let url = spawn_media_http_server(PNG_SIGNATURE.to_vec(), "image/png").await;
        let loader = SourceLoader::new(Vec::new(), PublicUrlPolicy::AllowPrivateForTests);

        let source = loader
            .load(&RequestMeta::default(), &url)
            .await
            .expect("HTTP image URL should be accepted");

        assert_eq!(source.name, "media.png");
        assert_eq!(source.uri.as_deref(), Some(url.as_str()));
        assert_eq!(source.mime_type, "image/png");
        assert_eq!(source.data, PNG_SIGNATURE.to_vec());
    }

    #[tokio::test]
    async fn load_decodes_data_urls_without_keeping_them() {
        let data_url = format!(
            "data:image/png;base64,{}",
            BASE64_STANDARD.encode(PNG_SIGNATURE)
        );
        let source = loader(Vec::new())
            .load(&RequestMeta::default(), &data_url)
            .await
            .expect("base64 image data URL should be accepted");
        assert_eq!(source.mime_type, "image/png");
        assert_eq!(source.data, PNG_SIGNATURE.to_vec());
        assert_eq!(source.uri, None);

        let svg = load_data_url(
            "data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%2F%3E",
        )
        .expect("percent encoded SVG data URL should be accepted");
        assert_eq!(svg.mime_type, "image/svg+xml");
        assert_eq!(
            svg.data,
            br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#.to_vec()
        );

        let text = loader(Vec::new())
            .load(&RequestMeta::default(), "data:text/plain;base64,aGVsbG8=")
            .await
            .unwrap();
        assert_eq!(text.mime_type, "text/plain");
        assert_eq!(text.data, b"hello".to_vec());
    }

    #[tokio::test]
    async fn load_rejects_blank_locations_and_unknown_schemes() {
        let loader = loader(Vec::new());
        let err = loader
            .load(&RequestMeta::default(), "   ")
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("cannot be empty"));

        let err = loader
            .load(&RequestMeta::default(), "ftp://example.com/x")
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("unsupported URL scheme"));
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

        assert_eq!(mime_type_from_name("a.png").as_deref(), Some("image/png"));
        assert_eq!(
            mime_type_from_name("IMG_0001.JPG").as_deref(),
            Some("image/jpeg")
        );

        assert!(is_data_url(" DATA:abc"));
        assert!(!is_data_url("http://x"));
    }

    #[test]
    fn media_content_accepts_mime_or_extension() {
        assert!(media_content(MediaKind::Image, loaded("x.png", "image/png")).is_ok());
        // Unknown mime but matching extension passes.
        assert!(
            media_content(
                MediaKind::Audio,
                loaded("x.mp3", "application/octet-stream")
            )
            .is_ok()
        );
        let err = media_content(MediaKind::Image, loaded("x.mp3", "audio/mpeg"))
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("does not look like image media"));
        assert!(err.to_string().contains("/workspace/x.mp3"));
    }

    #[test]
    fn mime_type_for_data_or_name_priority() {
        // Inferred recognized media type wins.
        assert_eq!(
            mime_type_for_data_or_name(&PNG_SIGNATURE, "x.bin", None),
            "image/png"
        );
        // The declared type wins when inference is not media.
        assert_eq!(
            mime_type_for_data_or_name(b"plain", "x.bin", Some("text/markdown")),
            "text/markdown"
        );
        // Then the file name.
        assert_eq!(
            mime_type_for_data_or_name(b"plain", "a.PNG", Some(OCTET_STREAM)),
            "image/png"
        );
        assert_eq!(
            mime_type_for_data_or_name(b"plain", "noext", None),
            OCTET_STREAM
        );
    }

    #[tokio::test]
    async fn metadata_cannot_expand_allowed_roots() {
        let temp = tempdir().unwrap();
        let allowed = temp.path().join("allowed");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&allowed).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let file = outside.join("image.png");
        fs::write(&file, PNG_SIGNATURE).unwrap();
        let mut meta = RequestMeta::default();
        meta.extra
            .insert("workspace".into(), serde_json::json!(outside));
        let result = resolve_media_path(&meta, &[allowed], file.to_str().unwrap()).await;
        assert!(
            result.is_err(),
            "request metadata authorized a directory outside configured roots"
        );
    }
}
