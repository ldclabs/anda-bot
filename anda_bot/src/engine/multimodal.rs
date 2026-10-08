mod attachment;
mod catalog;
mod source;

use anda_core::{
    Agent, AgentOutput, BoxError, CompletionFeatures, CompletionRequest, ContentPart,
    FunctionDefinition, Resource, StateFeatures, ToolGroupInfo,
};
use anda_engine::{context::AgentCtx, model::Model};
use serde::Deserialize;
use serde_json::json;
use std::{path::PathBuf, sync::Arc};

use super::resources::ResourceStore;
use crate::util::http_client::PublicUrlPolicy;
use attachment::{OtherAttachment, other_understanding_tool_names, understand_attachment};
pub use catalog::{
    AUDIO_UNDERSTANDING_AGENT_NAME, IMAGE_UNDERSTANDING_AGENT_NAME, MediaKind,
    OTHER_UNDERSTANDING_AGENT_NAME, VIDEO_UNDERSTANDING_AGENT_NAME,
};
use source::{SourceLoader, content_from_resource, media_content, resource_label};

#[derive(Debug, Default, Deserialize)]
struct MediaUnderstandingArgs {
    #[serde(default, alias = "_id")]
    resource_id: Option<u64>,
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
    /// Parses the tool arguments. A JSON object must match the schema, so a
    /// mistyped field comes back as an error the model can correct instead of
    /// turning the whole object into the question. Other text is the question.
    fn from_prompt(prompt: &str) -> Result<Self, BoxError> {
        let trimmed = prompt.trim();
        if trimmed.starts_with('{') {
            return serde_json::from_str(trimmed)
                .map_err(|err| format!("invalid arguments: {err}").into());
        }

        Ok(Self {
            question: (!trimmed.is_empty()).then(|| trimmed.to_string()),
            ..Self::default()
        })
    }

    fn resource_id(&self) -> Option<u64> {
        self.resource_id.filter(|id| *id > 0)
    }

    fn question(&self, kind: MediaKind) -> &str {
        self.question
            .as_deref()
            .map(str::trim)
            .filter(|question| !question.is_empty())
            .unwrap_or(kind.default_question())
    }

    /// The `url` and `path` values, trimmed and without blanks. Models often
    /// put one location in both, which must not load it twice.
    fn locations(&self) -> Vec<&str> {
        let mut locations = Vec::with_capacity(2);
        for location in [&self.url, &self.path]
            .into_iter()
            .flatten()
            .map(|location| location.trim())
        {
            if !location.is_empty() && !locations.contains(&location) {
                locations.push(location);
            }
        }
        locations
    }
}

#[derive(Clone)]
pub struct MediaUnderstandingAgent {
    kind: MediaKind,
    workspaces: Vec<PathBuf>,
    public_url_policy: PublicUrlPolicy,
    cli_workspaces: Option<super::shell_runtime::CliWorkspaceGrants>,
    resource_store: Option<Arc<ResourceStore>>,
}

impl MediaUnderstandingAgent {
    fn new(kind: MediaKind, workspaces: Vec<PathBuf>) -> Self {
        Self {
            kind,
            workspaces,
            public_url_policy: PublicUrlPolicy::PublicOnly,
            cli_workspaces: None,
            resource_store: None,
        }
    }

    pub fn image(workspaces: Vec<PathBuf>) -> Self {
        Self::new(MediaKind::Image, workspaces)
    }

    pub fn audio(workspaces: Vec<PathBuf>) -> Self {
        Self::new(MediaKind::Audio, workspaces)
    }

    pub fn video(workspaces: Vec<PathBuf>) -> Self {
        Self::new(MediaKind::Video, workspaces)
    }

    pub fn other(workspaces: Vec<PathBuf>) -> Self {
        Self::new(MediaKind::Other, workspaces)
    }

    pub(super) fn with_cli_workspaces(
        mut self,
        grants: super::shell_runtime::CliWorkspaceGrants,
    ) -> Self {
        self.cli_workspaces = Some(grants);
        self
    }

    /// Lets callers inspect message attachments by their persisted resource id.
    pub(super) fn with_resource_store(mut self, store: Arc<ResourceStore>) -> Self {
        self.resource_store = Some(store);
        self
    }

