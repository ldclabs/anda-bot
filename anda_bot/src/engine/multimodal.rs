use anda_core::{
    Agent, AgentContext, AgentInput, AgentOutput, BoxError, ByteBufB64, CompletionFeatures,
    CompletionRequest, ContentPart, FunctionDefinition, RequestMeta, Resource, StateFeatures,
    Usage, inline_data_from_data_url,
};
use anda_engine::{
    context::{AgentCtx, TOOLS_SEARCH_NAME, TOOLS_SELECT_NAME},
    extension::{
        fs::{ReadFileTool, SearchFileTool},
        shell::ShellTool,
        skill::SkillManager,
    },
    subagent::SubAgentManager,
};
use futures_util::{StreamExt, stream};
use liteparse::{LiteParse, LiteParseConfig, types::PdfInput};
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};

pub const IMAGE_UNDERSTANDING_AGENT_NAME: &str = "image_understanding";
pub const AUDIO_UNDERSTANDING_AGENT_NAME: &str = "audio_understanding";
pub const VIDEO_UNDERSTANDING_AGENT_NAME: &str = "video_understanding";
pub const OTHER_UNDERSTANDING_AGENT_NAME: &str = "attachment_understanding";

pub const IMAGE_MODEL_LABEL: &str = "image";
pub const AUDIO_MODEL_LABEL: &str = "audio";
pub const VIDEO_MODEL_LABEL: &str = "video";
pub const OTHER_MODEL_LABEL: &str = "flash";

const MAX_MEDIA_FILE_SIZE_BYTES: u64 = 25 * 1024 * 1024;
const MAX_MEDIA_UNDERSTANDING_CONCURRENCY: usize = 8;
const MAX_OTHER_TEXT_INLINE_BYTES: usize = 64 * 1024;
const MAX_OTHER_TEXT_SUMMARY_BYTES: usize = 512 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaKind {
    Image,
    Audio,
    Video,
    Other,
}

