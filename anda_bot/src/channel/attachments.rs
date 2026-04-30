use anda_core::{BoxError, ByteBufB64, Resource};
use anda_db::unix_ms;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::RwLock,
};

pub type MimeKind = infer2::MatcherType;
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

pub fn infer_from(file_name: &str, mime_type: Option<&str>) -> Option<InferType> {
    mime_type
        .and_then(|mime| infer2::get_from_mime(mime))
        .or_else(|| infer2::get_from_filename(file_name))
}

pub fn infer_from_resource(resource: &Resource) -> Option<InferType> {
    resource
        .blob
        .as_ref()
        .and_then(|blob| infer2::get(&blob.0))
        .or_else(|| infer_from(&resource.name, resource.mime_type.as_deref()))
}

pub fn infer_tag(it: &InferType) -> String {
    match it.matcher_type() {
        MimeKind::App => "app".to_string(),
        MimeKind::Archive => "archive".to_string(),
        MimeKind::Audio => "audio".to_string(),
        MimeKind::Book => "book".to_string(),
        MimeKind::Doc => "doc".to_string(),
        MimeKind::Font => "font".to_string(),
        MimeKind::Image => "image".to_string(),
        MimeKind::Text => "text".to_string(),
        MimeKind::Video => "video".to_string(),
        MimeKind::Custom => "custom".to_string(),
    }
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
        let tag = infer_tag(&t);
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
    } else if let Some(t) = infer_from_resource(&resource) {
        Cow::Owned(format!("{}.{}", infer_tag(&t), t.extension()))
    } else {
        Cow::Borrowed("document.bin")
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
}
