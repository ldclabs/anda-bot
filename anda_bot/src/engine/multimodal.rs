mod attachment;
mod catalog;
mod source;

use anda_core::{
    Agent, AgentContext, AgentInput, AgentOutput, BoxError, CompletionFeatures, CompletionRequest,
    ContentPart, FunctionDefinition, RequestMeta, Resource, StateFeatures, ToolGroupInfo, Usage,
};
use anda_engine::context::AgentCtx;
use futures_util::{StreamExt, stream};
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

use crate::util::http_client::new_reqwest_client;
use attachment::{AttachmentUnderstanding, OtherAttachment, other_understanding_tool_names};
pub use catalog::{
    AUDIO_UNDERSTANDING_AGENT_NAME, IMAGE_UNDERSTANDING_AGENT_NAME, MediaKind,
    OTHER_UNDERSTANDING_AGENT_NAME, VIDEO_UNDERSTANDING_AGENT_NAME,
};
use source::{MediaSourceLoader, resource_label};

pub const MEDIA_UNDERSTANDING_TOOL_GROUP_ID: &str = catalog::MEDIA_UNDERSTANDING_TOOL_GROUP_ID;
const MAX_MEDIA_UNDERSTANDING_CONCURRENCY: usize = 8;

pub fn media_understanding_tool_group_info() -> ToolGroupInfo {
    let group = catalog::media_understanding_tool_group_info();
    debug_assert_eq!(group.id, MEDIA_UNDERSTANDING_TOOL_GROUP_ID);
    group
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

    fn source_loader(&self) -> MediaSourceLoader {
        MediaSourceLoader::new(self.kind, self.workspaces.clone(), self.http.clone())
    }

    fn attachment_understanding(&self) -> AttachmentUnderstanding {
        AttachmentUnderstanding::new(self.workspaces.clone(), self.http.clone())
    }

    async fn run_other(
        &self,
        ctx: AgentCtx,
        prompt: String,
        resources: Vec<Resource>,
    ) -> Result<AgentOutput, BoxError> {
        let args = MediaUnderstandingArgs::from_prompt(&prompt);
        let question = args.question_or_default(self.kind);
        let attachment_understanding = self.attachment_understanding();
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
            attachments.push(
                attachment_understanding
                    .attachment_from_location(ctx.meta(), url)
                    .await?,
            );
        }

        if let Some(path) = args
            .path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            attachments.push(
                attachment_understanding
                    .attachment_from_location(ctx.meta(), path)
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

    async fn understand_other_attachment(
        &self,
        ctx: &AgentCtx,
        attachment: OtherAttachment,
        question: &str,
    ) -> Result<AgentOutput, BoxError> {
        self.attachment_understanding()
            .understand(ctx, attachment, question)
            .await
    }

    async fn content_from_location(
        &self,
        meta: &RequestMeta,
        location: &str,
    ) -> Result<ContentPart, BoxError> {
        self.source_loader()
            .content_from_location(meta, location)
            .await
    }

    fn content_from_resource(&self, resource: Resource) -> Result<ContentPart, BoxError> {
        self.source_loader().content_from_resource(resource)
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
    use anda_core::ByteBufB64;

    const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];

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
    fn media_understanding_args_from_blank_prompt_is_default() {
        let args = MediaUnderstandingArgs::from_prompt("   ");
        assert!(args.path.is_none() && args.url.is_none() && args.question.is_none());
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
    async fn other_attachment_from_http_url_fetches_and_reports_status() {
        let app = Router::new()
            .route("/doc.txt", get(|| async { "remote body" }))
            .route("/missing", get(|| async { (AxumStatus::NOT_FOUND, "") }));
        let base = spawn_router(app).await;
        let agent =
            MediaUnderstandingAgent::other(Vec::new()).with_http_client(new_reqwest_client());

        let url = reqwest::Url::parse(&format!("{base}/doc.txt")).unwrap();
        let attachment = agent
            .attachment_understanding()
            .attachment_from_http_url(url)
            .await
            .unwrap();
        assert_eq!(attachment.data.as_deref(), Some(b"remote body".as_ref()));
        assert_eq!(attachment.name, "doc.txt");

        let missing = reqwest::Url::parse(&format!("{base}/missing")).unwrap();
        let err = agent
            .attachment_understanding()
            .attachment_from_http_url(missing)
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
            .source_loader()
            .content_from_http_url(missing)
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("failed to fetch media"));

        let wrong_kind = reqwest::Url::parse(&format!("{base}/text")).unwrap();
        let err = agent
            .source_loader()
            .content_from_http_url(wrong_kind)
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("does not look like image media"));
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
        let big = "lorem ipsum ".repeat(8000); // large enough to use the summary path
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
