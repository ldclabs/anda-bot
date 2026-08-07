use anda_core::{
    AgentContext, AgentOutput, BoxError, ByteBufB64, CompletionFeatures, CompletionRequest,
    ContentPart, RequestMeta, Resource, StateFeatures, inline_data_from_data_url, text_from_bytes,
    text_from_bytes_with_encoding,
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
use anydoc::Format;
use ic_auth_types::Xid;
use std::{borrow::Cow, path::PathBuf};
use unicode_segmentation::UnicodeSegmentation;

use super::{
    catalog::MediaKind,
    source::{
        MAX_MEDIA_FILE_SIZE_BYTES, extension_from_name, mime_type_for_data_or_name,
        mime_type_for_data_or_path, normalize_mime_type, read_limited_response_bytes,
        resolve_media_path, resource_label, response_content_type, strip_data_url_scheme,
    },
};
use crate::util::file_uri::{
    file_uri_for_path, is_file_uri, path_from_file_uri, user_path_string_for_path,
};
use crate::util::http_client::PublicUrlPolicy;

const MAX_OTHER_TEXT_INLINE_BYTES: usize = 256 * 1024;
const MAX_OTHER_TEXT_SUMMARY_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
pub(super) struct AttachmentUnderstanding {
    workspaces: Vec<PathBuf>,
    public_url_policy: PublicUrlPolicy,
}

impl AttachmentUnderstanding {
    pub(super) fn new(workspaces: Vec<PathBuf>, public_url_policy: PublicUrlPolicy) -> Self {
        Self {
            workspaces,
            public_url_policy,
        }
    }

    pub(super) async fn attachment_from_location(
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
                "http" | "https" => self.attachment_from_http_url(url).await,
                "file" => self.attachment_from_path(meta, location).await,
                scheme if location.contains("://") => {
                    Err(format!("unsupported attachment URL scheme: {scheme}").into())
                }
                _ => self.attachment_from_path(meta, location).await,
            };
        }

        self.attachment_from_path(meta, location).await
    }

    pub(super) async fn attachment_from_path(
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

    pub(super) async fn attachment_from_http_url(
        &self,
        url: reqwest::Url,
    ) -> Result<OtherAttachment, BoxError> {
        let response =
            crate::util::http_client::fetch_public_url(url.clone(), self.public_url_policy).await?;
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

    pub(super) async fn understand(
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
            match self.attachment_from_location(ctx.meta(), uri).await {
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
                    return self.fallback(ctx, attachment, question).await;
                }
            }
        }

        let Some(data) = attachment.data.as_deref() else {
            return self.fallback(ctx, attachment, question).await;
        };

        // Content signatures name the container no matter how the attachment
        // was labelled, so they get first refusal. Plain text then takes
        // anything that decodes, which keeps CSV, JSON, and logs verbatim
        // instead of reshaping them; only what is left over is matched against
        // the MIME type and extension, which is what covers the formats anydoc
        // cannot fingerprint.
        if let Some(format) = Format::from_bytes(data) {
            return self
                .understand_document(ctx, attachment, format, question)
                .await;
        }

        if let Some(text) = attachment_text_from_bytes(data, &attachment) {
            return self
                .text_or_summary_output(
                    ctx,
                    &attachment.label,
                    text_language_for_name(&attachment.name),
                    "text attachment",
                    text.as_ref(),
                    question,
                )
                .await;
        }

        if let Some(format) = document_format_from_label(&attachment) {
            return self
                .understand_document(ctx, attachment, format, question)
                .await;
        }

        self.fallback(ctx, attachment, question).await
    }

    async fn understand_document(
        &self,
        ctx: &AgentCtx,
        mut attachment: OtherAttachment,
        format: Format,
        question: &str,
    ) -> Result<AgentOutput, BoxError> {
        let Some(data) = attachment.data.clone() else {
            return self.fallback(ctx, attachment, question).await;
        };

        let source = format!("{} converted by anydoc", format_label(format));
        match convert_document_to_markdown(data, format).await {
            Ok(markdown) if !markdown.trim().is_empty() => {
                self.text_or_summary_output(
                    ctx,
                    &attachment.label,
                    "markdown",
                    &source,
                    &markdown,
                    question,
                )
                .await
            }
            Ok(_) => Ok(AgentOutput {
                content: format!(
                    "anydoc read {} as {} but found no extractable text. The document may be scanned, image-only, or otherwise empty.",
                    attachment.label,
                    format_label(format)
                ),
                ..Default::default()
            }),
            Err(err) => {
                attachment.read_error = Some(format!("anydoc failed: {err}"));
                self.fallback(ctx, attachment, question).await
            }
        }
    }

    async fn text_or_summary_output(
        &self,
        ctx: &AgentCtx,
        label: &str,
        language: &str,
        source: &str,
        text: &str,
        question: &str,
    ) -> Result<AgentOutput, BoxError> {
        if text.len() <= MAX_OTHER_TEXT_INLINE_BYTES {
            return Ok(AgentOutput {
                content: format!(
                    "Detected {source} from {label} ({} bytes). Full text:\n\n{}",
                    text.len(),
                    fenced_text(language, text)
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

    pub(super) async fn fallback(
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
                    instructions: MediaKind::Other.instructions(),
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
}

pub(super) fn other_understanding_tool_names() -> Vec<String> {
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
pub(super) struct FallbackAttachmentFile {
    pub(super) path: Option<PathBuf>,
    pub(super) temporary: bool,
    pub(super) error: Option<String>,
}

pub(super) async fn fallback_other_attachment_file(
    attachment: &OtherAttachment,
) -> FallbackAttachmentFile {
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

pub(super) fn fallback_other_attachment_prompt(
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
pub(super) struct OtherAttachment {
    pub(super) label: String,
    pub(super) name: String,
    pub(super) mime_type: Option<String>,
    pub(super) uri: Option<String>,
    pub(super) size: Option<u64>,
    pub(super) data: Option<Vec<u8>>,
    pub(super) tags: Vec<String>,
    pub(super) read_error: Option<String>,
}

impl OtherAttachment {
    pub(super) fn from_resource(resource: Resource) -> Self {
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

    pub(super) fn to_resource(&self) -> Resource {
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

    pub(super) fn metadata_markdown(&self) -> String {
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

/// The anydoc [`Format`] an attachment's MIME type, name, or URI claims.
///
/// Only consulted once [`Format::from_bytes`] and the plain-text path have both
/// declined the bytes, so this is what picks up signature-less CSV and anything
/// whose container anydoc cannot fingerprint. A wrong label costs one failed
/// conversion, after which the attachment lands in the model fallback anyway.
pub(super) fn document_format_from_label(attachment: &OtherAttachment) -> Option<Format> {
    attachment
        .mime_type
        .as_deref()
        .and_then(normalize_mime_type)
        .as_deref()
        .and_then(format_from_mime_type)
        .or_else(|| extension_from_name(&attachment.name).and_then(Format::from_extension))
        .or_else(|| {
            attachment
                .uri
                .as_deref()
                .and_then(extension_from_name)
                .and_then(Format::from_extension)
        })
}

/// Maps an already-normalized MIME essence onto the parser anydoc should use.
///
/// `Format::from_extension` covers the filename side; this covers attachments
/// that arrive with a MIME type but no usable name, such as data URLs and
/// downloads whose URL path carries no extension.
fn format_from_mime_type(mime_type: &str) -> Option<Format> {
    Some(match mime_type {
        "application/pdf" | "application/x-pdf" => Format::Pdf,
        "application/msword" => Format::Doc,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        | "application/vnd.ms-word.document.macroenabled.12" => Format::Docx,
        "application/vnd.oasis.opendocument.text" => Format::Odt,
        "application/vnd.ms-powerpoint" => Format::Ppt,
        "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        | "application/vnd.openxmlformats-officedocument.presentationml.slideshow"
        | "application/vnd.ms-powerpoint.presentation.macroenabled.12" => Format::Pptx,
        "application/rtf" | "text/rtf" => Format::Rtf,
        "application/epub+zip" => Format::Epub,
        "application/vnd.ms-excel"
        | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        | "application/vnd.ms-excel.sheet.macroenabled.12"
        | "application/vnd.ms-excel.sheet.binary.macroenabled.12" => Format::Excel,
        "application/vnd.oasis.opendocument.spreadsheet" => Format::Ods,
        "application/vnd.oasis.opendocument.presentation" => Format::Odp,
        "text/csv" | "application/csv" => Format::Csv,
        _ => return None,
    })
}

/// Human-readable name for a [`Format`], used in the text handed to the model.
pub(super) fn format_label(format: Format) -> &'static str {
    match format {
        Format::Doc => "Word 97-2003 document",
        Format::Docx => "Word document",
        Format::Odt => "OpenDocument text",
        Format::Pdf => "PDF",
        Format::Ppt => "PowerPoint 97-2003 presentation",
        Format::Pptx => "PowerPoint presentation",
        Format::Rtf => "RTF document",
        Format::Epub => "EPUB book",
        Format::Excel => "Excel workbook",
        Format::Ods => "OpenDocument spreadsheet",
        Format::Odp => "OpenDocument presentation",
        Format::Csv => "CSV table",
    }
}

pub(super) fn attachment_text_from_bytes<'a>(
    data: &'a [u8],
    attachment: &OtherAttachment,
) -> Option<Cow<'a, str>> {
    // No fallback encoding means UTF-8 only, which is what every attachment
    // gets before the legacy check below opens up the platform code page.
    if let Some(text) = text_from_bytes_with_encoding(data, None) {
        return Some(text);
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
    if let Some(text) = text_from_bytes_with_encoding(data, None) {
        return Some(text);
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

pub(super) fn mime_type_allows_legacy_text_fallback(mime_type: &str) -> bool {
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

pub(super) fn is_text_extension(ext: &str) -> bool {
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

/// Converts a document attachment to Markdown with anydoc.
///
/// anydoc is pure Rust and synchronous, so conversion runs on the blocking pool
/// rather than stalling the runtime worker driving this agent. That also
/// contains a panic on hostile input as a task failure instead of tearing down
/// the process.
pub(super) async fn convert_document_to_markdown(
    data: Vec<u8>,
    format: Format,
) -> Result<String, BoxError> {
    tokio::task::spawn_blocking(move || anydoc::to_markdown_bytes(&data, format))
        .await
        .map_err(|err| -> BoxError { format!("document conversion task failed: {err}").into() })?
        .map_err(Into::into)
}

pub(super) fn fenced_text(language: &str, text: &str) -> String {
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

pub(super) fn text_language_for_name(name: &str) -> &'static str {
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

pub(super) fn bounded_text_for_summary(text: &str) -> (String, bool) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use anda_core::ByteBufB64;
    use std::fs;
    use tempfile::tempdir;

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
    fn pure_text_helpers() {
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
    fn document_format_from_label_uses_mime_name_and_uri() {
        assert_eq!(
            document_format_from_label(&test_other_attachment("report.pdf", None, vec![])),
            Some(Format::Pdf)
        );
        assert_eq!(
            document_format_from_label(&test_other_attachment(
                "blob",
                Some("application/pdf; charset=binary"),
                vec![]
            )),
            Some(Format::Pdf)
        );
        assert_eq!(
            document_format_from_label(&test_other_attachment("memo.docx", None, vec![])),
            Some(Format::Docx)
        );
        assert_eq!(
            document_format_from_label(&test_other_attachment(
                "book",
                Some("application/epub+zip"),
                vec![]
            )),
            Some(Format::Epub)
        );

        // The MIME type wins over a name that says otherwise.
        let mut mislabeled = test_other_attachment(
            "sheet.xlsx",
            Some("application/vnd.oasis.opendocument.spreadsheet"),
            vec![],
        );
        assert_eq!(document_format_from_label(&mislabeled), Some(Format::Ods));

        // A nameless download falls through to the URI extension.
        mislabeled = test_other_attachment("attachment", None, vec![]);
        mislabeled.uri = Some("https://example.com/files/deck.pptx".to_string());
        assert_eq!(document_format_from_label(&mislabeled), Some(Format::Pptx));

        assert_eq!(
            document_format_from_label(&test_other_attachment(
                "notes.txt",
                Some("text/plain"),
                vec![]
            )),
            None
        );
    }

    #[test]
    fn format_label_names_every_format() {
        for format in [
            Format::Doc,
            Format::Docx,
            Format::Odt,
            Format::Pdf,
            Format::Ppt,
            Format::Pptx,
            Format::Rtf,
            Format::Epub,
            Format::Excel,
            Format::Ods,
            Format::Odp,
            Format::Csv,
        ] {
            assert!(!format_label(format).is_empty());
        }
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

    #[tokio::test]
    async fn attachment_from_location_handles_schemes() {
        let understanding = AttachmentUnderstanding::new(Vec::new(), PublicUrlPolicy::PublicOnly);

        let err = understanding
            .attachment_from_location(&RequestMeta::default(), "   ")
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("cannot be empty"));

        let data_url = "data:text/plain;base64,aGVsbG8=";
        let attachment = understanding
            .attachment_from_location(&RequestMeta::default(), data_url)
            .await
            .expect("data url should decode");
        assert_eq!(attachment.data.as_deref(), Some(b"hello".as_ref()));

        let err = understanding
            .attachment_from_location(&RequestMeta::default(), "ftp://example.com/x")
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported attachment URL scheme")
        );
    }

    #[tokio::test]
    async fn attachment_from_path_reads_and_validates() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("doc.txt");
        fs::write(&file, b"file body").unwrap();
        let understanding = AttachmentUnderstanding::new(
            vec![dir.path().to_path_buf()],
            PublicUrlPolicy::PublicOnly,
        );

        let attachment = understanding
            .attachment_from_path(&RequestMeta::default(), "doc.txt")
            .await
            .expect("workspace file should resolve");
        assert_eq!(attachment.data.as_deref(), Some(b"file body".as_ref()));
        assert_eq!(attachment.size, Some(9));

        fs::create_dir(dir.path().join("subdir")).unwrap();
        let err = understanding
            .attachment_from_path(&RequestMeta::default(), "subdir")
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("not a regular file"));
    }

    #[tokio::test]
    async fn convert_document_to_markdown_rejects_invalid_bytes() {
        // Bytes that claim to be a PDF but are not: anydoc must report an error
        // rather than panic out of the blocking task.
        let result = convert_document_to_markdown(b"not a pdf at all".to_vec(), Format::Pdf).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn convert_document_to_markdown_renders_a_signature_less_format() {
        // CSV carries no signature, so it only converts when the label names it
        // — the path `document_format_from_label` exists to reach.
        let markdown = convert_document_to_markdown(b"name,qty\npanda,2\n".to_vec(), Format::Csv)
            .await
            .expect("csv should convert to markdown");

        assert!(markdown.contains("name"));
        assert!(markdown.contains("panda"));
        assert!(markdown.contains('|'), "expected a GFM table: {markdown}");
    }
}
