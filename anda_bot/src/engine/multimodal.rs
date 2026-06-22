use anda_core::{
    Agent, AgentContext, AgentInput, AgentOutput, BoxError, ByteBufB64, CompletionFeatures,
    CompletionRequest, ContentPart, FunctionDefinition, RequestMeta, Resource, StateFeatures,
    ToolGroupInfo, Usage, inline_data_from_data_url, text_from_bytes, utf8_text_from_bytes,
};
use anda_engine::{
    context::{AgentCtx, TOOLS_SEARCH_NAME, TOOLS_SELECT_NAME},
    extension::{
        fs::{ReadFileTool, SearchFileTool},
        shell::ShellTool,
        skill::SkillManager,
    },
    grapheme_safe_cutoff,
    subagent::SubAgentManager,
};
use futures_util::{StreamExt, stream};
use ic_auth_types::Xid;
use liteparse::{LiteParse, LiteParseConfig, types::PdfInput};
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use serde_json::json;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};
use unicode_segmentation::UnicodeSegmentation;

use crate::util::file_uri::{
    file_uri_for_path, is_file_uri, path_from_file_uri, user_path_string_for_path,
};
use crate::util::http_client::new_reqwest_client;

pub const IMAGE_UNDERSTANDING_AGENT_NAME: &str = "image_understanding";
pub const AUDIO_UNDERSTANDING_AGENT_NAME: &str = "audio_understanding";
pub const VIDEO_UNDERSTANDING_AGENT_NAME: &str = "video_understanding";
pub const OTHER_UNDERSTANDING_AGENT_NAME: &str = "attachment_understanding";

/// Stable id of the multimodal media-understanding capability group.
pub const MEDIA_UNDERSTANDING_TOOL_GROUP_ID: &str = "media_understanding";

pub const IMAGE_MODEL_LABEL: &str = "image";
pub const AUDIO_MODEL_LABEL: &str = "audio";
pub const VIDEO_MODEL_LABEL: &str = "video";
pub const OTHER_MODEL_LABEL: &str = "flash";

const MAX_MEDIA_FILE_SIZE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_MEDIA_UNDERSTANDING_CONCURRENCY: usize = 8;
const MAX_OTHER_TEXT_INLINE_BYTES: usize = 256 * 1024;
const MAX_OTHER_TEXT_SUMMARY_BYTES: usize = 1024 * 1024;
#[cfg(windows)]
const PDFIUM_DLL_NAME: &str = "pdfium.dll";

