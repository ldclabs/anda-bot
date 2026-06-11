use anda_core::{BoxError, ByteBufB64, Resource};
use anda_db::unix_ms;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::RwLock,
};

use crate::util::file_uri::file_uri_for_path;

pub type InferType = infer2::Type;

#[derive(Debug, Default)]
pub struct ChannelWorkspace {
    path: RwLock<Option<PathBuf>>,
}

impl ChannelWorkspace {
    pub fn set_path(&self, path: PathBuf) {
        *self.path.write().expect("channel workspace lock poisoned") = Some(path);
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.path
            .read()
            .expect("channel workspace lock poisoned")
            .clone()
    }

    pub async fn store_resource(
        &self,
        resource: &mut Resource,
        message_key: Option<&str>,
    ) -> Result<Option<PathBuf>, BoxError> {
        let Some(root) = self.path() else {
            return Ok(None);
        };
        let Some(blob) = resource.blob.as_ref() else {
            return Ok(None);
        };

        let dir = root.join("attachments");
        tokio::fs::create_dir_all(&dir).await?;

        let file_name = file_name_for_resource(resource);
        let stored_name = stored_attachment_name(message_key, &file_name);
        let path = unique_attachment_path(&dir, &stored_name).await?;
        tokio::fs::write(&path, &blob.0).await?;

        resource.uri = Some(file_uri_for_path(&path)?);
        if resource.size.is_none() {
            resource.size = Some(blob.0.len() as u64);
        }

        Ok(Some(path))
    }

    pub async fn store_resource_lossy(
        &self,
        resource: &mut Resource,
        message_key: Option<&str>,
        context: &str,
    ) {
        if let Err(err) = self.store_resource(resource, message_key).await {
            log::warn!("failed to store {context} in channel workspace: {err}");
        }
    }

    pub async fn store_resources_lossy(
        &self,
        resources: &mut [Resource],
        message_key: Option<&str>,
        context: &str,
    ) {
        for resource in resources {
            self.store_resource_lossy(resource, message_key, context)
                .await;
        }
    }
}

pub fn infer_from(file_name: &str, mime_type: Option<&str>) -> Option<InferType> {
    mime_type
        .and_then(infer2::get_from_mime)
        .or_else(|| infer2::get_from_filename(file_name))
}

pub fn infer_from_resource(resource: &Resource) -> Option<InferType> {
    resource
        .blob
        .as_ref()
        .and_then(|blob| infer2::get(&blob.0))
        .or_else(|| infer_from(&resource.name, resource.mime_type.as_deref()))
}

pub fn resource_from_bytes(file_name: String, bytes: Vec<u8>, description: &str) -> Resource {
    let size = bytes.len() as u64;
    infer_resource(Resource {
        name: file_name,
        description: Some(description.to_string()),
        blob: Some(ByteBufB64(bytes)),
        size: Some(size),
        ..Default::default()
    })
}

pub fn infer_resource(mut resource: Resource) -> Resource {
    if let Some(t) = infer_from_resource(&resource) {
        let tag = t.matcher_type().to_string();
        resource.mime_type = Some(t.mime_type().to_string());
        resource.name = if resource.name.trim().is_empty() {
            format!("{tag}.{}", t.extension())
        } else if !resource.name.trim().ends_with(t.extension()) {
            format!("{}.{}", resource.name.trim(), t.extension())
        } else {
            resource.name.trim().to_owned()
        };

        if !resource.tags.contains(&tag) {
            resource.tags.push(tag);
        }
    };

    resource
}

pub fn file_name_for_resource<'a>(resource: &'a Resource) -> Cow<'a, str> {
    if !resource.name.trim().is_empty() {
        Cow::Borrowed(resource.name.trim())
    } else if let Some(t) = infer_from_resource(resource) {
        Cow::Owned(format!("{}.{}", t.matcher_type(), t.extension()))
    } else {
        Cow::Borrowed("document.bin")
    }
}

pub fn is_http_url(target: &str) -> bool {
    target.starts_with("http://") || target.starts_with("https://")
}

fn stored_attachment_name(message_key: Option<&str>, file_name: &str) -> String {
    let file_name = sanitize_path_component(file_name, "attachment.bin");
    let prefix = message_key
        .and_then(|value| {
            let value = sanitize_path_component(value, "");
            (!value.is_empty()).then_some(value)
        })
        .unwrap_or_else(|| unix_ms().to_string());

    format!("{prefix}-{file_name}")
}