    #[cfg(test)]
    pub fn allow_private_urls_for_tests(mut self) -> Self {
        self.public_url_policy = PublicUrlPolicy::AllowPrivateForTests;
        self
    }

    pub fn model_label(&self) -> &'static str {
        self.kind.model_label()
    }

    /// Reads from the configured workspaces plus those the caller registered.
    fn sources(&self, ctx: &AgentCtx) -> SourceLoader {
        let mut workspaces = self.workspaces.clone();
        if let Some(grants) = &self.cli_workspaces {
            workspaces.extend(grants.paths_for(ctx.caller()));
        }
        SourceLoader::new(workspaces, self.public_url_policy)
    }

    /// Loads a message attachment the caller owns, rejecting one that belongs
    /// to a different understanding agent so the model can retry the right one.
    async fn resource_by_id(&self, ctx: &AgentCtx, id: u64) -> Result<Resource, BoxError> {
        let store = self
            .resource_store
            .as_ref()
            .ok_or("resource_id is not supported here; pass a path or url")?;
        let resource = store.get_resource_for(id, ctx.caller()).await?;
        let kind = MediaKind::from_resource(&resource);
        if kind != self.kind {
            return Err(format!(
                "resource {id} ({}) is not {} media; use {} instead",
                resource_label(&resource),
                self.kind.noun(),
                kind.agent_name()
            )
            .into());
        }
        Ok(resource)
    }

    /// Sends image, audio or video inputs to the model with this kind's label.
    async fn understand_media(
        &self,
        ctx: &AgentCtx,
        sources: &SourceLoader,
        question: &str,
        resources: Vec<Resource>,
        locations: &[&str],
    ) -> Result<AgentOutput, BoxError> {
        let prompt = self.completion_prompt(question, resources.len(), locations.len());
        let mut content = Vec::with_capacity(resources.len() + locations.len());
        for resource in resources {
            content.push(content_from_resource(self.kind, resource)?);
        }
        for location in locations {
            content.push(media_content(
                self.kind,
                sources.load(ctx.meta(), location).await?,
            )?);
        }

        ctx.completion(
            CompletionRequest {
                instructions: self.kind.instructions(),
                prompt,
                content,
                ..Default::default()
            },
            Vec::new(),
        )
        .await
    }

    /// Understands each attachment on its own, so one that fails leaves the
    /// others' results intact.
    async fn understand_attachments(
        &self,
        ctx: &AgentCtx,
        sources: &SourceLoader,
        question: &str,
        resources: Vec<Resource>,
        locations: &[&str],
    ) -> Result<AgentOutput, BoxError> {
        let mut attachments: Vec<OtherAttachment> = resources
            .into_iter()
            .map(OtherAttachment::from_resource)
            .collect();
        for location in locations {
            attachments.push(sources.load(ctx.meta(), location).await?.into());
        }

        let mut output = AgentOutput::default();
        let mut sections = Vec::with_capacity(attachments.len());
        for attachment in attachments {
            let label = attachment.label.clone();
            match understand_attachment(ctx, sources, attachment, question).await {
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

    fn completion_prompt(&self, question: &str, resources: usize, locations: usize) -> String {
        let noun = self.kind.noun();
        let attached = match resources {
            0 => None,
            1 => Some(format!("the attached {noun} resource")),
            n => Some(format!("the {n} attached {noun} resources")),
        };
        let located = match locations {
            0 => None,
            1 => Some("the media file at the supplied path or URL".to_string()),
            n => Some(format!("the {n} media files at the supplied paths or URLs")),
        };
        let target = attached
            .into_iter()
            .chain(located)
            .collect::<Vec<_>>()
            .join(" and ");

        format!("Understand {target}. Caller question or focus:\n{question}")
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
                "description": "Understand one message attachment by its resource `_id`, read a local media file path from the configured workspace, or fetch media from an http/https/data URL. Do not include a `prompt` property; use `question` for optional guidance so the media location is preserved.",
                "properties": {
                    "resource_id": {
                        "type": ["integer", "null"],
                        "description": "The `_id` of a message attachment (a `Resource` reference in the conversation). Omit when using a path or URL."
                    },
                    "path": {
                        "type": ["string", "null"],
                        "description": "Optional local media file path. Relative paths resolve from the current configured workspace; absolute paths must be inside an allowed workspace. This also accepts file/http/https/data URLs for compatibility. Omit when passing a resource_id."
                    },
                    "url": {
                        "type": ["string", "null"],
                        "description": "Optional media URL. Supports http, https, and data URLs. Omit when using a resource_id or local path."
                    },
                    "question": {
                        "type": ["string", "null"],
                        "description": "What to find out, focused on the user's request. Without one, the media is described generally."
                    }
                },
                "required": ["resource_id", "path", "url", "question"],
                "additionalProperties": false
            }),
            strict: Some(true),
        }
    }

    fn group(&self) -> Option<ToolGroupInfo> {
        Some(catalog::media_understanding_tool_group_info())
    }

    fn supported_resource_tags(&self) -> Vec<String> {
        self.kind.tags().iter().map(|tag| tag.to_string()).collect()
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
        mut resources: Vec<Resource>,
    ) -> Result<AgentOutput, BoxError> {
        let args = MediaUnderstandingArgs::from_prompt(&prompt)?;
        if let Some(id) = args.resource_id() {
            resources.push(self.resource_by_id(&ctx, id).await?);
        }
        let locations = args.locations();
        if resources.is_empty() && locations.is_empty() {
            return Err(format!(
                "{} requires a resource_id, an attached resource, a workspace file path, or a URL",
                self.kind.agent_name()
            )
            .into());
        }

        let sources = self.sources(&ctx);
        let question = args.question(self.kind);
        if self.kind == MediaKind::Other {
            self.understand_attachments(&ctx, &sources, question, resources, &locations)
                .await
        } else {
            self.understand_media(&ctx, &sources, question, resources, &locations)
                .await
        }
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
    MediaKind::ALL
        .iter()
        .flat_map(|kind| kind.tags())
        .map(|tag| tag.to_string())
        .collect()
}

