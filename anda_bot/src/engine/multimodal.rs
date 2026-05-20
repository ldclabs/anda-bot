use anda_core::{
    Agent, AgentContext, AgentInput, AgentOutput, BoxError, ByteBufB64, CompletionFeatures,
    CompletionRequest, ContentPart, FunctionDefinition, RequestMeta, Resource, StateFeatures,
    Usage,
};
use anda_engine::context::AgentCtx;
use futures_util::{StreamExt, stream};
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};

pub const IMAGE_UNDERSTANDING_AGENT_NAME: &str = "image_understanding";
pub const AUDIO_UNDERSTANDING_AGENT_NAME: &str = "audio_understanding";
pub const VIDEO_UNDERSTANDING_AGENT_NAME: &str = "video_understanding";

pub const IMAGE_MODEL_LABEL: &str = "image";
pub const AUDIO_MODEL_LABEL: &str = "audio";
pub const VIDEO_MODEL_LABEL: &str = "video";

const MAX_MEDIA_FILE_SIZE_BYTES: u64 = 25 * 1024 * 1024;
const MAX_MEDIA_UNDERSTANDING_CONCURRENCY: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaKind {
    Image,
    Audio,
    Video,
}

impl MediaKind {
    fn agent_name(self) -> &'static str {
        match self {
            Self::Image => IMAGE_UNDERSTANDING_AGENT_NAME,
            Self::Audio => AUDIO_UNDERSTANDING_AGENT_NAME,
            Self::Video => VIDEO_UNDERSTANDING_AGENT_NAME,
        }
    }

    fn model_label(self) -> &'static str {
        match self {
            Self::Image => IMAGE_MODEL_LABEL,
            Self::Audio => AUDIO_MODEL_LABEL,
            Self::Video => VIDEO_MODEL_LABEL,
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Image => {
                "Understand image attachments or image file paths using the model labeled `image`, returning a textual description for downstream agents."
            }
            Self::Audio => {
                "Understand audio attachments or audio file paths using the model labeled `audio`, returning transcription and sound notes for downstream agents."
            }
            Self::Video => {
                "Understand video attachments or video file paths using the model labeled `video`, returning a textual summary for downstream agents."
            }
        }
    }

    fn tags(self) -> Vec<String> {
        match self {
            Self::Image => ["image"].into_iter().map(ToString::to_string).collect(),
            Self::Audio => ["audio"].into_iter().map(ToString::to_string).collect(),
            Self::Video => ["video"].into_iter().map(ToString::to_string).collect(),
        }
    }

    fn default_question(self) -> &'static str {
        match self {
            Self::Image => {
                "Describe the image for a text-only agent. Include visible objects, people, layout, notable details, and any readable text."
            }
            Self::Audio => {
                "Understand this audio for a text-only agent. Transcribe speech when possible, identify language, speakers if apparent, sound events, tone, and any uncertainty."
            }
            Self::Video => {
                "Understand this video for a text-only agent. Summarize the key visual events, scenes, actions, on-screen text, and audible speech or sounds when available."
            }
        }
    }

    fn instructions(self) -> String {
        format!(
            "You are a specialized {kind} understanding subagent. Use the provided {kind} content or file path only. Answer the caller's question when one is provided; otherwise produce a concise but complete understanding that a text-only main agent can rely on. Return Markdown plain text. Preserve observable facts, transcribe visible or audible text when possible, and clearly mark uncertainty instead of guessing.",
            kind = self.noun()
        )
    }

    fn from_resource(resource: &Resource) -> Option<Self> {
        resource
            .mime_type
            .as_deref()
            .and_then(Self::from_mime_type)
            .or_else(|| {
                resource
                    .blob
                    .as_ref()
                    .and_then(|blob| infer2::get(&blob.0))
                    .and_then(|kind| Self::from_mime_type(kind.mime_type()))
            })
            .or_else(|| {
                infer2::get_from_filename(&resource.name)
                    .and_then(|kind| Self::from_mime_type(kind.mime_type()))
            })
            .or_else(|| Self::from_tags(&resource.tags))
            .or_else(|| extension_from_name(&resource.name).and_then(Self::from_extension))
    }

    fn from_mime_type(mime_type: &str) -> Option<Self> {
        let mime_type = mime_type.to_ascii_lowercase();
        if mime_type.starts_with("image/") {
            Some(Self::Image)
        } else if mime_type.starts_with("audio/") {
            Some(Self::Audio)
        } else if mime_type.starts_with("video/") {
            Some(Self::Video)
        } else {
            None
        }
    }

    fn from_tags(tags: &[String]) -> Option<Self> {
        for tag in tags {
            let tag = tag.to_ascii_lowercase();
            if tag == "image" {
                return Some(Self::Image);
            }
            if tag == "audio" {
                return Some(Self::Audio);
            }
            if tag == "video" {
                return Some(Self::Video);
            }
        }

        tags.iter()
            .find_map(|tag| Self::from_extension(tag.trim_start_matches('.')))
    }

    fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "avif" | "bmp" | "gif" | "heic" | "heif" | "jpeg" | "jpg" | "png" | "svg" | "tif"
            | "tiff" | "webp" => Some(Self::Image),
            "aac" | "aiff" | "amr" | "flac" | "m4a" | "mp3" | "mpga" | "oga" | "ogg" | "opus"
            | "pcm" | "wav" => Some(Self::Audio),
            "3gp" | "avi" | "flv" | "m4v" | "mkv" | "mov" | "mp4" | "mpg" | "ogv" | "webm"
            | "wmv" => Some(Self::Video),
            "mpeg" => Some(Self::Audio),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct MediaUnderstandingArgs {
    #[serde(default, alias = "file_path")]
    path: Option<String>,
    #[serde(
        default,
        alias = "query",
        alias = "task",
        alias = "instruction",
        alias = "instructions",
        alias = "prompt"
    )]
    question: Option<String>,
}