impl MediaKind {
    fn agent_name(self) -> &'static str {
        match self {
            Self::Image => IMAGE_UNDERSTANDING_AGENT_NAME,
            Self::Audio => AUDIO_UNDERSTANDING_AGENT_NAME,
            Self::Video => VIDEO_UNDERSTANDING_AGENT_NAME,
            Self::Other => OTHER_UNDERSTANDING_AGENT_NAME,
        }
    }

    fn model_label(self) -> &'static str {
        match self {
            Self::Image => IMAGE_MODEL_LABEL,
            Self::Audio => AUDIO_MODEL_LABEL,
            Self::Video => VIDEO_MODEL_LABEL,
            Self::Other => OTHER_MODEL_LABEL,
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Other => "attachment",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Image => {
                "Understand image attachments, image file paths, or image URLs using the model labeled `image`, returning a textual description for downstream agents."
            }
            Self::Audio => {
                "Understand audio attachments, audio file paths, or audio URLs using the model labeled `audio`, returning transcription and sound notes for downstream agents."
            }
            Self::Video => {
                "Understand video attachments, video file paths, or video URLs using the model labeled `video`, returning a textual summary for downstream agents."
            }
            Self::Other => {
                "Understand non-image/audio/video attachments. UTF-8 blobs are handled as text, PDFs are parsed locally with LiteParse, and other formats are delegated to available skills or shell-assisted extraction."
            }
        }
    }

    fn tags(self) -> Vec<String> {
        match self {
            Self::Image => ["image"].into_iter().map(ToString::to_string).collect(),
            Self::Audio => ["audio"].into_iter().map(ToString::to_string).collect(),
            Self::Video => ["video"].into_iter().map(ToString::to_string).collect(),
            Self::Other => [
                "text", "txt", "md", "markdown", "pdf", "json", "csv", "tsv", "doc", "docx", "xls",
                "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf", "html", "xml", "yaml", "toml",
                "log", "document", "file", "other",
            ]
            .into_iter()
            .map(ToString::to_string)
            .collect(),
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
            Self::Other => {
                "Understand this attachment for a text-only agent. Extract or summarize useful text, structure, metadata, and uncertainty. Do not invent unavailable content."
            }
        }
    }

    fn instructions(self) -> String {
        if self == Self::Other {
            return "You are a specialized attachment understanding subagent. Prefer local, faithful extraction over guessing. For UTF-8 text, preserve the actual content when it is small and summarize it when it is large. For PDFs, use LiteParse extraction results. For other formats, look for a suitable installed skill first, then use safe shell/read-only inspection, and only research a method over the network when local options are insufficient. Return Markdown plain text for the main agent and clearly mark failures or uncertainty.".to_string();
        }

        format!(
            "You are a specialized {kind} understanding subagent. Use the provided {kind} content, file path, or URL only. Answer the caller's question when one is provided; otherwise produce a concise but complete understanding that a text-only main agent can rely on. Return Markdown plain text. Preserve observable facts, transcribe visible or audible text when possible, and clearly mark uncertainty instead of guessing.",
            kind = self.noun()
        )
    }

    fn from_resource(resource: &Resource) -> Option<Self> {
        let media_kind = resource
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
            .or_else(|| extension_from_name(&resource.name).and_then(Self::from_extension));

        media_kind.or_else(|| is_other_resource_candidate(resource).then_some(Self::Other))
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
            "pdf" => Some(Self::Other),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct MediaUnderstandingArgs {
    #[serde(default, alias = "file_path")]
    path: Option<String>,
    #[serde(default, alias = "uri", alias = "file_uri")]
    url: Option<String>,
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
            url: None,
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
    http: reqwest::Client,
}

impl MediaUnderstandingAgent {
    pub fn image(workspaces: Vec<PathBuf>) -> Self {
        Self {
            kind: MediaKind::Image,
            workspaces,
            http: reqwest::Client::new(),
        }
    }

    pub fn audio(workspaces: Vec<PathBuf>) -> Self {
        Self {
            kind: MediaKind::Audio,
            workspaces,
            http: reqwest::Client::new(),
        }
    }

    pub fn video(workspaces: Vec<PathBuf>) -> Self {
        Self {
            kind: MediaKind::Video,
            workspaces,
            http: reqwest::Client::new(),
        }
    }

    pub fn other(workspaces: Vec<PathBuf>) -> Self {
        Self {
            kind: MediaKind::Other,
            workspaces,
            http: reqwest::Client::new(),
        }
    }

    pub fn with_http_client(mut self, http: reqwest::Client) -> Self {
        self.http = http;
        self
    }

    pub fn model_label(&self) -> &'static str {
        self.kind.model_label()
    }

    async fn run_other(
        &self,
        ctx: AgentCtx,
        prompt: String,
        resources: Vec<Resource>,
    ) -> Result<AgentOutput, BoxError> {
        let args = MediaUnderstandingArgs::from_prompt(&prompt);
        let question = args.question_or_default(self.kind);
        let mut attachments = Vec::with_capacity(resources.len() + 2);

        for resource in resources {
            attachments.push(OtherAttachment::from_resource(resource));
        }

        if let Some(url) = args
            .url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
        {
            attachments.push(self.other_attachment_from_location(ctx.meta(), url).await?);
        }

        if let Some(path) = args
            .path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            attachments.push(
                self.other_attachment_from_location(ctx.meta(), path)
                    .await?,
            );
        }

        if attachments.is_empty() {
            return Err(
                "other_understanding requires an attached resource, workspace file path, or URL"
                    .into(),
            );
        }

        let mut output = AgentOutput::default();
        let mut sections = Vec::with_capacity(attachments.len());
        for attachment in attachments {
            let label = attachment.label.clone();
            match self
                .understand_other_attachment(&ctx, attachment, &question)
                .await
            {
                Ok(section) => {
                    output.usage.accumulate(&section.usage);
                    let content = section.content.trim();
                    if content.is_empty() {
                        sections.push(format!("No description was returned for {label}."));
                    } else {
                        sections.push(content.to_string());
                    }
                }
                Err(err) => {
                    sections.push(format!("Failed to understand {label}, error: {err}"));
                }
            }
        }

        output.content = sections.join("\n\n---\n\n");
        Ok(output)
    }

    async fn other_attachment_from_location(
        &self,
        meta: &RequestMeta,
        location: &str,
    ) -> Result<OtherAttachment, BoxError> {
        let location = location.trim();
        if location.is_empty() {
            return Err("attachment location cannot be empty".into());
        }

        if strip_data_url_scheme(location).is_some() {
            let (data, mime_type) = inline_data_from_data_url(location)
                .ok_or_else(|| "invalid attachment data URL".to_string())?;
            if data.0.len() as u64 > MAX_MEDIA_FILE_SIZE_BYTES {
                return Err(format!(
                    "attachment data URL is too large: {} bytes, max {MAX_MEDIA_FILE_SIZE_BYTES}",
                    data.0.len()
                )
                .into());
            }

            return Ok(OtherAttachment {
                label: "data URL".to_string(),
                name: "data-url".to_string(),
                mime_type: Some(mime_type),
                uri: Some(location.to_string()),
                size: Some(data.0.len() as u64),
                data: Some(data.0),
                tags: Vec::new(),
                read_error: None,
            });
        }

        if let Ok(url) = reqwest::Url::parse(location) {
            return match url.scheme() {
                "http" | "https" => self.other_attachment_from_http_url(url).await,
                "file" => self.other_attachment_from_path(meta, location).await,
                scheme if location.contains("://") => {
                    Err(format!("unsupported attachment URL scheme: {scheme}").into())
                }
                _ => self.other_attachment_from_path(meta, location).await,
            };
        }

        self.other_attachment_from_path(meta, location).await
    }

    async fn other_attachment_from_path(
        &self,
        meta: &RequestMeta,
        path: &str,
    ) -> Result<OtherAttachment, BoxError> {
        let resolved = resolve_media_path(meta, &self.workspaces, path).await?;
        let metadata = tokio::fs::metadata(&resolved).await?;
        if !metadata.is_file() {
            return Err(format!(
                "attachment path is not a regular file: {}",
                resolved.display()
            )
            .into());
        }
        if metadata.len() > MAX_MEDIA_FILE_SIZE_BYTES {
            return Err(format!(
                "attachment file is too large: {} bytes, max {MAX_MEDIA_FILE_SIZE_BYTES}",
                metadata.len()
            )
            .into());
        }

        let data = tokio::fs::read(&resolved).await?;
        let name = resolved.to_string_lossy().to_string();
        let mime_type = mime_type_for_data_or_path(&data, &resolved, "application/octet-stream");

        Ok(OtherAttachment {
            label: name.clone(),
            name,
            mime_type: Some(mime_type),
            uri: Some(format!("file://{}", resolved.display())),
            size: Some(metadata.len()),
            data: Some(data),
            tags: Vec::new(),
            read_error: None,
        })
    }

    async fn other_attachment_from_http_url(
        &self,
        url: reqwest::Url,
    ) -> Result<OtherAttachment, BoxError> {
        let response = self.http.get(url.clone()).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("failed to fetch attachment URL {url}: {status}").into());
        }

        if let Some(content_length) = response.content_length()
            && content_length > MAX_MEDIA_FILE_SIZE_BYTES
        {
            return Err(format!(
                "attachment URL is too large: {content_length} bytes, max {MAX_MEDIA_FILE_SIZE_BYTES}"
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
        let name = url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("attachment")
            .to_string();

        Ok(OtherAttachment {
            label: url.to_string(),
            name,
            mime_type: Some(mime_type),
            uri: Some(url.to_string()),
            size: Some(data.len() as u64),
            data: Some(data),
            tags: Vec::new(),
            read_error: None,
        })
    }

    async fn understand_other_attachment(
        &self,
        ctx: &AgentCtx,
        mut attachment: OtherAttachment,
        question: &str,
    ) -> Result<AgentOutput, BoxError> {
        if attachment.data.is_none()
            && let Some(uri) = attachment
                .uri
                .as_deref()
                .filter(|uri| !uri.trim().is_empty())
        {
            match self.other_attachment_from_location(ctx.meta(), uri).await {
                Ok(mut loaded) => {
                    if loaded.name.trim().is_empty() || loaded.name == "attachment" {
                        loaded.name = attachment.name.clone();
                    }
                    if loaded.label.trim().is_empty() {
                        loaded.label = attachment.label.clone();
                    }
                    if loaded.tags.is_empty() {
                        loaded.tags = attachment.tags.clone();
                    }
                    attachment = loaded;
                }
                Err(err) => {
                    attachment.read_error = Some(err.to_string());
                    return self
                        .fallback_other_attachment(ctx, attachment, question)
                        .await;
                }
            }
        }

        let Some(data) = attachment.data.as_deref() else {
            return self
                .fallback_other_attachment(ctx, attachment, question)
                .await;
        };

        if attachment_looks_like_pdf(data, &attachment) {
            return self
                .understand_pdf_attachment(ctx, attachment, question)
                .await;
        }

        if let Some(text) = utf8_text_from_bytes(data) {
            return self
                .text_or_summary_output(
                    ctx,
                    &attachment.label,
                    &attachment.name,
                    "UTF-8 text attachment",
                    text,
                    question,
                )
                .await;
        }

        self.fallback_other_attachment(ctx, attachment, question)
            .await
    }

    async fn understand_pdf_attachment(
        &self,
        ctx: &AgentCtx,
        mut attachment: OtherAttachment,
        question: &str,
    ) -> Result<AgentOutput, BoxError> {
        let Some(data) = attachment.data.clone() else {
            return self
                .fallback_other_attachment(ctx, attachment, question)
                .await;
        };

        match parse_pdf_text(&data).await {
            Ok(text) if !text.trim().is_empty() => {
                self.text_or_summary_output(
                    ctx,
                    &attachment.label,
                    &attachment.name,
                    "PDF text parsed by LiteParse",
                    &text,
                    question,
                )
                .await
            }
            Ok(_) => Ok(AgentOutput {
                content: format!(
                    "LiteParse recognized {} as a PDF but did not extract text. The file may be scanned, image-only, encrypted, or otherwise sparse.",
                    attachment.label
                ),
                ..Default::default()
            }),
            Err(err) => {
                attachment.read_error = Some(format!("LiteParse failed: {err}"));
                self.fallback_other_attachment(ctx, attachment, question)
                    .await
            }
        }
    }

    async fn text_or_summary_output(
        &self,
        ctx: &AgentCtx,
        label: &str,
        name: &str,
        source: &str,
        text: &str,
        question: &str,
    ) -> Result<AgentOutput, BoxError> {
        if text.len() <= MAX_OTHER_TEXT_INLINE_BYTES {
            return Ok(AgentOutput {
                content: format!(
                    "Detected {source} from {label} ({} bytes). Full text:\n\n{}",
                    text.len(),
                    fenced_text(text_language_for_name(name), text)
                ),
                ..Default::default()
            });
        }

        let (summary_input, truncated) = bounded_text_for_summary(text);
        let mut output = ctx
            .completion(
                CompletionRequest {
                    instructions: "Summarize extracted attachment text faithfully for a downstream text-only agent. Preserve important names, numbers, dates, sections, decisions, and uncertainty. Do not invent content that is not present in the supplied text.".to_string(),
                    prompt: format!(
                        "Summarize {source} from {label}. Original text length: {} bytes.{}\n\nCaller question or focus:\n{question}",
                        text.len(),
                        if truncated {
                            " The supplied text is a bounded head/tail excerpt because the attachment is very large; say when conclusions may be incomplete."
                        } else {
                            ""
                        }
                    ),
                    content: vec![ContentPart::Text {
                        text: summary_input,
                    }],
                    max_output_tokens: Some(2048),
                    ..Default::default()
                },
                Vec::new(),
            )
            .await?;

        let summary = output.content.trim();
        output.content = format!(
            "Detected {source} from {label} ({} bytes). The text is too large to inline, so this is a summary{}:\n\n{}",
            text.len(),
            if truncated {
                " based on a bounded excerpt"
            } else {
                ""
            },
            if summary.is_empty() {
                "No summary was returned."
            } else {
                summary
            }
        );
        Ok(output)
    }

    async fn fallback_other_attachment(
        &self,
        ctx: &AgentCtx,
        attachment: OtherAttachment,
        question: &str,
    ) -> Result<AgentOutput, BoxError> {
        let metadata = attachment.metadata_markdown();
        let tools = ctx
            .definitions(Some(&other_understanding_tool_names()))
            .await;
        let mut output = ctx
            .completion(
                CompletionRequest {
                    instructions: self.kind.instructions(),
                    prompt: format!(
                        "Understand this non-image/audio/video attachment for the main agent.\n\nWorkflow:\n1. Search available tools/skills for a parser that matches the MIME type, extension, or file family; use an installed skill/subagent if one is suitable.\n2. If no skill fits, use safe shell or read-only file inspection to extract text or metadata when the attachment has an accessible local path or URL.\n3. If local tools are insufficient, use network-capable tools or shell commands to research a practical extraction method, then report the best next action.\n\nDo not invent attachment contents. If extraction is impossible, explain what was tried or what capability is missing.\n\nCaller question or focus:\n{question}\n\nAttachment metadata:\n{metadata}"
                    ),
                    tools,
                    max_output_tokens: Some(2048),
                    ..Default::default()
                },
                vec![attachment.to_resource()],
            )
            .await?;

        if output.content.trim().is_empty() {
            output.content = format!(
                "No automatic parser produced output for this attachment.\n\nAttachment metadata:\n{metadata}"
            );
        }

        Ok(output)
    }

    async fn content_from_location(
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
        let source = resolved.to_string_lossy();
        ensure_media_kind(self.kind, &mime_type, source.as_ref())?;

        Ok(ContentPart::InlineData {
            mime_type,
            data: ByteBufB64(data),
        })
    }

    async fn content_from_http_url(&self, url: reqwest::Url) -> Result<ContentPart, BoxError> {
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

    fn content_from_data_url(&self, data_url: &str) -> Result<ContentPart, BoxError> {
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

        if let Some(file_uri) = uri.filter(|uri| !uri.trim().is_empty())
            && (file_uri.starts_with("https://") || file_uri.starts_with("data:"))
        {
            return Ok(ContentPart::FileData {
                file_uri,
                mime_type,
            });
        }

        Err(format!("media resource {} has no inline data or URI", name).into())
    }

    fn completion_prompt(
        &self,
        args: &MediaUnderstandingArgs,
        resources_len: usize,
        locations_len: usize,
    ) -> String {
        let target = match (resources_len, locations_len) {
            (0, 0) => "the supplied media".to_string(),
            (0, 1) => "the media file at the supplied path or URL".to_string(),
            (0, locations_len) => {
                format!("the {locations_len} media files at the supplied paths or URLs")
            }
            (1, 0) => format!("the attached {} resource", self.kind.noun()),
            (resources_len, 0) => {
                format!(
                    "the {resources_len} attached {} resources",
                    self.kind.noun()
                )
            }
            (1, 1) => format!(
                "the attached {} resource and the media file at the supplied path or URL",
                self.kind.noun()
            ),
            (resources_len, locations_len) => format!(
                "the {resources_len} attached {} resources and the {locations_len} media files at the supplied paths or URLs",
                self.kind.noun()
            ),
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
                "description": "Understand one media attachment selected by resource tags, read a local media file path from the configured workspace, or fetch media from an http/https/data URL. Do not include a `prompt` property; use `question` for optional guidance so the media location is preserved.",
                "properties": {
                    "path": {
                        "type": ["string", "null"],
                        "description": "Optional local media file path. Relative paths resolve from the current configured workspace; absolute paths must be inside an allowed workspace. This also accepts file/http/https/data URLs for compatibility. Omit when passing an attached resource."
                    },
                    "url": {
                        "type": ["string", "null"],
                        "description": "Optional media URL. Supports http, https, and data URLs. Omit when using a local path or attached resource."
                    },
                    "question": {
                        "type": ["string", "null"],
                        "description": "Optional question or focus for the media understanding task."
                    }
                },
                "required": ["path", "url", "question"],
                "additionalProperties": false
            }),
            strict: Some(true),
        }
    }

    fn supported_resource_tags(&self) -> Vec<String> {
        self.kind.tags()
    }

    fn tool_dependencies(&self) -> Vec<String> {
        if self.kind == MediaKind::Other {
            other_understanding_tool_names()
        } else {
            Vec::new()
        }
    }

    async fn run(
        &self,
        ctx: AgentCtx,
        prompt: String,
        resources: Vec<Resource>,
    ) -> Result<AgentOutput, BoxError> {
        if self.kind == MediaKind::Other {
            return self.run_other(ctx, prompt, resources).await;
        }

        let args = MediaUnderstandingArgs::from_prompt(&prompt);
        let resources_len = resources.len();
        let mut locations_len = 0;
        let mut content = Vec::with_capacity(resources.len() + 2);

        for resource in resources {
            content.push(self.content_from_resource(resource)?);
        }

        if let Some(url) = args
            .url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
        {
            content.push(self.content_from_location(ctx.meta(), url).await?);
            locations_len += 1;
        }

        if let Some(path) = args
            .path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            content.push(self.content_from_location(ctx.meta(), path).await?);
            locations_len += 1;
        }

        if content.is_empty() {
            return Err(format!(
                "{} requires an attached {} resource, workspace file path, or media URL",
                self.kind.agent_name(),
                self.kind.noun()
            )
            .into());
        }

        ctx.completion(
            CompletionRequest {
                instructions: self.kind.instructions(),
                prompt: self.completion_prompt(&args, resources_len, locations_len),
                content,
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
        OTHER_UNDERSTANDING_AGENT_NAME,
    ]
    .into_iter()
    .map(ToString::to_string)
    .collect()
}

pub fn supported_media_resource_tags() -> Vec<String> {
    let mut tags = Vec::new();
    for kind in [
        MediaKind::Image,
        MediaKind::Audio,
        MediaKind::Video,
        MediaKind::Other,
    ] {
        for tag in kind.tags() {
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
    }
    tags
}

fn other_understanding_tool_names() -> Vec<String> {
    vec![
        TOOLS_SEARCH_NAME.to_string(),
        TOOLS_SELECT_NAME.to_string(),
        SkillManager::NAME.to_string(),
        SubAgentManager::NAME.to_string(),
        ShellTool::NAME.to_string(),
        ReadFileTool::NAME.to_string(),
        SearchFileTool::NAME.to_string(),
    ]
}

#[derive(Clone, Debug)]
struct OtherAttachment {
    label: String,
    name: String,
    mime_type: Option<String>,
    uri: Option<String>,
    size: Option<u64>,
    data: Option<Vec<u8>>,
    tags: Vec<String>,
    read_error: Option<String>,
}

impl OtherAttachment {
    fn from_resource(resource: Resource) -> Self {
        let label = resource_label(&resource);
        let data = resource.blob.map(|blob| blob.0);
        let size = resource
            .size
            .or_else(|| data.as_ref().map(|data| data.len() as u64));

        Self {
            label,
            name: resource.name,
            mime_type: resource.mime_type,
            uri: resource.uri,
            size,
            data,
            tags: resource.tags,
            read_error: None,
        }
    }

    fn to_resource(&self) -> Resource {
        Resource {
            name: self.name.clone(),
            mime_type: self.mime_type.clone(),
            uri: self.uri.clone(),
            size: self.size,
            blob: self.data.clone().map(ByteBufB64),
            tags: self.tags.clone(),
            ..Default::default()
        }
    }

    fn metadata_markdown(&self) -> String {
        let mut lines = vec![format!("- label: {}", self.label)];
        if !self.name.trim().is_empty() {
            lines.push(format!("- name: {}", self.name.trim()));
        }
        if let Some(mime_type) = self
            .mime_type
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            lines.push(format!("- mime_type: {}", mime_type.trim()));
        }
        if let Some(uri) = self.uri.as_deref().filter(|value| !value.trim().is_empty()) {
            lines.push(format!("- uri: {}", uri.trim()));
            if let Some(path) = uri.strip_prefix("file://") {
                lines.push(format!("- local_path: {path}"));
            }
        }
        if let Some(size) = self.size {
            lines.push(format!("- size_bytes: {size}"));
        }
        if !self.tags.is_empty() {
            lines.push(format!("- tags: {}", self.tags.join(", ")));
        }
        if self.data.is_some() {
            lines.push("- inline_blob_available: true".to_string());
        }
        if let Some(err) = self.read_error.as_deref() {
            lines.push(format!("- read_error: {err}"));
        }

        lines.join("\n")
    }
}

fn is_other_resource_candidate(resource: &Resource) -> bool {
    resource.blob.is_some()
        || resource
            .uri
            .as_deref()
            .is_some_and(|uri| !uri.trim().is_empty())
        || !resource.name.trim().is_empty()
        || resource
            .mime_type
            .as_deref()
            .is_some_and(|mime_type| !mime_type.trim().is_empty())
        || resource.size.unwrap_or_default() > 0
        || resource.tags.iter().any(|tag| !tag.trim().is_empty())
}

fn attachment_looks_like_pdf(data: &[u8], attachment: &OtherAttachment) -> bool {
    attachment
        .mime_type
        .as_deref()
        .and_then(normalize_mime_type)
        .as_deref()
        .is_some_and(is_pdf_mime_type)
        || infer2::get(data).is_some_and(|kind| is_pdf_mime_type(kind.mime_type()))
        || extension_from_name(&attachment.name).is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
        || attachment
            .uri
            .as_deref()
            .and_then(extension_from_name)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}

fn is_pdf_mime_type(mime_type: &str) -> bool {
    mime_type.trim().eq_ignore_ascii_case("application/pdf")
}

fn utf8_text_from_bytes(data: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(data).ok()?;
    looks_like_text(text).then_some(text)
}

fn looks_like_text(text: &str) -> bool {
    let mut sampled = 0usize;
    let mut suspicious = 0usize;
    for ch in text.chars().take(4096) {
        sampled += 1;
        if ch.is_control() && !matches!(ch, '\n' | '\r' | '\t') {
            suspicious += 1;
        }
    }

    sampled == 0 || suspicious * 100 / sampled <= 5
}

async fn parse_pdf_text(data: &[u8]) -> Result<String, BoxError> {
    let mut config = LiteParseConfig {
        quiet: true,
        ..Default::default()
    };
    let parser = LiteParse::new(config.clone());
    match parser.parse_input(PdfInput::Bytes(data.to_vec())).await {
        Ok(result) => Ok(result.text),
        Err(first_err) => {
            config.ocr_enabled = false;
            let parser = LiteParse::new(config);
            parser
                .parse_input(PdfInput::Bytes(data.to_vec()))
                .await
                .map(|result| result.text)
                .map_err(|second_err| {
                    format!("with OCR enabled: {first_err}; with OCR disabled: {second_err}").into()
                })
        }
    }
}

fn fenced_text(language: &str, text: &str) -> String {
    let fence = "`".repeat(longest_backtick_run(text).max(2) + 1);
    if language.is_empty() {
        format!("{fence}\n{text}\n{fence}")
    } else {
        format!("{fence}{language}\n{text}\n{fence}")
    }
}

fn longest_backtick_run(text: &str) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for ch in text.chars() {
        if ch == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

fn text_language_for_name(name: &str) -> &'static str {
    match extension_from_name(name)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("c") | Some("h") => "c",
        Some("cpp") | Some("cc") | Some("cxx") | Some("hpp") => "cpp",
        Some("css") => "css",
        Some("csv") => "csv",
        Some("go") => "go",
        Some("html") | Some("htm") => "html",
        Some("java") => "java",
        Some("js") | Some("mjs") | Some("cjs") => "javascript",
        Some("json") => "json",
        Some("jsonl") => "jsonl",
        Some("md") | Some("markdown") => "markdown",
        Some("py") => "python",
        Some("rs") => "rust",
        Some("sh") | Some("bash") | Some("zsh") => "bash",
        Some("toml") => "toml",
        Some("ts") | Some("tsx") => "typescript",
        Some("xml") => "xml",
        Some("yaml") | Some("yml") => "yaml",
        _ => "text",
    }
}