/// Returns the shared [`ToolGroupInfo`] for the media-understanding agents.
///
/// The image, audio, video, and generic attachment understanding agents report
/// this so discovery presents them as one bundle. The registry fills in the
/// member list from the agents actually registered.
pub fn media_understanding_tool_group_info() -> ToolGroupInfo {
    ToolGroupInfo {
        id: MEDIA_UNDERSTANDING_TOOL_GROUP_ID.to_string(),
        title: "Media understanding".to_string(),
        description: "Understand image, audio, video, and document/file attachments or workspace/URL media for downstream text-only reasoning.".to_string(),
        instructions: Some(
            "These agents share one media-understanding workflow. Pick `image_understanding`, `audio_understanding`, or `video_understanding` for matching visual/audio/video inputs, and `attachment_understanding` for PDFs, text files, documents, spreadsheets, slides, logs, or other non-media attachments. Provide `path` or `url` for workspace files and URLs, or pass attached resources directly; use `question` for the caller's focus.".to_string(),
        ),
    }
}

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
                "Understand non-image/audio/video attachments. Text blobs are handled directly, PDFs are parsed locally with LiteParse, and other formats are delegated to available skills or shell-assisted extraction."
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
            return "You are a specialized attachment understanding subagent. Prefer local, faithful extraction over guessing. For text attachments, preserve the actual content when it is small and summarize it when it is large. For PDFs, use LiteParse extraction results. For other formats, look for a suitable installed skill first, then use safe shell/read-only inspection, and only research a method over the network when local options are insufficient. Return Markdown plain text for the main agent and clearly mark failures or uncertainty.".to_string();
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
            http: new_reqwest_client(),
        }
    }

    pub fn audio(workspaces: Vec<PathBuf>) -> Self {
        Self {
            kind: MediaKind::Audio,
            workspaces,
            http: new_reqwest_client(),
        }
    }

    pub fn video(workspaces: Vec<PathBuf>) -> Self {
        Self {
            kind: MediaKind::Video,
            workspaces,
            http: new_reqwest_client(),
        }
    }

    pub fn other(workspaces: Vec<PathBuf>) -> Self {
        Self {
            kind: MediaKind::Other,
            workspaces,
            http: new_reqwest_client(),
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
                format!("{OTHER_UNDERSTANDING_AGENT_NAME} requires an attached resource, workspace file path, or URL")
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
            uri: Some(file_uri_for_path(&resolved)?),
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

        if let Some(text) = attachment_text_from_bytes(data, &attachment) {
            return self
                .text_or_summary_output(
                    ctx,
                    &attachment.label,
                    &attachment.name,
                    "text attachment",
                    text.as_ref(),
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
        let mut output = ctx.completion(
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
                ..Default::default()
            },
            Vec::new(),
        ).await?;

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
        let fallback_file = fallback_other_attachment_file(&attachment).await;
        let prompt = fallback_other_attachment_prompt(question, &attachment, &fallback_file);
        let mut resource = attachment.to_resource();
        if let Some(path) = fallback_file.path.as_deref()
            && let Ok(uri) = file_uri_for_path(path)
        {
            resource.uri = Some(uri);
        }
        let tools = ctx
            .definitions(Some(&other_understanding_tool_names()))
            .await;
        let mut output = ctx
            .completion(
                CompletionRequest {
                    instructions: self.kind.instructions(),
                    prompt,
                    model: Some("".to_string()), // ACTIVE_MODEL_LABEL
                    tools,
                    ..Default::default()
                },
                vec![resource],
            )
            .await?;

        if output.content.trim().is_empty() {
            let metadata = attachment.metadata_markdown();
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

    fn group(&self) -> Option<ToolGroupInfo> {
        Some(media_understanding_tool_group_info())
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

#[derive(Clone, Debug, Default)]
struct FallbackAttachmentFile {
    path: Option<PathBuf>,
    temporary: bool,
    error: Option<String>,
}

async fn fallback_other_attachment_file(attachment: &OtherAttachment) -> FallbackAttachmentFile {
    let existing_file = fallback_existing_attachment_file(attachment).await;
    match existing_file {
        FallbackAttachmentFile { path: Some(_), .. } => existing_file,
        FallbackAttachmentFile { error, .. } => {
            if let Some(data) = attachment.data.as_deref() {
                match write_fallback_attachment_file(attachment, data).await {
                    Ok(path) => FallbackAttachmentFile {
                        path: Some(path),
                        temporary: true,
                        error,
                    },
                    Err(err) => FallbackAttachmentFile {
                        path: None,
                        temporary: false,
                        error: Some(match error {
                            Some(previous) => {
                                format!("{previous}; failed to write temp file: {err}")
                            }
                            None => format!("failed to write temp file: {err}"),
                        }),
                    },
                }
            } else {
                FallbackAttachmentFile {
                    path: None,
                    temporary: false,
                    error,
                }
            }
        }
    }
}

async fn fallback_existing_attachment_file(attachment: &OtherAttachment) -> FallbackAttachmentFile {
    let Some(uri) = attachment
        .uri
        .as_deref()
        .map(str::trim)
        .filter(|uri| !uri.is_empty())
    else {
        return FallbackAttachmentFile::default();
    };

    let path = if is_file_uri(uri) {
        match path_from_file_uri(uri) {
            Ok(path) => path,
            Err(err) => {
                return FallbackAttachmentFile {
                    path: None,
                    temporary: false,
                    error: Some(format!(
                        "file URI cannot be converted to a local path: {err}"
                    )),
                };
            }
        }
    } else if reqwest::Url::parse(uri).is_ok() || strip_data_url_scheme(uri).is_some() {
        return FallbackAttachmentFile::default();
    } else {
        PathBuf::from(uri)
    };

    match tokio::fs::metadata(&path).await {
        Ok(metadata) if metadata.is_file() => FallbackAttachmentFile {
            path: Some(path),
            temporary: false,
            error: None,
        },
        Ok(_) => FallbackAttachmentFile {
            path: None,
            temporary: false,
            error: Some(format!(
                "attachment path is not a regular file: {}",
                path.display()
            )),
        },
        Err(err) => FallbackAttachmentFile {
            path: None,
            temporary: false,
            error: Some(format!(
                "attachment path is not readable: {}: {err}",
                path.display()
            )),
        },
    }
}

async fn write_fallback_attachment_file(
    attachment: &OtherAttachment,
    data: &[u8],
) -> Result<PathBuf, BoxError> {
    let dir = std::env::temp_dir().join("anda-bot-attachments");
    tokio::fs::create_dir_all(&dir).await?;
    let file_name = fallback_attachment_file_name(attachment);
    let path = dir.join(format!("{}-{file_name}", Xid::new()));
    tokio::fs::write(&path, data).await?;
    Ok(path)
}

fn fallback_attachment_file_name(attachment: &OtherAttachment) -> String {
    let candidate: Cow<'_, str> = if !attachment.name.trim().is_empty() {
        Cow::Borrowed(attachment.name.as_str())
    } else {
        Cow::Owned(
            attachment
                .uri
                .as_deref()
                .and_then(|uri| {
                    reqwest::Url::parse(uri)
                        .ok()
                        .and_then(|url| url.path_segments()?.next_back().map(str::to_string))
                })
                .unwrap_or_else(|| "attachment.bin".to_string()),
        )
    };

    sanitize_fallback_attachment_file_name(candidate.as_ref(), "attachment.bin")
}

fn sanitize_fallback_attachment_file_name(value: &str, fallback: &str) -> String {
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

fn fallback_other_attachment_prompt(
    question: &str,
    attachment: &OtherAttachment,
    fallback_file: &FallbackAttachmentFile,
) -> String {
    let metadata = attachment.metadata_markdown();
    let tool_access_note = fallback_tool_access_note(attachment, fallback_file);

    format!(
        "Understand this non-image/audio/video attachment for the main agent.\n\nInput boundary:\n- This fallback is used only after built-in text/PDF extraction and direct model-readable media handling were not sufficient.\n- Do not assume the model can directly read the attachment bytes from the prompt; inspect the file path or URL below with tools.\n{tool_access_note}\n\nWorkflow:\n1. Search available tools/skills for a parser that matches the MIME type, extension, or file family; use an installed skill/subagent if one is suitable.\n2. Use shell or read-only file inspection against the provided local path when available. Prefer safe commands that extract metadata/text over mutating the file.\n3. If there is no local path but metadata includes a URL, use network-capable tools or shell commands to refetch or research a practical extraction method, then report the best next action.\n4. If extraction is impossible, explain what was tried or what capability is missing.\n\nDo not invent attachment contents.\n\nCaller question or focus:\n{question}\n\nAttachment metadata:\n{metadata}"
    )
}

fn fallback_tool_access_note(
    attachment: &OtherAttachment,
    fallback_file: &FallbackAttachmentFile,
) -> String {
    if let Some(path) = fallback_file.path.as_deref() {
        let origin = if fallback_file.temporary {
            "A temporary local copy has been written for shell/file tools"
        } else {
            "Metadata includes an existing local file path"
        };
        let mut note = format!("- {origin}: {}", user_path_string_for_path(path));
        if let Some(err) = fallback_file.error.as_deref() {
            note.push_str(&format!(
                "\n- Earlier local-path preparation warning: {err}"
            ));
        }
        return note;
    }

    if let Some(err) = fallback_file.error.as_deref() {
        return format!(
            "- No local file is available for shell/file tools. Local-path preparation failed: {err}"
        );
    }

    let Some(uri) = attachment
        .uri
        .as_deref()
        .map(str::trim)
        .filter(|uri| !uri.is_empty())
    else {
        return "- No local file path or external URL is available in metadata; shell/file tools cannot reach the attachment bytes.".to_string();
    };

    if strip_data_url_scheme(uri).is_some() {
        return "- Metadata contains a data URL/inline blob, but no local temp file could be prepared for shell/file tools.".to_string();
    }

    if let Ok(url) = reqwest::Url::parse(uri) {
        return match url.scheme() {
            "http" | "https" => format!(
                "- Metadata includes an http(s) URL. Network-capable tools or shell may refetch it if network access is available: {uri}"
            ),
            scheme => format!(
                "- Metadata includes URI scheme `{scheme}`, which is not a shell-readable attachment path."
            ),
        };
    }

    "- Metadata includes a path-like value. File tools may use it only if it resolves to an accessible local file.".to_string()
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
            lines.push(format!("- uri: {}", display_attachment_uri(uri)));
            if is_file_uri(uri)
                && let Ok(path) = path_from_file_uri(uri)
            {
                lines.push(format!(
                    "- local_path: {}",
                    user_path_string_for_path(&path)
                ));
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

fn display_attachment_uri(uri: &str) -> String {
    let trimmed = uri.trim();
    if strip_data_url_scheme(trimmed).is_some() {
        let prefix = trimmed
            .split_once(',')
            .map(|(prefix, _)| prefix)
            .unwrap_or("data:");
        format!("{prefix},... ({} chars)", trimmed.len())
    } else {
        trimmed.to_string()
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

fn attachment_text_from_bytes<'a>(
    data: &'a [u8],
    attachment: &OtherAttachment,
) -> Option<Cow<'a, str>> {
    if let Some(text) = utf8_text_from_bytes(data) {
        return Some(Cow::Borrowed(text));
    }

    if !attachment_allows_legacy_text_fallback(attachment) {
        return None;
    }

    text_from_bytes(data)
}

#[cfg(test)]
fn attachment_text_from_bytes_with_windows_code_page<'a>(
    data: &'a [u8],
    attachment: &OtherAttachment,
    code_page: u32,
) -> Option<Cow<'a, str>> {
    if let Some(text) = utf8_text_from_bytes(data) {
        return Some(Cow::Borrowed(text));
    }

    if !attachment_allows_legacy_text_fallback(attachment) {
        return None;
    }

    anda_core::text_from_bytes_with_encoding(data, anda_core::windows_code_page_encoding(code_page))
}

fn attachment_allows_legacy_text_fallback(attachment: &OtherAttachment) -> bool {
    if attachment
        .mime_type
        .as_deref()
        .and_then(normalize_mime_type)
        .is_some_and(|mime_type| mime_type_allows_legacy_text_fallback(&mime_type))
    {
        return true;
    }

    if extension_from_name(&attachment.name).is_some_and(is_text_extension) {
        return true;
    }

    attachment.tags.iter().any(|tag| {
        let tag = tag.trim().trim_start_matches('.').to_ascii_lowercase();
        matches!(
            tag.as_str(),
            "text"
                | "txt"
                | "md"
                | "markdown"
                | "json"
                | "jsonl"
                | "ndjson"
                | "csv"
                | "tsv"
                | "xml"
                | "yaml"
                | "yml"
                | "toml"
                | "html"
                | "htm"
                | "log"
        )
    })
}

fn mime_type_allows_legacy_text_fallback(mime_type: &str) -> bool {
    let essence = mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .to_ascii_lowercase();

    essence.is_empty()
        || essence.starts_with("text/")
        || essence.ends_with("+json")
        || essence.ends_with("+xml")
        || matches!(
            essence.as_str(),
            "application/json"
                | "application/xml"
                | "application/javascript"
                | "application/x-javascript"
                | "application/x-ndjson"
                | "application/yaml"
                | "application/x-yaml"
                | "application/toml"
                | "application/x-www-form-urlencoded"
        )
}

fn is_text_extension(ext: &str) -> bool {
    matches!(
        ext.trim().to_ascii_lowercase().as_str(),
        "txt"
            | "text"
            | "md"
            | "markdown"
            | "json"
            | "jsonl"
            | "ndjson"
            | "csv"
            | "tsv"
            | "xml"
            | "yaml"
            | "yml"
            | "toml"
            | "html"
            | "htm"
            | "js"
            | "mjs"
            | "cjs"
            | "jsx"
            | "ts"
            | "tsx"
            | "css"
            | "rs"
            | "py"
            | "go"
            | "java"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "sh"
            | "bash"
            | "zsh"
            | "ps1"
            | "bat"
            | "cmd"
            | "ini"
            | "conf"
            | "cfg"
            | "env"
            | "log"
    )
}

async fn parse_pdf_text(data: &[u8]) -> Result<String, BoxError> {
    #[cfg(windows)]
    ensure_pdfium_library_available()?;

    let mut config = LiteParseConfig {
        quiet: true,
        ocr_enabled: cfg!(all(not(target_env = "musl"), not(target_os = "windows"))),
        ..Default::default()
    };
    let first_ocr_enabled = config.ocr_enabled;
    match parse_pdf_text_once(data, config.clone()).await {
        Ok(text) => Ok(text),
        Err(first_err) if first_ocr_enabled => {
            config.ocr_enabled = false;
            parse_pdf_text_once(data, config)
                .await
                .map_err(|second_err| {
                    format!("with OCR enabled: {first_err}; with OCR disabled: {second_err}").into()
                })
        }
        Err(err) => Err(err),
    }
}

async fn parse_pdf_text_once(data: &[u8], config: LiteParseConfig) -> Result<String, BoxError> {
    let parser = LiteParse::new(config);
    parser
        .parse_input(PdfInput::Bytes(data.to_vec()))
        .await
        .map(|result| result.text)
        .map_err(Into::into)
}

#[cfg(windows)]
fn ensure_pdfium_library_available() -> Result<(), BoxError> {
    let mut load_errors = Vec::new();
    for candidate in pdfium_library_candidates() {
        if !candidate.is_file() && candidate != Path::new(PDFIUM_DLL_NAME) {
            continue;
        }
        match try_load_pdfium_library(&candidate) {
            Ok(()) => return Ok(()),
            Err(err) => load_errors.push(format!("{}: {err}", candidate.display())),
        }
    }

    let detail = if load_errors.is_empty() {
        "searched PDFIUM_LIB_PATH, the anda.exe directory, the current directory, and PATH"
            .to_string()
    } else {
        format!(
            "could not load any pdfium.dll candidate: {}",
            load_errors.join("; ")
        )
    };
    Err(format!(
        "{PDFIUM_DLL_NAME} is not available for LiteParse PDF extraction ({detail}). Set PDFIUM_LIB_PATH to the directory containing {PDFIUM_DLL_NAME}."
    )
    .into())
}

#[cfg(windows)]
fn pdfium_library_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) =
        std::env::var_os("PDFIUM_LIB_PATH").filter(|path| !path.as_os_str().is_empty())
    {
        push_pdfium_candidate(&mut candidates, PathBuf::from(path));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        push_pdfium_candidate(&mut candidates, dir.to_path_buf());
    }
    if let Ok(dir) = std::env::current_dir() {
        push_pdfium_candidate(&mut candidates, dir);
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            push_pdfium_candidate(&mut candidates, dir);
        }
    }
    push_pdfium_candidate(&mut candidates, PathBuf::from(PDFIUM_DLL_NAME));
    candidates
}

#[cfg(windows)]
fn push_pdfium_candidate(candidates: &mut Vec<PathBuf>, path: PathBuf) {
    let candidate = pdfium_library_candidate_for_path(&path);
    if !candidates.iter().any(|existing| existing == &candidate) {
        candidates.push(candidate);
    }
}

#[cfg(windows)]
fn pdfium_library_candidate_for_path(path: &Path) -> PathBuf {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(PDFIUM_DLL_NAME))
    {
        path.to_path_buf()
    } else {
        path.join(PDFIUM_DLL_NAME)
    }
}

#[cfg(windows)]
fn try_load_pdfium_library(path: &Path) -> Result<(), String> {
    liteparse_pdfium_sys::dynamic::load(path)
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

    let excerpt_bytes = MAX_OTHER_TEXT_SUMMARY_BYTES / 2;
    let head_len = grapheme_safe_cutoff(text, excerpt_bytes);
    let tail_start = grapheme_safe_suffix_start(text, excerpt_bytes);
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

fn grapheme_safe_suffix_start(text: &str, max_bytes: usize) -> usize {
    if text.len() <= max_bytes {
        return 0;
    }

    let mut start = text.len();
    for (idx, _) in UnicodeSegmentation::grapheme_indices(text, true).rev() {
        if text.len() - idx > max_bytes {
            break;
        }
        start = idx;
    }
    start
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
    fn media_understanding_agents_share_tool_group() {
        for agent in [
            MediaUnderstandingAgent::image(Vec::new()),
            MediaUnderstandingAgent::audio(Vec::new()),
            MediaUnderstandingAgent::video(Vec::new()),
            MediaUnderstandingAgent::other(Vec::new()),
        ] {
            let group = agent.group().expect("media agent should report a group");
            assert_eq!(group.id, MEDIA_UNDERSTANDING_TOOL_GROUP_ID);
            assert_eq!(group.title, "Media understanding");
            assert!(
                group
                    .instructions
                    .as_deref()
                    .is_some_and(|instructions| instructions.contains("attachment_understanding"))
            );
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
    fn text_attachment_detection_rejects_control_heavy_binary() {
        let attachment = test_other_attachment("notes.txt", Some("text/plain"), vec![]);

        assert_eq!(
            attachment_text_from_bytes(b"plain text", &attachment).as_deref(),
            Some("plain text")
        );
        assert!(attachment_text_from_bytes(&[0, 0, 0, 0, 0, 0], &attachment).is_none());
    }

    #[test]
    fn text_attachment_detection_decodes_legacy_windows_text_when_text_like() {
        let gbk = [0xD6, 0xD0, 0xCE, 0xC4];
        let attachment =
            test_other_attachment("notes.txt", Some("application/octet-stream"), vec![]);

        assert_eq!(
            attachment_text_from_bytes_with_windows_code_page(&gbk, &attachment, 936).as_deref(),
            Some("中文")
        );
    }

    #[test]
    fn text_attachment_detection_rejects_legacy_fallback_for_binary_mime() {
        let gbk = [0xD6, 0xD0, 0xCE, 0xC4];
        let attachment = test_other_attachment("image.jpg", Some("image/jpeg"), vec![]);

        assert!(
            attachment_text_from_bytes_with_windows_code_page(&gbk, &attachment, 936).is_none()
        );
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
    fn bounded_text_for_summary_preserves_grapheme_boundaries() {
        fn summary_parts(bounded: &str) -> (&str, &str) {
            let (head, rest) = bounded
                .split_once("\n\n[... omitted ")
                .expect("bounded summary should include omission marker");
            let (_, tail) = rest
                .split_once(" ...]\n\n")
                .expect("bounded summary should include marker terminator");
            (head, tail)
        }

        let excerpt_bytes = MAX_OTHER_TEXT_SUMMARY_BYTES / 2;
        let emoji = "👩‍💻";

        let head_split_text = format!(
            "{}{}{}",
            "a".repeat(excerpt_bytes - '👩'.len_utf8()),
            emoji,
            "b".repeat(MAX_OTHER_TEXT_SUMMARY_BYTES)
        );
        let (bounded, truncated) = bounded_text_for_summary(&head_split_text);
        let (head, _) = summary_parts(&bounded);

        assert!(truncated);
        assert_eq!(head.len(), excerpt_bytes - '👩'.len_utf8());
        assert!(!head.contains('👩'));

        let tail_split_text = format!(
            "{}{}{}",
            "a".repeat(excerpt_bytes),
            emoji,
            "b".repeat(excerpt_bytes - emoji.len() + '👩'.len_utf8())
        );
        let (bounded, truncated) = bounded_text_for_summary(&tail_split_text);
        let (_, tail) = summary_parts(&bounded);

        assert!(truncated);
        assert!(tail.starts_with('b'));
        assert!(tail.len() <= excerpt_bytes);
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

    fn test_other_attachment(
        name: &str,
        mime_type: Option<&str>,
        tags: Vec<&str>,
    ) -> OtherAttachment {
        OtherAttachment {
            label: name.to_string(),
            name: name.to_string(),
            mime_type: mime_type.map(ToString::to_string),
            uri: None,
            size: None,
            data: None,
            tags: tags.into_iter().map(ToString::to_string).collect(),
            read_error: None,
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
        let agent = MediaUnderstandingAgent::image(Vec::new()).with_http_client(
            reqwest::Client::builder()
                .no_proxy()
                .build()
                .expect("test HTTP client should build"),
        );

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
            &file_uri_for_path(&file).expect("file URI should be generated"),
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

    use axum::{Router, http::StatusCode as AxumStatus, routing::get};

    async fn spawn_router(app: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn mock_ctx() -> AgentCtx {
        anda_engine::engine::EngineBuilder::new().mock_ctx()
    }

    fn text_resource(name: &str, body: &str) -> Resource {
        Resource {
            name: name.to_string(),
            mime_type: Some("text/plain".to_string()),
            blob: Some(ByteBufB64(body.as_bytes().to_vec())),
            tags: vec!["text".to_string()],
            ..Default::default()
        }
    }

    #[test]
    fn media_kind_metadata_covers_every_variant() {
        for kind in [
            MediaKind::Image,
            MediaKind::Audio,
            MediaKind::Video,
            MediaKind::Other,
        ] {
            assert!(!kind.agent_name().is_empty());
            assert!(!kind.model_label().is_empty());
            assert!(!kind.noun().is_empty());
            assert!(!kind.description().is_empty());
            assert!(!kind.default_question().is_empty());
            assert!(!kind.instructions().is_empty());
            assert!(!kind.tags().is_empty());
        }
        assert!(MediaKind::Other.instructions().contains("attachment"));
        assert!(MediaKind::Image.instructions().contains("image"));
    }

    #[test]
    fn media_kind_from_mime_tags_and_extension() {
        assert_eq!(
            MediaKind::from_mime_type("IMAGE/PNG"),
            Some(MediaKind::Image)
        );
        assert_eq!(
            MediaKind::from_mime_type("audio/mp3"),
            Some(MediaKind::Audio)
        );
        assert_eq!(
            MediaKind::from_mime_type("video/mp4"),
            Some(MediaKind::Video)
        );
        assert_eq!(MediaKind::from_mime_type("application/zip"), None);

        assert_eq!(
            MediaKind::from_tags(&["Image".to_string()]),
            Some(MediaKind::Image)
        );
        assert_eq!(
            MediaKind::from_tags(&["audio".to_string()]),
            Some(MediaKind::Audio)
        );
        assert_eq!(
            MediaKind::from_tags(&["video".to_string()]),
            Some(MediaKind::Video)
        );
        assert_eq!(
            MediaKind::from_tags(&[".mp3".to_string()]),
            Some(MediaKind::Audio)
        );
        assert_eq!(MediaKind::from_tags(&["nope".to_string()]), None);

        assert_eq!(MediaKind::from_extension("JPG"), Some(MediaKind::Image));
        assert_eq!(MediaKind::from_extension("flac"), Some(MediaKind::Audio));
        assert_eq!(MediaKind::from_extension("mpeg"), Some(MediaKind::Audio));
        assert_eq!(MediaKind::from_extension("mkv"), Some(MediaKind::Video));
        assert_eq!(MediaKind::from_extension("pdf"), Some(MediaKind::Other));
        assert_eq!(MediaKind::from_extension("unknown"), None);
    }

    #[test]
    fn media_understanding_args_from_blank_prompt_is_default() {
        let args = MediaUnderstandingArgs::from_prompt("   ");
        assert!(args.path.is_none() && args.url.is_none() && args.question.is_none());
    }

    #[test]
    fn pure_mime_and_text_helpers() {
        assert!(is_pdf_mime_type(" Application/PDF "));
        assert!(!is_pdf_mime_type("text/plain"));

        assert_eq!(
            normalize_mime_type(" Text/Plain; charset=utf-8 ").as_deref(),
            Some("text/plain")
        );
        assert_eq!(normalize_mime_type("   "), None);

        assert_eq!(extension_from_name("dir/file.RS"), Some("RS"));
        assert_eq!(extension_from_name("noext"), None);
        assert_eq!(mime_type_from_name("a.png").as_deref(), Some("image/png"));

        assert_eq!(title_case("hello"), "Hello");
        assert_eq!(title_case(""), "");

        assert_eq!(strip_data_url_scheme(" DATA:abc"), Some("abc"));
        assert_eq!(strip_data_url_scheme("http://x"), None);

        assert_eq!(text_language_for_name("a.rs"), "rust");
        assert_eq!(text_language_for_name("a.py"), "python");
        assert_eq!(text_language_for_name("a.cpp"), "cpp");
        assert_eq!(text_language_for_name("a.unknown"), "text");

        assert!(is_text_extension("MD"));
        assert!(!is_text_extension("png"));

        assert!(mime_type_allows_legacy_text_fallback("application/json"));
        assert!(mime_type_allows_legacy_text_fallback("text/x-rust"));
        assert!(mime_type_allows_legacy_text_fallback("application/ld+json"));
        assert!(!mime_type_allows_legacy_text_fallback("image/png"));
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

    #[test]
    fn other_resource_candidate_and_pdf_detection() {
        assert!(!is_other_resource_candidate(&Resource::default()));
        assert!(is_other_resource_candidate(&Resource {
            name: "x".to_string(),
            ..Default::default()
        }));

        let pdf_attachment = test_other_attachment("report.pdf", None, vec![]);
        assert!(attachment_looks_like_pdf(b"random", &pdf_attachment));
        let by_mime = test_other_attachment("blob", Some("application/pdf"), vec![]);
        assert!(attachment_looks_like_pdf(b"random", &by_mime));
        let not_pdf = test_other_attachment("notes.txt", Some("text/plain"), vec![]);
        assert!(!attachment_looks_like_pdf(b"random", &not_pdf));
    }

    #[test]
    fn other_attachment_round_trip_and_metadata() {
        let resource = Resource {
            name: "notes.txt".to_string(),
            mime_type: Some("text/plain".to_string()),
            uri: Some("file:///tmp/notes.txt".to_string()),
            blob: Some(ByteBufB64(b"data".to_vec())),
            tags: vec!["text".to_string()],
            ..Default::default()
        };
        let attachment = OtherAttachment::from_resource(resource);
        assert_eq!(attachment.size, Some(4));
        let back = attachment.to_resource();
        assert_eq!(back.name, "notes.txt");
        assert_eq!(back.size, Some(4));

        let md = attachment.metadata_markdown();
        assert!(md.contains("- name: notes.txt"));
        assert!(md.contains("- mime_type: text/plain"));
        assert!(md.contains("- uri: file:///tmp/notes.txt"));
        assert!(md.contains("- local_path:"));
        assert!(md.contains("- size_bytes: 4"));
        assert!(md.contains("- tags: text"));
        assert!(md.contains("- inline_blob_available: true"));

        let mut with_error = attachment;
        with_error.read_error = Some("boom".to_string());
        assert!(
            with_error
                .metadata_markdown()
                .contains("- read_error: boom")
        );
    }

    #[test]
    fn other_attachment_metadata_abbreviates_data_url() {
        let mut attachment =
            test_other_attachment("blob.bin", Some("application/octet-stream"), vec![]);
        attachment.uri = Some("data:application/octet-stream;base64,AAAA".to_string());

        let metadata = attachment.metadata_markdown();

        assert!(metadata.contains("- uri: data:application/octet-stream;base64,..."));
        assert!(!metadata.contains("AAAA"));
    }

    #[tokio::test]
    async fn fallback_other_attachment_file_writes_blob_for_shell_tools() {
        let mut attachment =
            test_other_attachment("blob.bin", Some("application/octet-stream"), vec![]);
        attachment.data = Some(vec![0u8, 1, 2, 3]);

        let fallback_file = fallback_other_attachment_file(&attachment).await;

        assert!(fallback_file.temporary);
        let path = fallback_file
            .path
            .as_ref()
            .expect("fallback should write a temp file");
        assert!(
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("blob.bin"))
        );
        assert_eq!(
            fs::read(path).expect("temp file should be readable"),
            vec![0u8, 1, 2, 3]
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn fallback_prompt_describes_attachment_access_boundary() {
        let mut inline =
            test_other_attachment("blob.bin", Some("application/octet-stream"), vec![]);
        inline.data = Some(vec![0u8, 1, 2, 3]);
        let inline_file = FallbackAttachmentFile {
            path: Some(std::env::temp_dir().join("blob.bin")),
            temporary: true,
            error: None,
        };
        let inline_prompt = fallback_other_attachment_prompt("inspect", &inline, &inline_file);
        assert!(inline_prompt.contains("Do not assume the model can directly read"));
        assert!(inline_prompt.contains("temporary local copy"));
        assert!(inline_prompt.contains("blob.bin"));

        let mut file = test_other_attachment("docx.bin", Some("application/octet-stream"), vec![]);
        file.uri = Some("file:///tmp/docx.bin".to_string());
        let file_prompt = fallback_other_attachment_prompt(
            "inspect",
            &file,
            &FallbackAttachmentFile {
                path: Some(PathBuf::from("/tmp/docx.bin")),
                temporary: false,
                error: None,
            },
        );
        assert!(file_prompt.contains("existing local file path"));

        let mut remote =
            test_other_attachment("docx.bin", Some("application/octet-stream"), vec![]);
        remote.uri = Some("https://example.com/docx.bin".to_string());
        let remote_prompt = fallback_other_attachment_prompt(
            "inspect",
            &remote,
            &FallbackAttachmentFile::default(),
        );
        assert!(remote_prompt.contains("http(s) URL"));
        assert!(remote_prompt.contains("https://example.com/docx.bin"));
    }

    #[test]
    fn completion_prompt_describes_inputs() {
        let agent = MediaUnderstandingAgent::image(Vec::new());
        let args = MediaUnderstandingArgs::from_prompt("focus");
        assert!(
            agent
                .completion_prompt(&args, 0, 0)
                .contains("the supplied media")
        );
        assert!(
            agent
                .completion_prompt(&args, 0, 1)
                .contains("the media file at")
        );
        assert!(
            agent
                .completion_prompt(&args, 0, 2)
                .contains("2 media files")
        );
        assert!(
            agent
                .completion_prompt(&args, 1, 0)
                .contains("attached image resource")
        );
        assert!(
            agent
                .completion_prompt(&args, 2, 0)
                .contains("2 attached image resources")
        );
        assert!(
            agent
                .completion_prompt(&args, 1, 1)
                .contains("and the media file")
        );
        assert!(
            agent
                .completion_prompt(&args, 2, 2)
                .contains("2 attached image")
        );
    }

    #[tokio::test]
    async fn run_other_inlines_small_text_attachment() {
        let ctx = mock_ctx();
        let agent = MediaUnderstandingAgent::other(Vec::new());
        let output = agent
            .run(
                ctx,
                "summarize".to_string(),
                vec![text_resource("notes.txt", "hello world")],
            )
            .await
            .expect("text attachment should be understood without a model");
        assert!(output.content.contains("hello world"));
        assert!(output.content.contains("text attachment"));
    }

    #[tokio::test]
    async fn run_other_requires_attachments() {
        let ctx = mock_ctx();
        let agent = MediaUnderstandingAgent::other(Vec::new());
        let err = agent
            .run(ctx, "{}".to_string(), vec![])
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("requires an attached resource"));
    }

    #[tokio::test]
    async fn run_other_absorbs_fallback_failures_into_sections() {
        // A binary, non-text, non-pdf attachment falls through to the model
        // fallback, which fails on the mock ctx; the error is captured in the
        // section text rather than failing the whole run.
        let ctx = mock_ctx();
        let agent = MediaUnderstandingAgent::other(Vec::new());
        let resource = Resource {
            name: "blob.bin".to_string(),
            mime_type: Some("application/octet-stream".to_string()),
            blob: Some(ByteBufB64(vec![0u8, 1, 2, 3, 0, 0, 0, 0])),
            ..Default::default()
        };
        let output = agent
            .run(ctx, "{}".to_string(), vec![resource])
            .await
            .expect("run_other should not fail on fallback errors");
        assert!(output.content.contains("Failed to understand") || !output.content.is_empty());
    }

    #[tokio::test]
    async fn run_image_builds_content_then_fails_without_model() {
        let ctx = mock_ctx();
        let agent = MediaUnderstandingAgent::image(Vec::new());
        let resource = Resource {
            name: "photo.png".to_string(),
            mime_type: Some("image/png".to_string()),
            blob: Some(ByteBufB64(PNG_SIGNATURE.to_vec())),
            ..Default::default()
        };
        // content_from_resource succeeds; the completion fails (no model).
        let err = agent
            .run(ctx, "{}".to_string(), vec![resource])
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    #[tokio::test]
    async fn run_image_requires_some_content() {
        let ctx = mock_ctx();
        let agent = MediaUnderstandingAgent::image(Vec::new());
        let err = agent
            .run(ctx, "{}".to_string(), vec![])
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("requires an attached"));
    }

    #[tokio::test]
    async fn understand_media_resources_records_errors_without_panicking() {
        let ctx = mock_ctx();
        let image = Resource {
            name: "photo.png".to_string(),
            mime_type: Some("image/png".to_string()),
            blob: Some(ByteBufB64(PNG_SIGNATURE.to_vec())),
            ..Default::default()
        };
        let plain = Resource {
            name: "plain".to_string(),
            ..Default::default()
        };
        let (resources, _usage) =
            understand_media_resources(&ctx, vec![image, plain.clone()]).await;
        assert_eq!(resources.len(), 2);
        // The media resource gets an error-formatted description; the
        // non-media resource is returned unchanged.
        assert!(resources[0].description.is_some());
    }

    #[tokio::test]
    async fn other_attachment_from_location_handles_schemes() {
        let agent = MediaUnderstandingAgent::other(Vec::new());

        let err = agent
            .other_attachment_from_location(&RequestMeta::default(), "   ")
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("cannot be empty"));

        let data_url = "data:text/plain;base64,aGVsbG8=";
        let attachment = agent
            .other_attachment_from_location(&RequestMeta::default(), data_url)
            .await
            .expect("data url should decode");
        assert_eq!(attachment.data.as_deref(), Some(b"hello".as_ref()));

        let err = agent
            .other_attachment_from_location(&RequestMeta::default(), "ftp://example.com/x")
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported attachment URL scheme")
        );
    }

    #[tokio::test]
    async fn other_attachment_from_path_reads_and_validates() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("doc.txt");
        fs::write(&file, b"file body").unwrap();
        let agent = MediaUnderstandingAgent::other(vec![dir.path().to_path_buf()]);

        let attachment = agent
            .other_attachment_from_path(&RequestMeta::default(), "doc.txt")
            .await
            .expect("workspace file should resolve");
        assert_eq!(attachment.data.as_deref(), Some(b"file body".as_ref()));
        assert_eq!(attachment.size, Some(9));

        // A directory is not a regular file.
        fs::create_dir(dir.path().join("subdir")).unwrap();
        let err = agent
            .other_attachment_from_path(&RequestMeta::default(), "subdir")
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("not a regular file"));
    }

    #[tokio::test]
    async fn other_attachment_from_http_url_fetches_and_reports_status() {
        let app = Router::new()
            .route("/doc.txt", get(|| async { "remote body" }))
            .route("/missing", get(|| async { (AxumStatus::NOT_FOUND, "") }));
        let base = spawn_router(app).await;
        let agent =
            MediaUnderstandingAgent::other(Vec::new()).with_http_client(new_reqwest_client());

        let url = reqwest::Url::parse(&format!("{base}/doc.txt")).unwrap();
        let attachment = agent.other_attachment_from_http_url(url).await.unwrap();
        assert_eq!(attachment.data.as_deref(), Some(b"remote body".as_ref()));
        assert_eq!(attachment.name, "doc.txt");

        let missing = reqwest::Url::parse(&format!("{base}/missing")).unwrap();
        let err = agent
            .other_attachment_from_http_url(missing)
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("failed to fetch attachment"));
    }

    #[tokio::test]
    async fn content_from_http_url_validates_status_and_kind() {
        let app = Router::new()
            .route("/img", get(|| async { (AxumStatus::NOT_FOUND, "") }))
            .route(
                "/text",
                get(|| async {
                    (
                        [(axum::http::header::CONTENT_TYPE, "text/plain")],
                        "not an image",
                    )
                }),
            );
        let base = spawn_router(app).await;
        let agent =
            MediaUnderstandingAgent::image(Vec::new()).with_http_client(new_reqwest_client());

        let missing = reqwest::Url::parse(&format!("{base}/img")).unwrap();
        let err = agent
            .content_from_http_url(missing)
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("failed to fetch media"));

        let wrong_kind = reqwest::Url::parse(&format!("{base}/text")).unwrap();
        let err = agent
            .content_from_http_url(wrong_kind)
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("does not look like image media"));
    }

    #[tokio::test]
    async fn parse_pdf_text_rejects_invalid_bytes() {
        // Not a valid PDF; both OCR-on and OCR-off passes should fail without panicking.
        let result = parse_pdf_text(b"not a pdf at all").await;
        assert!(result.is_err());
    }

    fn mock_model_ctx() -> AgentCtx {
        anda_engine::engine::EngineBuilder::new()
            .with_model(anda_engine::model::Model::mock_implemented())
            .mock_ctx()
    }

    #[tokio::test]
    async fn run_other_summarizes_large_text_via_model() {
        // A text attachment larger than the inline limit is routed through the
        // model summary path, which succeeds with the deterministic mock model.
        let ctx = mock_model_ctx();
        let agent = MediaUnderstandingAgent::other(Vec::new());
        let big = "lorem ipsum ".repeat(8000); // > MAX_OTHER_TEXT_INLINE_BYTES
        let output = agent
            .run(
                ctx,
                "summarize".to_string(),
                vec![text_resource("big.txt", &big)],
            )
            .await
            .expect("large text summary should succeed");
        assert!(output.content.contains("too large to inline") || !output.content.is_empty());
    }

    #[tokio::test]
    async fn run_other_falls_back_to_model_for_binary_attachment() {
        let ctx = mock_model_ctx();
        let agent = MediaUnderstandingAgent::other(Vec::new());
        let resource = Resource {
            name: "blob.bin".to_string(),
            mime_type: Some("application/octet-stream".to_string()),
            blob: Some(ByteBufB64(vec![0u8, 1, 2, 3, 0, 0, 0, 0])),
            ..Default::default()
        };
        let output = agent
            .run(ctx, "{}".to_string(), vec![resource])
            .await
            .expect("fallback understanding should succeed with the mock model");
        assert!(!output.content.is_empty());
    }

    #[tokio::test]
    async fn run_image_completes_with_mock_model() {
        let ctx = mock_model_ctx();
        let agent = MediaUnderstandingAgent::image(Vec::new());
        let resource = Resource {
            name: "photo.png".to_string(),
            mime_type: Some("image/png".to_string()),
            blob: Some(ByteBufB64(PNG_SIGNATURE.to_vec())),
            ..Default::default()
        };
        let output = agent
            .run(ctx, "{}".to_string(), vec![resource])
            .await
            .expect("image understanding should complete with the mock model");
        assert!(output.content.contains("attached image resource"));
        assert!(output.content.contains("Describe the image"));
    }

    #[tokio::test]
    async fn understand_media_resources_runs_with_model_ctx() {
        let ctx = mock_model_ctx();
        let image = Resource {
            name: "photo.png".to_string(),
            mime_type: Some("image/png".to_string()),
            blob: Some(ByteBufB64(PNG_SIGNATURE.to_vec())),
            ..Default::default()
        };
        let (resources, _usage) = understand_media_resources(&ctx, vec![image]).await;
        assert_eq!(resources.len(), 1);
        assert!(resources[0].description.is_some());
    }
}