impl MediaUnderstandingArgs {
    fn from_prompt(prompt: &str) -> Self {
        let trimmed = prompt.trim();
        if trimmed.is_empty() {
            return Self::default();
        }

        serde_json::from_str::<Self>(trimmed).unwrap_or_else(|_| Self {
            path: None,
            question: Some(trimmed.to_string()),
        })
    }

    fn question_or_default(&self, kind: MediaKind) -> String {
        self.question
            .as_deref()
            .map(str::trim)
            .filter(|question| !question.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| kind.default_question().to_string())
    }
}

#[derive(Clone)]
pub struct MediaUnderstandingAgent {
    kind: MediaKind,
    workspaces: Vec<PathBuf>,
}

impl MediaUnderstandingAgent {
    pub fn image(workspaces: Vec<PathBuf>) -> Self {
        Self {
            kind: MediaKind::Image,
            workspaces,
        }
    }

    pub fn audio(workspaces: Vec<PathBuf>) -> Self {
        Self {
            kind: MediaKind::Audio,
            workspaces,
        }
    }

    pub fn video(workspaces: Vec<PathBuf>) -> Self {
        Self {
            kind: MediaKind::Video,
            workspaces,
        }
    }

    pub fn model_label(&self) -> &'static str {
        self.kind.model_label()
    }

    async fn content_from_path(
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
        if MediaKind::from_mime_type(&mime_type).is_none()
            && extension_from_name(&resolved.to_string_lossy()).and_then(MediaKind::from_extension)
                != Some(self.kind)
        {
            return Err(format!(
                "file does not look like {} media: {} ({mime_type})",
                self.kind.noun(),
                resolved.display()
            )
            .into());
        }

        Ok(ContentPart::InlineData {
            mime_type,
            data: ByteBufB64(data),
        })
    }

    fn content_from_resource(&self, resource: Resource) -> Result<ContentPart, BoxError> {
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

        if let Some(file_uri) = uri.filter(|uri| !uri.trim().is_empty()) {
            return Ok(ContentPart::FileData {
                file_uri,
                mime_type,
            });
        }

        Err(format!("media resource {} has no inline data or URI", name).into())
    }

    fn completion_prompt(&self, args: &MediaUnderstandingArgs, resources_len: usize) -> String {
        let target = if resources_len == 0 {
            "the media file at the supplied path".to_string()
        } else if resources_len == 1 {
            format!("the attached {} resource", self.kind.noun())
        } else {
            format!(
                "the {resources_len} attached {} resources",
                self.kind.noun()
            )
        };

        format!(
            "Understand {target}. Caller question or focus:\n{}",
            args.question_or_default(self.kind)
        )
    }
}

impl Agent<AgentCtx> for MediaUnderstandingAgent {
    fn name(&self) -> String {
        self.kind.agent_name().to_string()
    }