fn bounded_text_for_summary(text: &str) -> (String, bool) {
    if text.len() <= MAX_OTHER_TEXT_SUMMARY_BYTES {
        return (text.to_string(), false);
    }

    let head_len = previous_char_boundary(text, MAX_OTHER_TEXT_SUMMARY_BYTES / 2);
    let tail_start = next_char_boundary(text, text.len() - (MAX_OTHER_TEXT_SUMMARY_BYTES / 2));
    let omitted = tail_start.saturating_sub(head_len);
    (
        format!(
            "{}\n\n[... omitted {omitted} bytes from the middle of this attachment ...]\n\n{}",
            &text[..head_len],
            &text[tail_start..]
        ),
        true,
    )
}

fn previous_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn next_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

pub async fn understand_media_resources(
    ctx: &AgentCtx,
    resources: Vec<Resource>,
) -> (Vec<Resource>, Usage) {
    let results = stream::iter(resources.into_iter().map(|mut resource| {
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
                resources: vec![resource.clone()],
                ..Default::default()
            };

            let (understanding, usage) = match ctx.agent_run(input).await {
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
                            "[$system: kind={:?}]\n{} understanding {:?} from attachments\n\n{}",
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
                        "[$system: kind={:?}]\n{} understanding {:?} from attachments\n\nFailed to understand this {} from attachments, error: {}",
                        kind.agent_name(),
                        title_case(kind.noun()),
                        label,
                        kind.noun(),
                        err
                    ),
                    Usage::default(),
                ),
            };

            resource.description = Some(merge_resource_description(
                resource.description.as_deref(),
                &understanding,
            ));
            (resource, usage)
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

fn merge_resource_description(existing: Option<&str>, understanding: &str) -> String {
    let understanding = understanding.trim();
    match existing
        .map(str::trim)
        .filter(|description| !description.is_empty())
    {
        Some(description) if understanding.is_empty() => description.to_string(),
        Some(description) => format!("{description}\n\n---\n\n{understanding}"),
        None => understanding.to_string(),
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

fn strip_data_url_scheme(url: &str) -> Option<&str> {
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

async fn read_limited_response_bytes(response: reqwest::Response) -> Result<Vec<u8>, BoxError> {
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

fn response_content_type(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(normalize_mime_type)
}

fn normalize_mime_type(value: &str) -> Option<String> {
    value
        .split(';')
        .next()
        .map(str::trim)
        .filter(|mime_type| !mime_type.is_empty())
        .map(str::to_ascii_lowercase)
}

fn ensure_media_kind(kind: MediaKind, mime_type: &str, source_name: &str) -> Result<(), BoxError> {
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

fn mime_type_for_data_or_path(data: &[u8], path: &Path, fallback: &str) -> String {
    let name = path.to_string_lossy();
    mime_type_for_data_or_name(data, name.as_ref(), None, fallback)
}

fn mime_type_for_data_or_name(
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
    use crate::util::json_schema::assert_openai_strict_parameters;
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
    fn media_understanding_schema_is_openai_strict() {
        for agent in [
            MediaUnderstandingAgent::image(Vec::new()),
            MediaUnderstandingAgent::audio(Vec::new()),
            MediaUnderstandingAgent::video(Vec::new()),
            MediaUnderstandingAgent::other(Vec::new()),
        ] {
            let definition = agent.definition();
            assert_eq!(definition.strict, Some(true));
            assert_openai_strict_parameters(&definition.parameters);
        }
    }

    #[test]
    fn parses_json_args_with_path_and_question() {
        let args = MediaUnderstandingArgs::from_prompt(
            r#"{"path":"images/cat.png","question":"What is unusual?"}"#,
        );

        assert_eq!(args.path.as_deref(), Some("images/cat.png"));
        assert_eq!(args.url, None);
        assert_eq!(args.question.as_deref(), Some("What is unusual?"));
    }

    #[test]
    fn parses_json_args_with_url_and_question() {
        let args = MediaUnderstandingArgs::from_prompt(
            r#"{"url":"https://example.com/cat.png","question":"What is unusual?"}"#,
        );

        assert_eq!(args.path, None);
        assert_eq!(args.url.as_deref(), Some("https://example.com/cat.png"));
        assert_eq!(args.question.as_deref(), Some("What is unusual?"));
    }

    #[test]
    fn plain_prompt_becomes_question() {
        let args = MediaUnderstandingArgs::from_prompt("describe the scene");

        assert_eq!(args.path, None);
        assert_eq!(args.url, None);
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
    fn detects_other_kind_for_utf8_blob() {
        let resource = Resource {
            name: "notes.txt".to_string(),
            blob: Some(ByteBufB64(b"hello from a text attachment".to_vec())),
            ..Default::default()
        };

        assert_eq!(MediaKind::from_resource(&resource), Some(MediaKind::Other));
    }

    #[test]
    fn detects_other_kind_for_pdf() {
        let resource = Resource {
            name: "report.bin".to_string(),
            mime_type: Some("application/pdf".to_string()),
            ..Default::default()
        };

        assert_eq!(MediaKind::from_resource(&resource), Some(MediaKind::Other));
    }

    #[test]
    fn media_agent_names_include_other_understanding() {
        assert!(media_agent_names().contains(&OTHER_UNDERSTANDING_AGENT_NAME.to_string()));
        assert!(supported_media_resource_tags().contains(&"pdf".to_string()));
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
    fn media_understanding_merges_into_resource_description() {
        assert_eq!(
            merge_resource_description(None, "Image description"),
            "Image description"
        );
        assert_eq!(
            merge_resource_description(Some("Original description"), "Image description"),
            "Original description\n\n---\n\nImage description"
        );
        assert_eq!(
            merge_resource_description(Some("Original description"), "   "),
            "Original description"
        );
    }

    #[test]
    fn utf8_text_detection_rejects_control_heavy_binary() {
        assert_eq!(utf8_text_from_bytes(b"plain text"), Some("plain text"));
        assert!(utf8_text_from_bytes(&[0, 0, 0, 0, 0, 0]).is_none());
    }

    #[test]
    fn bounded_text_for_summary_preserves_char_boundaries() {
        let text = "你".repeat(MAX_OTHER_TEXT_SUMMARY_BYTES);
        let (bounded, truncated) = bounded_text_for_summary(&text);

        assert!(truncated);
        assert!(bounded.contains("omitted"));
        assert!(bounded.is_char_boundary(bounded.len()));
    }

    #[test]
    fn fenced_text_extends_backtick_fence() {
        let fenced = fenced_text("markdown", "```inner```");

        assert!(fenced.starts_with("````markdown"));
        assert!(fenced.ends_with("````"));
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
            uri: Some("https://example.com/clip.mp4".to_string()),
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
                assert_eq!(file_uri, "https://example.com/clip.mp4");
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
    async fn content_from_location_fetches_http_url() {
        let url = spawn_media_http_server(PNG_SIGNATURE.to_vec(), "image/png").await;
        let agent = MediaUnderstandingAgent::image(Vec::new());

        let content = agent
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
        let agent = MediaUnderstandingAgent::image(Vec::new());

        let content = agent
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
        let agent = MediaUnderstandingAgent::image(Vec::new());
        let content = agent
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
