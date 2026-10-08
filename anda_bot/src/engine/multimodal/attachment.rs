use anda_core::{
    AgentContext, AgentOutput, BoxError, ByteBufB64, CompletionFeatures, CompletionRequest,
    ContentPart, RequestMeta, Resource, StateFeatures, text_from_bytes,
    text_from_bytes_with_encoding,
};
use anda_engine::{
    context::{AgentCtx, TOOLS_SEARCH_NAME, TOOLS_SELECT_NAME},
    extension::{
        fs::{ReadFileTool, SearchFileTool},
        shell::ShellTool,
        skill::{SkillManager, SkillsListTool, SkillsReadTool},
    },
    grapheme_safe_cutoff,
    subagent::SubAgentManager,
};
use anydoc::Format;
use ic_auth_types::Xid;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};
use tempfile::TempPath;
use unicode_segmentation::UnicodeSegmentation;

use super::{
    catalog::{MediaKind, extension_from_name},
    source::{
        DEFAULT_URL_FILE_NAME, LoadedSource, SourceLoader, is_data_url, normalize_mime_type,
        resource_label,
    },
};
use crate::util::file_uri::{
    file_uri_for_path, is_file_uri, path_from_file_uri, user_path_string_for_path,
};
use crate::util::fs::sanitize_path_component;

/// Text up to this size goes back to the main agent verbatim, where it stays
/// in the conversation; larger text is summarized.
const MAX_OTHER_TEXT_INLINE_BYTES: usize = 64 * 1024;
/// Largest excerpt a summary reads, so CJK text, at roughly a token per
/// character, fits a 400K-token context window.
const MAX_OTHER_TEXT_SUMMARY_BYTES: usize = 512 * 1024;

/// Understands one non-media attachment: documents and text locally, anything
/// else through the active model with tools.
pub(super) async fn understand_attachment(
    ctx: &AgentCtx,
    sources: &SourceLoader,
    mut attachment: OtherAttachment,
    question: &str,
) -> Result<AgentOutput, BoxError> {
    if attachment.data.is_none()
        && let Some(uri) = attachment
            .uri
            .as_deref()
            .map(str::trim)
            .filter(|uri| !uri.is_empty())
    {
        // A denied location must not be handed to shell as a parser fallback,
        // which would undo the workspace/URL boundary.
        let mut loaded = OtherAttachment::from(sources.load(ctx.meta(), uri).await?);
        if loaded.name == DEFAULT_URL_FILE_NAME {
            loaded.name = std::mem::take(&mut attachment.name);
        }
        loaded.tags = std::mem::take(&mut attachment.tags);
        attachment = loaded;
    }

    let Some(data) = attachment.data.as_deref() else {
        // Without bytes or a location, a model with tools would have nothing
        // to inspect either.
        return Ok(AgentOutput {
            content: format!(
                "{} has no content to read.\n\nAttachment metadata:\n{}",
                attachment.label,
                attachment.metadata_markdown()
            ),
            ..Default::default()
        });
    };

    // Content signatures name the container no matter how the attachment
    // was labelled, so they get first refusal. Plain text then takes
    // anything that decodes, which keeps CSV, JSON, and logs verbatim
    // instead of reshaping them; only what is left over is matched against
    // the MIME type and extension, which is what covers the formats anydoc
    // cannot fingerprint.
    if let Some(format) = Format::from_bytes(data) {
        return understand_document(ctx, sources, data.to_vec(), attachment, format, question)
            .await;
    }

    if let Some(text) = attachment_text_from_bytes(data, &attachment) {
        return text_or_summary_output(
            ctx,
            &attachment.label,
            text_language_for_name(&attachment.name),
            "text attachment",
            &text,
            question,
        )
        .await;
    }

    if let Some(format) = document_format_from_label(&attachment) {
        return understand_document(ctx, sources, data.to_vec(), attachment, format, question)
            .await;
    }

    fallback(ctx, sources, attachment, question).await
}