    fn description(&self) -> String {
        self.kind.description().to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: json!({
                "type": "object",
                "description": "Understand one media attachment selected by resource tags, or read a local media file path from the configured workspace. Do not include a `prompt` property; use `question` for optional guidance so the file path is preserved.",
                "properties": {
                    "path": {
                        "type": ["string", "null"],
                        "description": "Optional local media file path. Relative paths resolve from the current configured workspace; absolute paths must be inside an allowed workspace. Omit when passing an attached resource."
                    },
                    "question": {
                        "type": ["string", "null"],
                        "description": "Optional question or focus for the media understanding task."
                    }
                },
                "additionalProperties": false
            }),
            strict: Some(true),
        }
    }

    fn supported_resource_tags(&self) -> Vec<String> {
        self.kind.tags()
    }

    async fn run(
        &self,
        ctx: AgentCtx,
        prompt: String,
        resources: Vec<Resource>,
    ) -> Result<AgentOutput, BoxError> {
        let args = MediaUnderstandingArgs::from_prompt(&prompt);
        let resources_len = resources.len();
        let mut content = Vec::with_capacity(resources.len() + 1);

        for resource in resources {
            content.push(self.content_from_resource(resource)?);
        }

        if let Some(path) = args
            .path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            content.push(self.content_from_path(ctx.meta(), path).await?);
        }

        if content.is_empty() {
            return Err(format!(
                "{} requires an attached {} resource or a workspace file path",
                self.kind.agent_name(),
                self.kind.noun()
            )
            .into());
        }

        ctx.completion(
            CompletionRequest {
                instructions: self.kind.instructions(),
                prompt: self.completion_prompt(&args, resources_len),
                content,
                max_output_tokens: Some(ctx.model.max_output.clamp(1024, 8192)),
                ..Default::default()
            },
            Vec::new(),
        )
        .await
    }
}

pub fn media_agent_names() -> Vec<String> {
    [
        IMAGE_UNDERSTANDING_AGENT_NAME,
        AUDIO_UNDERSTANDING_AGENT_NAME,
        VIDEO_UNDERSTANDING_AGENT_NAME,
    ]
    .into_iter()
    .map(ToString::to_string)
    .collect()
}

