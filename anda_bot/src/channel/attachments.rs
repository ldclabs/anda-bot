use anda_core::{BoxError, ByteBufB64, Resource};
use anda_db::unix_ms;
use std::{
    path::{Path, PathBuf},
    sync::RwLock,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Image,
    Audio,
    Video,
    Document,
}

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

        resource.uri = Some(local_file_uri(&path));
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

pub fn attachment_kind(file_name: &str, mime_type: Option<&str>) -> AttachmentKind {
    let mime_type = mime_type.unwrap_or_default().to_ascii_lowercase();
    if mime_type.starts_with("image/") {
        AttachmentKind::Image
    } else if mime_type.starts_with("audio/") {
        AttachmentKind::Audio
    } else if mime_type.starts_with("video/") {
        AttachmentKind::Video
    } else if let Some(extension) = file_extension(file_name) {
        match extension.as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "tiff" | "svg" => {
                AttachmentKind::Image
            }
            "mp3" | "m4a" | "wav" | "ogg" | "oga" | "opus" | "flac" | "aac" => {
                AttachmentKind::Audio
            }
            "mp4" | "mov" | "webm" | "mkv" | "avi" | "wmv" | "flv" => AttachmentKind::Video,
            _ => AttachmentKind::Document,
        }
    } else {
        AttachmentKind::Document
    }
}

pub fn mime_type_for_path(path: &str) -> Option<&'static str> {
    match file_extension(path)?.as_str() {
        "txt" => Some("text/plain"),
        "md" => Some("text/markdown"),
        "json" => Some("application/json"),
        "csv" => Some("text/csv"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        "mp3" => Some("audio/mpeg"),
        "m4a" => Some("audio/mp4"),
        "wav" => Some("audio/wav"),
        "ogg" | "oga" | "opus" => Some("audio/ogg"),
        "flac" => Some("audio/flac"),
        "aac" => Some("audio/aac"),
        "webm" => Some("video/webm"),
        "mp4" => Some("video/mp4"),
        "mov" => Some("video/quicktime"),
        "pdf" => Some("application/pdf"),
        _ => None,
    }
}

pub fn resource_tags(
    kind: AttachmentKind,
    file_name: &str,
    mime_type: Option<&str>,
) -> Vec<String> {
    let mut tags = Vec::new();
    let mime_type = mime_type.unwrap_or_default();
    if mime_type.starts_with("text/") || file_name.ends_with(".txt") {
        tags.push("text".to_string());
    } else if file_name.ends_with(".md") || mime_type == "text/markdown" {
        tags.push("md".to_string());
    } else {
        tags.push(
            match kind {
                AttachmentKind::Image => "image",
                AttachmentKind::Audio => "audio",
                AttachmentKind::Video => "video",
                AttachmentKind::Document => "document",
            }
            .to_string(),
        );
    }

    if let Some(extension) = file_extension(file_name)
        && !tags.iter().any(|tag| tag == &extension)
    {
        tags.push(extension);
    }

    tags
}

pub fn resource_from_bytes(
    kind: AttachmentKind,
    file_name: String,
    mime_type: Option<String>,
    bytes: Vec<u8>,
    description: &str,
) -> Resource {
    let size = bytes.len() as u64;
    Resource {
        tags: resource_tags(kind, &file_name, mime_type.as_deref()),
        name: file_name,
        description: Some(description.to_string()),
        mime_type,
        blob: Some(ByteBufB64(bytes)),
        size: Some(size),
        ..Default::default()
    }
}

pub fn file_name_for_resource(resource: &Resource) -> String {
    if !resource.name.trim().is_empty() {
        resource.name.clone()
    } else {
        default_file_name_for_resource(resource).to_string()
    }
}

pub fn default_file_name_for_resource(resource: &Resource) -> &'static str {
    let mime_type = resource.mime_type.as_deref().unwrap_or_default();
    if resource.tags.iter().any(|tag| tag == "image") || mime_type.starts_with("image/") {
        "image.jpg"
    } else if resource.tags.iter().any(|tag| tag == "video") || mime_type.starts_with("video/") {
        "video.mp4"
    } else if resource.tags.iter().any(|tag| tag == "audio") || mime_type.starts_with("audio/") {
        "audio.mp3"
    } else {
        "document.bin"
    }
}

pub fn is_http_url(target: &str) -> bool {
    target.starts_with("http://") || target.starts_with("https://")
}

fn file_extension(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
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

fn local_file_uri(path: &Path) -> String {
    format!("file://{}", path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_tags_detect_text_and_audio() {
        assert_eq!(
            resource_tags(AttachmentKind::Document, "notes.txt", Some("text/plain")),
            vec!["text".to_string(), "txt".to_string()]
        );
        assert_eq!(
            resource_tags(AttachmentKind::Audio, "voice.ogg", Some("audio/ogg")),
            vec!["audio".to_string(), "ogg".to_string()]
        );
    }

    #[tokio::test]
    async fn channel_workspace_stores_resource_and_sets_uri() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace = ChannelWorkspace::default();
        workspace.set_path(temp_dir.path().join("telegram:test"));
        let mut resource = resource_from_bytes(
            AttachmentKind::Document,
            "../../notes.txt".to_string(),
            Some("text/plain".to_string()),
            b"hello".to_vec(),
            "test attachment",
        );

        let path = workspace
            .store_resource(&mut resource, Some("msg/1"))
            .await
            .unwrap()
            .unwrap();

        assert!(path.exists());
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"hello");
        assert!(path.ends_with("msg_1-notes.txt"));
        assert_eq!(
            resource.uri.as_deref(),
            Some(format!("file://{}", path.to_string_lossy()).as_str())
        );
        assert_eq!(resource.blob.as_ref().unwrap().0, b"hello".to_vec());
    }
}