async fn understand_document(
    ctx: &AgentCtx,
    sources: &SourceLoader,
    data: Vec<u8>,
    mut attachment: OtherAttachment,
    format: Format,
    question: &str,
) -> Result<AgentOutput, BoxError> {
    match convert_document_to_markdown(data, format).await {
        Ok(markdown) if !markdown.trim().is_empty() => {
            let source = format!("{} converted by anydoc", format_label(format));
            text_or_summary_output(
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
            fallback(ctx, sources, attachment, question).await
        }
    }
}

async fn text_or_summary_output(
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

/// Hands an attachment no built-in parser could read to the active model, with
/// tools to find a skill or inspect a local copy.
async fn fallback(
    ctx: &AgentCtx,
    sources: &SourceLoader,
    mut attachment: OtherAttachment,
    question: &str,
) -> Result<AgentOutput, BoxError> {
    // Dropping a temporary copy deletes it, also when the run is cancelled.
    let local_file = local_attachment_file(ctx.meta(), sources, &mut attachment).await;
    let metadata = attachment.metadata_markdown();
    let prompt = fallback_prompt(question, &metadata, &local_file);
    let mut resource = attachment.into_resource();
    if let Some(path) = local_file.path()
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
                model: Some(crate::engine::ACTIVE_MODEL_LABEL.to_string()),
                tools,
                ..Default::default()
            },
            vec![resource],
        )
        .await?;

    if output.content.trim().is_empty() {
        output.content = format!(
            "No automatic parser produced output for this attachment.\n\nAttachment metadata:\n{metadata}"
        );
    }

    Ok(output)
}

pub(super) fn other_understanding_tool_names() -> Vec<String> {
    vec![
        TOOLS_SEARCH_NAME.to_string(),
        TOOLS_SELECT_NAME.to_string(),
        SkillManager::NAME.to_string(),
        SkillsListTool::NAME.to_string(),
        SkillsReadTool::NAME.to_string(),
        SubAgentManager::NAME.to_string(),
        ShellTool::NAME.to_string(),
        crate::engine::agent::SHELL_SESSION_NAME.to_string(),
        ReadFileTool::NAME.to_string(),
        SearchFileTool::NAME.to_string(),
    ]
}

/// The local file the fallback's shell and file tools read.
enum LocalFile {
    /// The attachment's own file, inside the caller's workspaces.
    Workspace(PathBuf),
    /// A copy of the attachment bytes, deleted when this value drops.
    Temporary(TempPath),
    /// No local file, and why.
    Unavailable(String),
}

impl LocalFile {
    fn path(&self) -> Option<&Path> {
        match self {
            Self::Workspace(path) => Some(path),
            Self::Temporary(path) => Some(path),
            Self::Unavailable(_) => None,
        }
    }
}

/// Finds or writes the local file for the fallback. An attached blob is usable,
/// but an accompanying path is not a grant: one outside the caller's
/// workspaces is dropped from the attachment.
async fn local_attachment_file(
    meta: &RequestMeta,
    sources: &SourceLoader,
    attachment: &mut OtherAttachment,
) -> LocalFile {
    if let Some(uri) = attachment
        .uri
        .as_deref()
        .filter(|uri| is_file_uri(uri) || reqwest::Url::parse(uri).is_err())
    {
        if let Ok(path) = sources.resolve_path(meta, uri).await
            && tokio::fs::metadata(&path)
                .await
                .is_ok_and(|metadata| metadata.is_file())
        {
            return LocalFile::Workspace(path);
        }
        attachment.uri = None;
    }

    let Some(data) = attachment.data.as_deref() else {
        return LocalFile::Unavailable("the attachment has no inline bytes".to_string());
    };
    match write_fallback_attachment_file(attachment, data).await {
        Ok(path) => LocalFile::Temporary(path),
        Err(err) => LocalFile::Unavailable(format!("writing a temporary copy failed: {err}")),
    }
}

async fn write_fallback_attachment_file(
    attachment: &OtherAttachment,
    data: &[u8],
) -> Result<TempPath, BoxError> {
    let dir = std::env::temp_dir().join("anda-bot-attachments");
    tokio::fs::create_dir_all(&dir).await?;
    let file_name = fallback_attachment_file_name(attachment);
    let path = TempPath::try_from_path(dir.join(format!("{}-{file_name}", Xid::new())))?;
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

    sanitize_path_component(candidate.as_ref(), "attachment.bin")
}