fn sanitize_path_component(value: &str, fallback: &str) -> String {
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

async fn unique_attachment_path(dir: &Path, file_name: &str) -> Result<PathBuf, BoxError> {
    let candidate = dir.join(file_name);
    if !tokio::fs::try_exists(&candidate).await? {
        return Ok(candidate);
    }

    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("attachment");
    let extension = path.extension().and_then(|extension| extension.to_str());

    for suffix in 1..1000 {
        let file_name = match extension {
            Some(extension) => format!("{stem}-{suffix}.{extension}"),
            None => format!("{stem}-{suffix}"),
        };
        let candidate = dir.join(file_name);
        if !tokio::fs::try_exists(&candidate).await? {
            return Ok(candidate);
        }
    }

    Err(format!(
        "unable to find available attachment path in {}",
        dir.display()
    )
    .into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::file_uri::path_from_file_uri;

    #[test]
    fn sanitize_path_component_keeps_safe_ascii_and_collapses_separators() {
        assert_eq!(
            sanitize_path_component(" report-01.json ", "fallback"),
            "report-01.json"
        );
        assert_eq!(
            sanitize_path_component("../奇怪 文件?.png", "fallback"),
            "png"
        );
        assert_eq!(
            sanitize_path_component("***", "fallback.bin"),
            "fallback.bin"
        );
        assert_eq!(
            sanitize_path_component(&"a".repeat(128), "fallback"),
            "a".repeat(96)
        );
    }

    #[test]
    fn stored_attachment_name_uses_sanitized_message_key_and_file_name() {
        assert_eq!(
            stored_attachment_name(Some(" chat/42 "), " report final.pdf "),
            "chat_42-report_final.pdf"
        );

        let generated = stored_attachment_name(Some("***"), "***");
        assert!(generated.ends_with("-attachment.bin"));
        assert!(
            generated[..generated.len() - "-attachment.bin".len()]
                .chars()
                .all(|ch| ch.is_ascii_digit())
        );
    }

    #[test]
    fn infer_resource_normalizes_name_mime_and_tags() {
        let resource = infer_resource(Resource {
            name: " photo ".to_string(),
            mime_type: Some("image/png".to_string()),
            ..Default::default()
        });

        assert_eq!(resource.name, "photo.png");
        assert_eq!(resource.mime_type.as_deref(), Some("image/png"));
        assert!(resource.tags.iter().any(|tag| tag == "Image"));
    }

    #[test]
    fn file_name_for_resource_uses_trimmed_name_or_inferred_fallback() {
        let named = Resource {
            name: " document.txt ".to_string(),
            ..Default::default()
        };
        assert_eq!(file_name_for_resource(&named), "document.txt");

        let inferred = Resource {
            name: String::new(),
            mime_type: Some("image/png".to_string()),
            ..Default::default()
        };
        assert_eq!(file_name_for_resource(&inferred), "Image.png");

        assert_eq!(file_name_for_resource(&Resource::default()), "document.bin");
    }

    #[test]
    fn resource_from_bytes_sets_blob_size_description_and_infers_when_possible() {
        let png = vec![
            0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 0, b'I', b'H', b'D', b'R',
        ];
        let resource = resource_from_bytes("screenshot".to_string(), png.clone(), "screen grab");

        assert_eq!(resource.description.as_deref(), Some("screen grab"));
        assert_eq!(resource.size, Some(png.len() as u64));
        assert_eq!(
            resource.blob.as_ref().map(|blob| blob.0.as_slice()),
            Some(png.as_slice())
        );
        assert_eq!(resource.name, "screenshot.png");
        assert_eq!(resource.mime_type.as_deref(), Some("image/png"));
    }

    #[test]
    fn http_url_detection_is_scheme_specific() {
        assert!(is_http_url("http://example.com/file"));
        assert!(is_http_url("https://example.com/file"));
        assert!(!is_http_url("ftp://example.com/file"));
        assert!(!is_http_url("HTTPS://example.com/file"));
    }

    #[tokio::test]
    async fn unique_attachment_path_adds_suffix_when_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("voice.mp3");
        tokio::fs::write(&existing, b"old").await.unwrap();

        let path = unique_attachment_path(dir.path(), "voice.mp3")
            .await
            .unwrap();

        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("voice-1.mp3")
        );
    }

    #[test]
    fn local_file_uri_prefixes_path_losslessly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("voice file.mp3");
        let uri = file_uri_for_path(&path).unwrap();

        assert!(uri.starts_with("file://"));
        assert!(uri.ends_with("/voice%20file.mp3"));
        assert_eq!(path_from_file_uri(&uri).unwrap(), path);
    }

    #[tokio::test]
    async fn store_resource_skips_missing_workspace_or_blob_and_dedupes_names() {
        let workspace = ChannelWorkspace::default();
        let mut with_blob = resource_from_bytes("a.txt".to_string(), b"data".to_vec(), "test");

        // No workspace path: storing is a no-op.
        assert!(
            workspace
                .store_resource(&mut with_blob, None)
                .await
                .unwrap()
                .is_none()
        );

        let dir = tempfile::tempdir().unwrap();
        workspace.set_path(dir.path().to_path_buf());

        // No blob: nothing to store.
        let mut without_blob = Resource {
            name: "b.txt".to_string(),
            ..Default::default()
        };
        assert!(
            workspace
                .store_resource(&mut without_blob, None)
                .await
                .unwrap()
                .is_none()
        );

        // Storing twice with the same message key produces unique file names.
        let first = workspace
            .store_resource(&mut with_blob, Some("m1"))
            .await
            .unwrap()
            .expect("stored path");
        let mut duplicate = resource_from_bytes("a.txt".to_string(), b"data2".to_vec(), "test");
        duplicate.size = None;
        let second = workspace
            .store_resource(&mut duplicate, Some("m1"))
            .await
            .unwrap()
            .expect("stored path");
        assert_ne!(first, second);
        assert_eq!(duplicate.size, Some(5));

        // The lossy variant tolerates errors silently.
        let mut lossy = resource_from_bytes("c.txt".to_string(), b"x".to_vec(), "test");
        workspace
            .store_resources_lossy(std::slice::from_mut(&mut lossy), None, "test attachment")
            .await;
        assert!(lossy.uri.is_some());
    }
}