pub fn supported_media_resource_tags() -> Vec<String> {
    let mut tags = Vec::new();
    for kind in [MediaKind::Image, MediaKind::Audio, MediaKind::Video] {
        for tag in kind.tags() {
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
    }
    tags
}

pub async fn understand_media_resources(
    ctx: &AgentCtx,
    resources: Vec<Resource>,
) -> (Vec<Resource>, Usage) {
    let results = stream::iter(resources.into_iter().map(|resource| {
        let ctx = ctx.clone();

        async move {
            let Some(kind) = MediaKind::from_resource(&resource) else {
                return (resource, Usage::default());
            };

            let label = resource_label(&resource);
            let prompt = json!({ "question": kind.default_question() }).to_string();
            let input = AgentInput {
                name: kind.agent_name().to_string(),
                prompt,
                resources: vec![resource],
                ..Default::default()
            };

            let (text, usage) = match ctx.agent_run(input).await {
                Ok((agent_output, _)) => {
                    let mut usage = Usage::default();
                    usage.accumulate(&agent_output.usage);
                    let content = agent_output.content.trim();
                    let text = if content.is_empty() {
                        format!(
                            "[$system: kind={:?}]\n{} understanding {:?} from attachments\n\nNo description was returned.",
                            kind.agent_name(),
                            title_case(kind.noun()),
                            label
                        )
                    } else {
                        format!(
                            "[$system: kind={:?}]\n{} understanding {:?} from attachments\n\n{:?}",
                            kind.agent_name(),
                            title_case(kind.noun()),
                            label,
                            content
                        )
                    };

                    (text, usage)
                }
                Err(err) => (
                    format!(
                        "[$system: kind={:?}]\n{} understanding {:?} from attachments\n\nFailed to understand this {} from attachments: {}",
                        kind.agent_name(),
                        title_case(kind.noun()),
                        label,
                        kind.noun(),
                        err
                    ),
                    Usage::default(),
                ),
            };

            (
                text_resource(
                    format!("{}_understanding.md", kind.noun()),
                    format!("Text understanding generated from {label}"),
                    text,
                ),
                usage,
            )
        }
    }))
    .buffered(MAX_MEDIA_UNDERSTANDING_CONCURRENCY)
    .collect::<Vec<_>>()
    .await;

    let mut output = Vec::with_capacity(results.len());
    let mut usage = Usage::default();
    for (resource, resource_usage) in results {
        usage.accumulate(&resource_usage);
        output.push(resource);
    }

    (output, usage)
}

fn text_resource(name: String, description: String, text: String) -> Resource {
    let bytes = text.into_bytes();
    Resource {
        tags: vec![
            "text".to_string(),
            "md".to_string(),
            "media_understanding".to_string(),
        ],
        name,
        description: Some(description),
        mime_type: Some("text/markdown".to_string()),
        size: Some(bytes.len() as u64),
        blob: Some(ByteBufB64(bytes)),
        ..Default::default()
    }
}

fn resource_label(resource: &Resource) -> String {
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

async fn resolve_media_path(
    meta: &RequestMeta,
    defaults: &[PathBuf],
    user_path: &str,
) -> Result<PathBuf, BoxError> {
    let requested = PathBuf::from(strip_file_uri(user_path.trim()));
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

fn workspaces_from_meta(meta: &RequestMeta, defaults: &[PathBuf]) -> Vec<PathBuf> {
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

fn strip_file_uri(path: &str) -> &str {
    path.strip_prefix("file://").unwrap_or(path)
}

fn mime_type_for_data_or_path(data: &[u8], path: &Path, fallback: &str) -> String {
    infer2::get(data)
        .map(|kind| kind.mime_type().to_string())
        .or_else(|| mime_type_from_name(&path.to_string_lossy()))
        .unwrap_or_else(|| fallback.to_string())
}

fn mime_type_from_name(name: &str) -> Option<String> {
    infer2::get_from_filename(name).map(|kind| kind.mime_type().to_string())
}

fn extension_from_name(name: &str) -> Option<&str> {
    Path::new(name).extension().and_then(|ext| ext.to_str())
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];

    #[test]
    fn parses_json_args_with_path_and_question() {
        let args = MediaUnderstandingArgs::from_prompt(
            r#"{"path":"images/cat.png","question":"What is unusual?"}"#,
        );

        assert_eq!(args.path.as_deref(), Some("images/cat.png"));
        assert_eq!(args.question.as_deref(), Some("What is unusual?"));
    }

    #[test]
    fn plain_prompt_becomes_question() {
        let args = MediaUnderstandingArgs::from_prompt("describe the scene");

        assert_eq!(args.path, None);
        assert_eq!(args.question.as_deref(), Some("describe the scene"));
    }

    #[test]
    fn detects_media_kind_from_mime_before_extension() {
        let resource = Resource {
            name: "clip.mp4".to_string(),
            mime_type: Some("audio/mp4".to_string()),
            ..Default::default()
        };

        assert_eq!(MediaKind::from_resource(&resource), Some(MediaKind::Audio));
    }

    #[test]
    fn blank_alias_question_uses_default_question() {
        let args =
            MediaUnderstandingArgs::from_prompt(r#"{"path":"audio/sample.mp3","query":"   "}"#);

        assert_eq!(args.path.as_deref(), Some("audio/sample.mp3"));
        assert_eq!(
            args.question_or_default(MediaKind::Audio),
            MediaKind::Audio.default_question()
        );
    }

    #[test]
    fn detects_media_kind_from_tag_extension() {
        let resource = Resource {
            name: "payload.bin".to_string(),
            tags: vec![".webm".to_string()],
            ..Default::default()
        };

        assert_eq!(MediaKind::from_resource(&resource), Some(MediaKind::Video));
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
        let agent = MediaUnderstandingAgent::image(Vec::new());
        let resource = Resource {
            name: "photo.bin".to_string(),
            blob: Some(ByteBufB64(PNG_SIGNATURE.to_vec())),
            ..Default::default()
        };

        let content = agent
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
        let agent = MediaUnderstandingAgent::video(Vec::new());
        let resource = Resource {
            name: "clip.mp4".to_string(),
            uri: Some("file:///tmp/clip.mp4".to_string()),
            mime_type: Some("video/mp4".to_string()),
            ..Default::default()
        };

        let content = agent
            .content_from_resource(resource)
            .expect("video uri should be accepted");

        match content {
            ContentPart::FileData {
                file_uri,
                mime_type,
            } => {
                assert_eq!(file_uri, "file:///tmp/clip.mp4");
                assert_eq!(mime_type.as_deref(), Some("video/mp4"));
            }
            other => panic!("expected file data, got {other:?}"),
        }
    }

    #[test]
    fn content_from_resource_rejects_mismatched_media_kind() {
        let agent = MediaUnderstandingAgent::image(Vec::new());
        let resource = Resource {
            name: "speech.mp3".to_string(),
            mime_type: Some("audio/mpeg".to_string()),
            ..Default::default()
        };

        let err = agent
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

        let agent = MediaUnderstandingAgent::image(vec![dir.path().to_path_buf()]);
        let content = agent
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
    async fn resolve_media_path_accepts_file_uri_within_workspace() {
        let dir = tempdir().expect("tempdir should be created");
        let file = dir.path().join("cat.png");
        fs::write(&file, PNG_SIGNATURE).expect("image file should be written");
        let file = file.canonicalize().expect("file should canonicalize");

        let resolved = resolve_media_path(
            &RequestMeta::default(),
            &[dir.path().to_path_buf()],
            &format!("file://{}", file.display()),
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
}