fn fallback_prompt(question: &str, metadata: &str, local_file: &LocalFile) -> String {
    let local_access = match local_file {
        LocalFile::Workspace(path) => format!(
            "- Metadata includes an existing local file path in an authorized workspace: {}",
            user_path_string_for_path(path)
        ),
        LocalFile::Temporary(path) => format!(
            "- A temporary local copy has been written for shell/file tools: {}",
            user_path_string_for_path(path)
        ),
        LocalFile::Unavailable(reason) => format!(
            "- No local file is available for shell/file tools ({reason}). Network-capable tools may refetch an http(s) URL from the metadata."
        ),
    };

    format!(
        "Understand this non-image/audio/video attachment for the main agent.\n\nInput boundary:\n- This fallback is used only after built-in text/PDF extraction and direct model-readable media handling were not sufficient.\n- Do not assume the model can directly read the attachment bytes from the prompt; inspect the file path or URL below with tools.\n{local_access}\n\nWorkflow:\n1. Search available tools/skills for a parser that matches the MIME type, extension, or file family; use an installed skill/subagent if one is suitable.\n2. Use shell or read-only file inspection against the provided local path when available. Prefer safe commands that extract metadata/text over mutating the file.\n3. If there is no local path but metadata includes a URL, use network-capable tools or shell commands to refetch or research a practical extraction method, then report the best next action.\n4. If extraction is impossible, explain what was tried or what capability is missing.\n\nDo not invent attachment contents.\n\nCaller question or focus:\n{question}\n\nAttachment metadata:\n{metadata}"
    )
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

    fn into_resource(self) -> Resource {
        Resource {
            name: self.name,
            mime_type: self.mime_type,
            uri: self.uri,
            size: self.size,
            blob: self.data.map(ByteBufB64),
            tags: self.tags,
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

impl From<LoadedSource> for OtherAttachment {
    fn from(source: LoadedSource) -> Self {
        Self {
            label: source.label,
            name: source.name,
            mime_type: Some(source.mime_type),
            uri: source.uri,
            size: Some(source.data.len() as u64),
            data: Some(source.data),
            tags: Vec::new(),
            read_error: None,
        }
    }
}

fn display_attachment_uri(uri: &str) -> String {
    let trimmed = uri.trim();
    if is_data_url(trimmed) {
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
fn document_format_from_label(attachment: &OtherAttachment) -> Option<Format> {
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
fn format_label(format: Format) -> &'static str {
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

fn attachment_text_from_bytes<'a>(
    data: &'a [u8],
    attachment: &OtherAttachment,
) -> Option<Cow<'a, str>> {
    decode_attachment_text(data, attachment, text_from_bytes)
}

/// Decodes UTF-8, then tries `legacy` (the platform code page) only for an
/// attachment labelled as text.
fn decode_attachment_text<'a>(
    data: &'a [u8],
    attachment: &OtherAttachment,
    legacy: impl FnOnce(&'a [u8]) -> Option<Cow<'a, str>>,
) -> Option<Cow<'a, str>> {
    // No fallback encoding means UTF-8 only.
    text_from_bytes_with_encoding(data, None).or_else(|| {
        if attachment_allows_legacy_text_fallback(attachment) {
            legacy(data)
        } else {
            None
        }
    })
}

fn attachment_allows_legacy_text_fallback(attachment: &OtherAttachment) -> bool {
    attachment
        .mime_type
        .as_deref()
        .and_then(normalize_mime_type)
        .is_some_and(|mime_type| mime_type_allows_legacy_text_fallback(&mime_type))
        || extension_from_name(&attachment.name).is_some_and(is_text_extension)
        || attachment
            .tags
            .iter()
            .any(|tag| is_text_extension(tag.trim().trim_start_matches('.')))
}

/// Whether a normalized MIME essence names a text format.
fn mime_type_allows_legacy_text_fallback(essence: &str) -> bool {
    essence.starts_with("text/")
        || essence.ends_with("+json")
        || essence.ends_with("+xml")
        || matches!(
            essence,
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

/// Converts a document attachment to Markdown with anydoc.
///
/// anydoc is pure Rust and synchronous, so conversion runs on the blocking pool
/// rather than stalling the runtime worker driving this agent. That also
/// contains a panic on hostile input as a task failure instead of tearing down
/// the process.
async fn convert_document_to_markdown(data: Vec<u8>, format: Format) -> Result<String, BoxError> {
    tokio::task::spawn_blocking(move || anydoc::to_markdown_bytes(&data, format))
        .await
        .map_err(|err| -> BoxError { format!("document conversion task failed: {err}").into() })?
        .map_err(Into::into)
}

fn fenced_text(language: &str, text: &str) -> String {
    let longest_backtick_run = text
        .split(|ch| ch != '`')
        .map(str::len)
        .max()
        .unwrap_or_default();
    let fence = "`".repeat(longest_backtick_run.max(2) + 1);
    if language.is_empty() {
        format!("{fence}\n{text}\n{fence}")
    } else {
        format!("{fence}{language}\n{text}\n{fence}")
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::http_client::PublicUrlPolicy;
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

    fn sources(workspaces: Vec<PathBuf>) -> SourceLoader {
        SourceLoader::new(workspaces, PublicUrlPolicy::PublicOnly)
    }

    fn decode_with_gbk<'a>(data: &'a [u8], attachment: &OtherAttachment) -> Option<Cow<'a, str>> {
        decode_attachment_text(data, attachment, |data| {
            text_from_bytes_with_encoding(data, anda_core::windows_code_page_encoding(936))
        })
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
        assert_eq!(decode_with_gbk(&gbk, &attachment).as_deref(), Some("中文"));

        // A text tag is enough, and code extensions count as text.
        let tagged = test_other_attachment("blob", None, vec![" .MD"]);
        assert_eq!(decode_with_gbk(&gbk, &tagged).as_deref(), Some("中文"));
        let source = test_other_attachment("main.rs", None, vec![]);
        assert_eq!(decode_with_gbk(&gbk, &source).as_deref(), Some("中文"));
    }

    #[test]
    fn text_attachment_detection_rejects_legacy_fallback_for_binary_mime() {
        let gbk = [0xD6, 0xD0, 0xCE, 0xC4];
        let attachment = test_other_attachment("image.jpg", Some("image/jpeg"), vec![]);

        assert!(decode_with_gbk(&gbk, &attachment).is_none());
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

        assert_eq!(fenced_text("", "plain"), "```\nplain\n```");
        assert!(fenced_text("", "a `````b").starts_with("``````\n"));
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

        let md = attachment.metadata_markdown();
        assert!(md.contains("- name: notes.txt"));
        assert!(md.contains("- mime_type: text/plain"));
        assert!(md.contains("- uri: file:///tmp/notes.txt"));
        assert!(md.contains("- local_path:"));
        assert!(md.contains("- size_bytes: 4"));
        assert!(md.contains("- tags: text"));
        assert!(md.contains("- inline_blob_available: true"));

        let mut with_error = attachment.clone();
        with_error.read_error = Some("boom".to_string());
        assert!(
            with_error
                .metadata_markdown()
                .contains("- read_error: boom")
        );

        let back = attachment.into_resource();
        assert_eq!(back.name, "notes.txt");
        assert_eq!(back.size, Some(4));
        assert_eq!(back.blob.map(|blob| blob.0), Some(b"data".to_vec()));
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
    async fn local_file_is_a_temporary_copy_deleted_on_drop() {
        let mut attachment =
            test_other_attachment("blob.bin", Some("application/octet-stream"), vec![]);
        attachment.data = Some(vec![0u8, 1, 2, 3]);

        let local =
            local_attachment_file(&RequestMeta::default(), &sources(vec![]), &mut attachment).await;

        let LocalFile::Temporary(path) = &local else {
            panic!("expected a temporary copy");
        };
        let path = path.to_path_buf();
        assert!(
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("blob.bin"))
        );
        assert_eq!(fs::read(&path).unwrap(), vec![0u8, 1, 2, 3]);
        drop(local);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn local_file_uses_workspace_paths_only() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let inside = workspace.join("deck.key");
        fs::write(&inside, b"keynote").unwrap();
        let outside = dir.path().join("secret.key");
        fs::write(&outside, b"secret").unwrap();
        let sources = sources(vec![workspace]);

        let mut own = test_other_attachment("deck.key", None, vec![]);
        own.uri = Some(file_uri_for_path(&inside).unwrap());
        own.data = Some(b"keynote".to_vec());
        let local = local_attachment_file(&RequestMeta::default(), &sources, &mut own).await;
        assert!(
            matches!(&local, LocalFile::Workspace(path) if path == &inside.canonicalize().unwrap())
        );
        assert!(own.uri.is_some());

        // A path outside the workspaces is not a grant: it is dropped, and the
        // attached bytes are copied instead.
        let mut foreign = test_other_attachment("secret.key", None, vec![]);
        foreign.uri = Some(outside.to_string_lossy().into_owned());
        foreign.data = Some(b"secret".to_vec());
        let local = local_attachment_file(&RequestMeta::default(), &sources, &mut foreign).await;
        assert!(matches!(local, LocalFile::Temporary(_)));
        assert_eq!(foreign.uri, None);

        let mut empty = test_other_attachment("nothing.bin", None, vec![]);
        let local = local_attachment_file(&RequestMeta::default(), &sources, &mut empty).await;
        assert!(matches!(local, LocalFile::Unavailable(_)));
    }

    #[test]
    fn fallback_prompt_describes_attachment_access_boundary() {
        // A name nothing else uses, since dropping the path deletes the file.
        let temp =
            TempPath::try_from_path(std::env::temp_dir().join(format!("{}-blob.bin", Xid::new())))
                .unwrap();
        let inline_prompt =
            fallback_prompt("inspect", "- label: blob.bin", &LocalFile::Temporary(temp));
        assert!(inline_prompt.contains("Do not assume the model can directly read"));
        assert!(inline_prompt.contains("temporary local copy"));
        assert!(inline_prompt.contains("blob.bin"));

        let file_prompt = fallback_prompt(
            "inspect",
            "- label: docx.bin",
            &LocalFile::Workspace(PathBuf::from("/tmp/docx.bin")),
        );
        assert!(file_prompt.contains("existing local file path"));

        let remote_prompt = fallback_prompt(
            "inspect",
            "- uri: https://example.com/docx.bin",
            &LocalFile::Unavailable("writing a temporary copy failed: disk full".to_string()),
        );
        assert!(remote_prompt.contains("No local file is available"));
        assert!(remote_prompt.contains("disk full"));
        assert!(remote_prompt.contains("http(s) URL"));
        assert!(remote_prompt.contains("https://example.com/docx.bin"));
    }

    #[tokio::test]
    async fn fallback_removes_its_temporary_copy() {
        let copy_left = |name: &str| {
            fs::read_dir(std::env::temp_dir().join("anda-bot-attachments"))
                .map(|entries| {
                    entries
                        .flatten()
                        .any(|entry| entry.file_name().to_string_lossy().ends_with(name))
                })
                .unwrap_or(false)
        };
        let attachment = |name: &str| {
            let mut attachment =
                test_other_attachment(name, Some("application/octet-stream"), vec![]);
            attachment.data = Some(vec![0u8, 1, 2, 3]);
            attachment
        };

        // A run that succeeds and one that fails (no model) both clean up.
        let with_model = anda_engine::engine::EngineBuilder::new()
            .with_model(anda_engine::model::Model::mock_implemented())
            .mock_ctx();
        let name = format!("cleanup-{}.bin", Xid::new());
        fallback(&with_model, &sources(vec![]), attachment(&name), "inspect")
            .await
            .expect("the mock model should answer");
        assert!(!copy_left(&name), "the copy should be deleted after a run");

        let without_model = anda_engine::engine::EngineBuilder::new().mock_ctx();
        let name = format!("cleanup-{}.bin", Xid::new());
        fallback(
            &without_model,
            &sources(vec![]),
            attachment(&name),
            "inspect",
        )
        .await
        .map(|_| ())
        .unwrap_err();
        assert!(
            !copy_left(&name),
            "the copy should be deleted after a failure"
        );
    }

    #[tokio::test]
    async fn an_attachment_without_bytes_or_location_skips_the_model() {
        // No model is configured, so reaching one would fail the call.
        let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx();
        let attachment = test_other_attachment("ghost.bin", Some("application/zip"), vec![]);

        let output = understand_attachment(&ctx, &sources(vec![]), attachment, "inspect")
            .await
            .expect("an empty attachment should be reported, not sent to a model");

        assert!(output.content.contains("ghost.bin has no content to read"));
        assert!(output.content.contains("- mime_type: application/zip"));
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