/// Image formats every built-in provider accepts inline. Others stay references, because a
/// rejected attachment would fail the whole turn rather than one tool call.
const INLINE_IMAGE_MIME_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];
/// Largest image sent inline, within the providers' per-image limits.
const MAX_INLINE_IMAGE_BYTES: usize = 5 * 1024 * 1024;
/// Inline bytes allowed per message, so several images stay under request size limits.
const MAX_INLINE_MESSAGE_BYTES: usize = 10 * 1024 * 1024;

/// The images among `attachments` that `model` reads itself, aligned with `attachments`.
///
/// A model labeled `image` gets PNG, JPEG, GIF and WebP attachments directly. The session runner
/// keeps those bytes for the current task only, so each one travels next to its `Resource`
/// reference (see [`attachment_content`]); everything else is inspected through an
/// understanding agent.
pub fn inline_images(model: Option<&Model>, attachments: &[Resource]) -> Vec<Option<ContentPart>> {
    let reads_images = model.is_some_and(|model| {
        model
            .labels
            .iter()
            .any(|label| label.eq_ignore_ascii_case(MediaKind::Image.model_label()))
    });
    let mut budget = MAX_INLINE_MESSAGE_BYTES;
    attachments
        .iter()
        .map(|resource| {
            let blob = resource.blob.as_ref().filter(|blob| {
                reads_images && !blob.is_empty() && blob.len() <= MAX_INLINE_IMAGE_BYTES.min(budget)
            })?;
            let mime_type = source::mime_type_for_data_or_name(
                blob,
                &resource.name,
                resource.mime_type.as_deref(),
            );
            if !INLINE_IMAGE_MIME_TYPES.contains(&mime_type.as_str()) {
                return None;
            }
            budget -= blob.len();
            Some(ContentPart::InlineData {
                mime_type,
                data: blob.clone(),
            })
        })
        .collect()
}

/// Message content for stored attachments: each `Resource` reference, followed by the bytes
/// [`inline_images`] chose for it.
pub fn attachment_content(
    stored: Vec<Resource>,
    inline: Vec<Option<ContentPart>>,
) -> Vec<ContentPart> {
    let mut content = Vec::with_capacity(stored.len() * 2);
    let mut inline = inline.into_iter();
    for resource in stored {
        content.push(ContentPart::any_from("Resource", resource));
        content.extend(inline.next().flatten());
    }
    content
}

/// Understanding agents a session should load up front so the model can
/// inspect these attachments without discovering the tools first.
pub fn media_agent_names_for(resources: &[Resource]) -> Vec<String> {
    let mut kinds = Vec::new();
    for kind in resources.iter().map(MediaKind::from_resource) {
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    kinds
        .into_iter()
        .map(|kind| kind.agent_name().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::json_schema::assert_openai_strict_parameters;
    use anda_core::ByteBufB64;
    use axum::{Router, http::StatusCode as AxumStatus, routing::get};

    const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];

    fn all_agents() -> [MediaUnderstandingAgent; 4] {
        [
            MediaUnderstandingAgent::image(Vec::new()),
            MediaUnderstandingAgent::audio(Vec::new()),
            MediaUnderstandingAgent::video(Vec::new()),
            MediaUnderstandingAgent::other(Vec::new()),
        ]
    }

    fn parse(prompt: &str) -> MediaUnderstandingArgs {
        MediaUnderstandingArgs::from_prompt(prompt).expect("arguments should parse")
    }

    #[test]
    fn media_understanding_schema_is_openai_strict() {
        for agent in all_agents() {
            let definition = agent.definition();
            assert_eq!(definition.strict, Some(true));
            assert_openai_strict_parameters(&definition.parameters);
        }
    }

    #[test]
    fn media_understanding_agents_share_tool_group() {
        for agent in all_agents() {
            let group = agent.group().expect("media agent should report a group");
            assert_eq!(group.id, catalog::MEDIA_UNDERSTANDING_TOOL_GROUP_ID);
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
        let args = parse(r#"{"path":"images/cat.png","question":"What is unusual?"}"#);

        assert_eq!(args.path.as_deref(), Some("images/cat.png"));
        assert_eq!(args.url, None);
        assert_eq!(args.question.as_deref(), Some("What is unusual?"));
    }

    #[test]
    fn parses_json_args_with_url_and_question() {
        let args = parse(r#"{"url":"https://example.com/cat.png","question":"What is unusual?"}"#);

        assert_eq!(args.path, None);
        assert_eq!(args.url.as_deref(), Some("https://example.com/cat.png"));
        assert_eq!(args.question.as_deref(), Some("What is unusual?"));
    }

    #[test]
    fn plain_prompt_becomes_question() {
        let args = parse("describe the scene");

        assert_eq!(args.path, None);
        assert_eq!(args.url, None);
        assert_eq!(args.question.as_deref(), Some("describe the scene"));
    }

    #[test]
    fn mistyped_json_args_are_an_error_not_a_question() {
        let err = MediaUnderstandingArgs::from_prompt(r#"{"resource_id":"42","path":"a.png"}"#)
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("invalid arguments"));
    }

    #[test]
    fn media_agent_names_include_other_understanding() {
        assert!(media_agent_names().contains(&OTHER_UNDERSTANDING_AGENT_NAME.to_string()));
        assert!(supported_media_resource_tags().contains(&"pdf".to_string()));
    }

    #[test]
    fn blank_alias_question_uses_default_question() {
        let args = parse(r#"{"path":"audio/sample.mp3","query":"   "}"#);

        assert_eq!(args.path.as_deref(), Some("audio/sample.mp3"));
        assert_eq!(
            args.question(MediaKind::Audio),
            MediaKind::Audio.default_question()
        );
    }

    #[test]
    fn locations_skip_blanks_and_repeats() {
        let args = parse(r#"{"url":" a.png ","path":"a.png"}"#);
        assert_eq!(args.locations(), vec!["a.png"]);

        let args = parse(r#"{"url":"https://example.com/a.png","path":"b.png"}"#);
        assert_eq!(args.locations(), vec!["https://example.com/a.png", "b.png"]);

        let args = parse(r#"{"url":"  ","path":null}"#);
        assert!(args.locations().is_empty());
    }

    #[test]
    fn parses_resource_id_and_its_alias() {
        let parsed = parse(r#"{"resource_id":42,"question":"What is red?"}"#);
        assert_eq!(parsed.resource_id(), Some(42));
        assert_eq!(parsed.question.as_deref(), Some("What is red?"));

        assert_eq!(parse(r#"{"_id":7}"#).resource_id(), Some(7));

        // A zero id is the strict-schema "absent" value some models send.
        assert_eq!(
            parse(r#"{"resource_id":0,"path":"a.png"}"#).resource_id(),
            None
        );
    }

    fn image_model(labels: &[&str]) -> Model {
        Model::mock_implemented().with_labels(labels.iter().map(|l| l.to_string()).collect())
    }

    fn png(name: &str, len: usize) -> Resource {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.resize(len.max(PNG_SIGNATURE.len()), 0);
        Resource {
            name: name.to_string(),
            mime_type: Some("image/png".to_string()),
            blob: Some(ByteBufB64(bytes)),
            ..Default::default()
        }
    }

    #[test]
    fn inline_images_go_to_models_labeled_image_only() {
        let attachments = vec![png("a.png", 16), text_resource("notes.txt", "hi")];

        let inline = inline_images(Some(&image_model(&["flash", "Image"])), &attachments);
        assert!(matches!(
            &inline[0],
            Some(ContentPart::InlineData { mime_type, .. }) if mime_type == "image/png"
        ));
        assert!(inline[1].is_none());

        for model in [Some(image_model(&["flash"])), None] {
            assert!(
                inline_images(model.as_ref(), &attachments)
                    .iter()
                    .all(Option::is_none)
            );
        }
    }

    #[test]
    fn inline_images_skip_unsupported_formats_and_oversized_bytes() {
        let model = image_model(&["image"]);
        // Labeled PNG but really a BMP: the sniffed type decides, and BMP is not portable.
        let bmp = Resource {
            name: "scan.png".to_string(),
            mime_type: Some("image/png".to_string()),
            blob: Some(ByteBufB64(b"BM\x00\x00\x00\x00\x00\x00\x00\x00".to_vec())),
            ..Default::default()
        };
        let big = png("big.png", MAX_INLINE_IMAGE_BYTES + 1);
        let referenced = Resource {
            name: "remote.png".to_string(),
            mime_type: Some("image/png".to_string()),
            uri: Some("https://example.com/remote.png".to_string()),
            ..Default::default()
        };
        assert!(
            inline_images(Some(&model), &[bmp, big, referenced])
                .iter()
                .all(Option::is_none)
        );

        // The message budget stops inlining once it is spent.
        let near_cap = MAX_INLINE_IMAGE_BYTES;
        let inline = inline_images(
            Some(&model),
            &[
                png("1.png", near_cap),
                png("2.png", near_cap),
                png("3.png", 16),
            ],
        );
        assert!(inline[0].is_some());
        assert!(inline[1].is_some());
        assert!(inline[2].is_none());
    }

    #[test]
    fn attachment_content_puts_bytes_after_their_reference() {
        let stored = vec![
            Resource {
                _id: 1,
                name: "a.png".to_string(),
                ..Default::default()
            },
            Resource {
                _id: 2,
                name: "b.txt".to_string(),
                ..Default::default()
            },
        ];
        let bytes = ContentPart::InlineData {
            mime_type: "image/png".to_string(),
            data: ByteBufB64(PNG_SIGNATURE.to_vec()),
        };

        let content = attachment_content(stored, vec![Some(bytes.clone()), None]);

        assert_eq!(content.len(), 3);
        let first = content[0].clone().any_into::<Resource>("Resource").unwrap();
        assert_eq!(first._id, 1);
        assert_eq!(content[1], bytes);
        let second = content[2].clone().any_into::<Resource>("Resource").unwrap();
        assert_eq!(second._id, 2);
    }

    #[test]
    fn media_agent_names_for_lists_each_kind_once() {
        let png = Resource {
            name: "a.png".to_string(),
            mime_type: Some("image/png".to_string()),
            ..Default::default()
        };
        let resources = vec![
            png.clone(),
            png,
            text_resource("notes.txt", "hi"),
            Resource {
                name: "clip.mp3".to_string(),
                ..Default::default()
            },
        ];
        assert_eq!(
            media_agent_names_for(&resources),
            vec![
                IMAGE_UNDERSTANDING_AGENT_NAME.to_string(),
                OTHER_UNDERSTANDING_AGENT_NAME.to_string(),
                AUDIO_UNDERSTANDING_AGENT_NAME.to_string(),
            ]
        );
        assert!(media_agent_names_for(&[]).is_empty());

        // An attachment of unknown type is still inspectable as a generic one.
        let unknown = Resource {
            name: "archive.bin".to_string(),
            ..Default::default()
        };
        assert_eq!(
            media_agent_names_for(&[unknown]),
            vec![OTHER_UNDERSTANDING_AGENT_NAME.to_string()]
        );
    }

    fn mock_ctx() -> AgentCtx {
        anda_engine::engine::EngineBuilder::new().mock_ctx()
    }

    fn mock_model_ctx() -> AgentCtx {
        anda_engine::engine::EngineBuilder::new()
            .with_model(anda_engine::model::Model::mock_implemented())
            .mock_ctx()
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

    fn image_resource() -> Resource {
        Resource {
            name: "photo.png".to_string(),
            mime_type: Some("image/png".to_string()),
            blob: Some(ByteBufB64(PNG_SIGNATURE.to_vec())),
            ..Default::default()
        }
    }

    #[test]
    fn media_understanding_args_from_blank_prompt_is_default() {
        let args = parse("   ");
        assert!(args.path.is_none() && args.url.is_none() && args.question.is_none());
    }

    #[test]
    fn completion_prompt_describes_inputs() {
        let agent = MediaUnderstandingAgent::image(Vec::new());
        let prompt = |resources, locations| agent.completion_prompt("focus", resources, locations);
        assert!(prompt(0, 1).contains("Understand the media file at the supplied path or URL."));
        assert!(prompt(0, 2).contains("the 2 media files at the supplied paths or URLs"));
        assert!(prompt(1, 0).contains("Understand the attached image resource."));
        assert!(prompt(2, 0).contains("the 2 attached image resources"));
        assert!(prompt(1, 1).contains(
            "the attached image resource and the media file at the supplied path or URL"
        ));
        assert!(prompt(2, 2).contains("2 attached image resources and the 2 media files"));
        assert!(prompt(1, 0).ends_with("Caller question or focus:\nfocus"));
    }

    #[tokio::test]
    async fn run_other_inlines_small_text_attachment() {
        let agent = MediaUnderstandingAgent::other(Vec::new());
        let output = agent
            .run(
                mock_ctx(),
                "summarize".to_string(),
                vec![text_resource("notes.txt", "hello world")],
            )
            .await
            .expect("text attachment should be understood without a model");
        assert!(output.content.contains("hello world"));
        assert!(output.content.contains("text attachment"));
    }

    #[tokio::test]
    async fn run_other_converts_a_document_with_anydoc() {
        // RTF decodes as plain text, so this also pins the routing order: the
        // content signature has to win over the text path, or the agent would
        // hand the model raw control words instead of Markdown.
        let agent = MediaUnderstandingAgent::other(Vec::new());
        let resource = Resource {
            name: "memo.rtf".to_string(),
            mime_type: Some("application/rtf".to_string()),
            blob: Some(ByteBufB64(
                br"{\rtf1\ansi\ansicpg1252 Quarterly panda census: 42.\par}".to_vec(),
            )),
            ..Default::default()
        };

        let output = agent
            .run(mock_ctx(), "summarize".to_string(), vec![resource])
            .await
            .expect("document attachment should convert without a model");

        assert!(output.content.contains("Quarterly panda census: 42."));
        assert!(output.content.contains("converted by anydoc"));
        assert!(!output.content.contains(r"\rtf1"));
    }

    #[tokio::test]
    async fn run_requires_an_input() {
        for agent in all_agents() {
            let err = agent
                .run(mock_ctx(), "{}".to_string(), vec![])
                .await
                .map(|_| ())
                .unwrap_err();
            assert!(err.to_string().contains("requires a resource_id"));
        }
    }

    #[tokio::test]
    async fn run_rejects_mistyped_arguments() {
        let err = MediaUnderstandingAgent::image(Vec::new())
            .run(
                mock_ctx(),
                r#"{"resource_id":"7","question":"what?"}"#.to_string(),
                vec![],
            )
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("invalid arguments"));
    }

    #[tokio::test]
    async fn run_other_absorbs_fallback_failures_into_sections() {
        // A binary, non-text, non-document attachment falls through to the
        // model fallback, which fails on the mock ctx; the error is captured in
        // the section text rather than failing the whole run.
        let agent = MediaUnderstandingAgent::other(Vec::new());
        let resource = Resource {
            name: "blob.bin".to_string(),
            mime_type: Some("application/octet-stream".to_string()),
            blob: Some(ByteBufB64(vec![0u8, 1, 2, 3, 0, 0, 0, 0])),
            ..Default::default()
        };
        let output = agent
            .run(mock_ctx(), "{}".to_string(), vec![resource])
            .await
            .expect("run_other should not fail on fallback errors");
        assert!(output.content.contains("Failed to understand blob.bin"));
    }

    #[tokio::test]
    async fn run_image_builds_content_then_fails_without_model() {
        let agent = MediaUnderstandingAgent::image(Vec::new());
        // The content builds; the completion fails (no model).
        let err = agent
            .run(mock_ctx(), "{}".to_string(), vec![image_resource()])
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    #[tokio::test]
    async fn url_inputs_are_fetched_and_checked() {
        let app = Router::new()
            .route("/doc.txt", get(|| async { "remote body" }))
            .route("/missing", get(|| async { (AxumStatus::NOT_FOUND, "") }))
            .route(
                "/text",
                get(|| async {
                    (
                        [(axum::http::header::CONTENT_TYPE, "text/plain")],
                        "not an image",
                    )
                }),
            );
        let base = crate::test_support::spawn_http_mock(app).await;
        let url_args = |path: &str| json!({ "url": format!("{base}{path}") }).to_string();

        let other = MediaUnderstandingAgent::other(Vec::new()).allow_private_urls_for_tests();
        let output = other
            .run(mock_ctx(), url_args("/doc.txt"), vec![])
            .await
            .expect("a fetched text file should be inlined");
        assert!(output.content.contains("remote body"));
        let err = other
            .run(mock_ctx(), url_args("/missing"), vec![])
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("failed to fetch"));

        let image = MediaUnderstandingAgent::image(Vec::new()).allow_private_urls_for_tests();
        let err = image
            .run(mock_ctx(), url_args("/missing"), vec![])
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("failed to fetch"));
        let err = image
            .run(mock_ctx(), url_args("/text"), vec![])
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("does not look like image media"));
    }

    #[tokio::test]
    async fn one_location_in_url_and_path_is_loaded_once() {
        let app = Router::new().route(
            "/cat.png",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "image/png")],
                    PNG_SIGNATURE.to_vec(),
                )
            }),
        );
        let base = crate::test_support::spawn_http_mock(app).await;
        let location = format!("{base}/cat.png");
        let agent = MediaUnderstandingAgent::image(Vec::new()).allow_private_urls_for_tests();

        let output = agent
            .run(
                mock_model_ctx(),
                json!({ "url": location, "path": location }).to_string(),
                vec![],
            )
            .await
            .expect("the image should be understood");

        // The mock model echoes the prompt, which counts the inputs.
        assert!(
            output
                .content
                .contains("the media file at the supplied path or URL")
        );
        assert!(!output.content.contains("2 media files"));
    }

    #[tokio::test]
    async fn run_other_summarizes_large_text_via_model() {
        // Text beyond the inline limit is summarized, which succeeds with the
        // deterministic mock model.
        let agent = MediaUnderstandingAgent::other(Vec::new());
        let big = "lorem ipsum ".repeat(8000);
        let output = agent
            .run(
                mock_model_ctx(),
                "summarize".to_string(),
                vec![text_resource("big.txt", &big)],
            )
            .await
            .expect("large text summary should succeed");
        assert!(output.content.contains("too large to inline"));
    }

    #[tokio::test]
    async fn run_other_falls_back_to_model_for_binary_attachment() {
        let agent = MediaUnderstandingAgent::other(Vec::new());
        let resource = Resource {
            name: "blob.bin".to_string(),
            mime_type: Some("application/octet-stream".to_string()),
            blob: Some(ByteBufB64(vec![0u8, 1, 2, 3, 0, 0, 0, 0])),
            ..Default::default()
        };
        let output = agent
            .run(mock_model_ctx(), "{}".to_string(), vec![resource])
            .await
            .expect("fallback understanding should succeed with the mock model");
        assert!(!output.content.is_empty());
    }

    #[tokio::test]
    async fn run_image_completes_with_mock_model() {
        let agent = MediaUnderstandingAgent::image(Vec::new());
        let output = agent
            .run(mock_model_ctx(), "{}".to_string(), vec![image_resource()])
            .await
            .expect("image understanding should complete with the mock model");
        assert!(output.content.contains("attached image resource"));
        assert!(output.content.contains("Describe the image"));
    }

    #[tokio::test]
    async fn resource_id_loads_an_owned_attachment_of_the_matching_kind() {
        let ctx = mock_model_ctx();
        let store = Arc::new(
            ResourceStore::connect(crate::test_support::memory_db("media_resources").await)
                .await
                .unwrap(),
        );
        let image = image_resource();
        let saved = store
            .persist_resources(
                ctx.caller(),
                vec![image.clone(), text_resource("a.txt", "hi")],
            )
            .await
            .unwrap();
        let (image_id, text_id) = (saved[0]._id, saved[1]._id);
        let mut foreign_image = image;
        foreign_image.blob = Some(ByteBufB64([PNG_SIGNATURE.as_slice(), b"other"].concat()));
        let foreign = store
            .persist_resources(
                &anda_core::Principal::management_canister(),
                vec![foreign_image],
            )
            .await
            .unwrap()[0]
            ._id;

        let agent = MediaUnderstandingAgent::image(Vec::new()).with_resource_store(store);
        let output = agent
            .run(
                ctx.clone(),
                json!({ "resource_id": image_id, "question": "What color is it?" }).to_string(),
                vec![],
            )
            .await
            .expect("an owned image attachment should be understood");
        assert!(output.content.contains("attached image resource"));
        assert!(output.content.contains("What color is it?"));

        let err = agent
            .run(
                ctx.clone(),
                json!({ "resource_id": text_id }).to_string(),
                vec![],
            )
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("use attachment_understanding"));

        let err = agent
            .run(ctx, json!({ "resource_id": foreign }).to_string(), vec![])
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("permission denied"));
    }

    #[tokio::test]
    async fn resource_id_without_a_store_is_rejected() {
        let agent = MediaUnderstandingAgent::image(Vec::new());
        let err = agent
            .run(mock_ctx(), json!({ "resource_id": 1 }).to_string(), vec![])
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("resource_id is not supported"));
    }

    #[tokio::test]
    async fn media_access_honors_owner_registered_workspaces_only() {
        use anda_core::Principal;
        let temp = tempfile::tempdir().unwrap();
        let configured = temp.path().join("configured");
        let registered = temp.path().join("registered");
        tokio::fs::create_dir_all(&configured).await.unwrap();
        tokio::fs::create_dir_all(&registered).await.unwrap();
        tokio::fs::write(registered.join("note.txt"), b"registered data")
            .await
            .unwrap();
        let note = registered.join("note.txt");
        let note = note.to_str().unwrap();
        let owner = Principal::from_slice(&[1]);
        let grants = crate::engine::shell_runtime::CliWorkspaceGrants::new(owner);
        grants.register(&registered).await.unwrap();
        let agent = MediaUnderstandingAgent::other(vec![configured]).with_cli_workspaces(grants);
        let ctx = mock_ctx().with_caller(owner);
        assert!(
            agent
                .sources(&ctx)
                .load_path(ctx.meta(), note)
                .await
                .is_ok()
        );

        let other = ctx.with_caller(Principal::from_slice(&[2]));
        assert!(
            agent
                .sources(&other)
                .load_path(other.meta(), note)
                .await
                .is_err()
        );
    }
}
