use anda_core::{AgentInput, AgentOutput, BoxError, Message, RequestMeta, ToolInput, Usage};
use anda_engine::memory::{Conversation, ConversationStatus};
use anda_kip::Response as KipResponse;
use clap::{Args, Subcommand};
use serde_json::json;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::{
    engine::{ConversationsTool, ConversationsToolArgs},
    gateway,
    util::text::read_text_file,
};

const DEFAULT_POLL_INTERVAL_MS: u64 = 1000;
const MAX_CONVERSATION_CHAIN: usize = 64;

#[derive(Subcommand)]
pub enum AgentCommand {
    /// Run an agent once and wait until the response is complete.
    Run(AgentRunCommand),
}

#[derive(Args)]
pub struct AgentRunCommand {
    /// Agent name. Empty value uses the default agent.
    #[arg(long, default_value = "")]
    name: String,

    /// User prompt sent to the agent.
    #[arg(long)]
    prompt: Option<String>,

    /// File containing the prompt.
    #[arg(long)]
    prompt_file: Option<PathBuf>,

    /// Authoritative workspace for file and shell tools.
    #[arg(long)]
    workspace: Option<PathBuf>,

    /// Stable session id. Stored as request metadata thread.
    #[arg(long)]
    session_id: Option<String>,

    /// Optional request metadata as a JSON object.
    #[arg(long)]
    meta: Option<String>,

    /// Optional path for the complete AgentOutput JSON.
    #[arg(long)]
    output_json: Option<PathBuf>,

    /// Maximum seconds to wait for completion. 0 means wait indefinitely.
    #[arg(long, default_value_t = 0)]
    wait_timeout_secs: u64,

    /// Poll interval in milliseconds while waiting for completion.
    #[arg(long, default_value_t = DEFAULT_POLL_INTERVAL_MS)]
    poll_interval_ms: u64,
}

pub async fn run(client: &gateway::Client, cmd: AgentCommand) -> Result<(), BoxError> {
    match cmd {
        AgentCommand::Run(cmd) => run_once(client, cmd).await,
    }
}

async fn run_once(client: &gateway::Client, cmd: AgentRunCommand) -> Result<(), BoxError> {
    let prompt = read_prompt(cmd.prompt.as_deref(), cmd.prompt_file.as_ref()).await?;
    let workspace = match cmd.workspace.as_ref() {
        Some(path) => {
            let workspace = absolute_workspace(path)?;
            Some(workspace)
        }
        None => None,
    };

    let mut meta = parse_meta(cmd.meta)?;
    apply_agent_meta_defaults(&mut meta, workspace.as_deref(), cmd.session_id.as_deref());

    let mut input = AgentInput::new(cmd.name, prompt.clone());
    input.meta = Some(meta);

    let initial_output = client.agent_run(&input).await?;
    let output = wait_for_agent_output(
        client,
        initial_output,
        wait_timeout(cmd.wait_timeout_secs),
        Duration::from_millis(cmd.poll_interval_ms.max(500)),
    )
    .await?;

    if let Some(path) = cmd.output_json.as_ref() {
        write_text(path, &serde_json::to_string_pretty(&output)?).await?;
    }

    println!("\n{}", serde_json::to_string_pretty(&output)?);

    if let Some(reason) = output.failed_reason.as_deref()
        && !reason.trim().is_empty()
    {
        return Err(format!("agent failed: {reason}").into());
    }

    Ok(())
}

async fn read_prompt(
    prompt: Option<&str>,
    prompt_file: Option<&PathBuf>,
) -> Result<String, BoxError> {
    match (prompt, prompt_file) {
        (Some(_), Some(_)) => Err("--prompt and --prompt-file cannot be used together".into()),
        (Some(prompt), None) => Ok(prompt.to_string()),
        (None, Some(path)) => Ok(read_text_file(path).await?),
        (None, None) => Err("--prompt or --prompt-file is required".into()),
    }
}

fn wait_timeout(secs: u64) -> Option<Duration> {
    (secs > 0).then(|| Duration::from_secs(secs))
}

fn absolute_workspace(path: &Path) -> Result<PathBuf, BoxError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn parse_meta(meta: Option<String>) -> Result<RequestMeta, BoxError> {
    match meta {
        Some(meta) => {
            Ok(serde_json::from_str(&meta).map_err(|e| format!("invalid --meta JSON: {e}"))?)
        }
        None => Ok(RequestMeta::default()),
    }
}

fn apply_agent_meta_defaults(
    meta: &mut RequestMeta,
    workspace: Option<&Path>,
    session_id: Option<&str>,
) {
    if let Some(workspace) = workspace {
        meta.extra
            .entry("workspace".to_string())
            .or_insert_with(|| json!(workspace.to_string_lossy().to_string()));
    }

    if let Some(session_id) = session_id.filter(|session_id| !session_id.trim().is_empty()) {
        meta.extra
            .entry("thread".to_string())
            .or_insert_with(|| json!(session_id));
    }
}

async fn wait_for_agent_output(
    client: &gateway::Client,
    initial: AgentOutput,
    timeout: Option<Duration>,
    poll_interval: Duration,
) -> Result<AgentOutput, BoxError> {
    let Some(root_id) = initial.conversation else {
        return Ok(initial);
    };

    let started_at = Instant::now();
    let mut conversations = Vec::new();
    let mut seen = HashSet::new();
    let mut current_id = root_id;

    loop {
        let conversation = get_conversation(client, current_id).await?;
        upsert_conversation(&mut conversations, &mut seen, conversation)?;

        let last = conversations
            .last()
            .expect("conversation list is populated after upsert");

        if let Some(child_id) = last.child {
            if child_id == current_id || seen.contains(&child_id) {
                return Err(
                    format!("conversation child chain contains a cycle at {child_id}").into(),
                );
            }
            current_id = child_id;
            continue;
        }

        if is_terminal_conversation_status(&last.status) {
            return Ok(output_from_conversation_chain(initial, &conversations));
        }

        if let Some(timeout) = timeout
            && started_at.elapsed() >= timeout
        {
            return Err(format!(
                "agent did not complete conversation {root_id} within {}s",
                timeout.as_secs()
            )
            .into());
        }

        tokio::time::sleep(poll_interval).await;
    }
}

fn upsert_conversation(
    conversations: &mut Vec<Conversation>,
    seen: &mut HashSet<u64>,
    conversation: Conversation,
) -> Result<(), BoxError> {
    if let Some(last) = conversations.last_mut()
        && last._id == conversation._id
    {
        *last = conversation;
        return Ok(());
    }

    if conversations.len() >= MAX_CONVERSATION_CHAIN {
        return Err(
            format!("conversation child chain is longer than {MAX_CONVERSATION_CHAIN}").into(),
        );
    }
    if !seen.insert(conversation._id) {
        return Err(format!(
            "conversation child chain contains a cycle at {}",
            conversation._id
        )
        .into());
    }
    conversations.push(conversation);
    Ok(())
}

async fn get_conversation(
    client: &gateway::Client,
    conversation_id: u64,
) -> Result<Conversation, BoxError> {
    let output = client
        .tool_call::<ConversationsToolArgs, KipResponse>(&ToolInput::new(
            ConversationsTool::NAME.to_string(),
            ConversationsToolArgs::GetConversation {
                _id: conversation_id,
            },
        ))
        .await?;

    match output.output {
        KipResponse::Ok { result, .. } => Ok(serde_json::from_value::<Conversation>(result)?),
        other => Err(format!("conversation API returned an error: {other:?}").into()),
    }
}

fn output_from_conversation_chain(
    mut output: AgentOutput,
    conversations: &[Conversation],
) -> AgentOutput {
    let Some(last) = conversations.last() else {
        return output;
    };

    let mut usage = Usage::default();
    let mut chat_history = Vec::new();
    let mut artifacts = Vec::new();
    for conversation in conversations {
        usage.accumulate(&conversation.usage);
        artifacts.extend(conversation.artifacts.clone());
        chat_history.extend(
            conversation
                .messages
                .iter()
                .filter_map(|message| serde_json::from_value::<Message>(message.clone()).ok()),
        );
    }

    output.content = latest_assistant_text(&chat_history).unwrap_or_default();
    output.thoughts = latest_assistant_thoughts(&chat_history);
    output.usage = usage;
    output.chat_history = chat_history;
    output.artifacts = artifacts;
    output.conversation = Some(last._id);
    output.failed_reason = last.failed_reason.clone().or_else(|| {
        if matches!(last.status, ConversationStatus::Cancelled) {
            Some("conversation cancelled".to_string())
        } else if matches!(last.status, ConversationStatus::Failed) {
            Some("conversation failed".to_string())
        } else {
            None
        }
    });
    output
}

fn latest_assistant_text(messages: &[Message]) -> Option<String> {
    messages
        .iter()
        .rev()
        .filter(|message| message.role == "assistant")
        .filter_map(Message::text)
        .find(|text| !text.trim().is_empty())
}

fn latest_assistant_thoughts(messages: &[Message]) -> Option<String> {
    messages
        .iter()
        .rev()
        .filter(|message| message.role == "assistant")
        .filter_map(Message::thoughts)
        .find(|text| !text.trim().is_empty())
}

async fn write_text(path: &Path, text: &str) -> Result<(), BoxError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, text).await?;
    Ok(())
}

fn is_terminal_conversation_status(status: &ConversationStatus) -> bool {
    matches!(
        status,
        ConversationStatus::Completed | ConversationStatus::Cancelled | ConversationStatus::Failed
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use anda_core::ContentPart;

    #[test]
    fn agent_meta_defaults_preserve_explicit_values() {
        let mut meta: RequestMeta = serde_json::from_value(json!({
            "source": "custom",
            "workspace": "/tmp/custom",
        }))
        .unwrap();

        apply_agent_meta_defaults(&mut meta, Some(Path::new("/tmp/project")), Some("s1"));

        assert_eq!(meta.user.as_deref(), None);
        assert_eq!(
            meta.get_extra_as::<String>("source"),
            Some("custom".to_string())
        );
        assert_eq!(
            meta.get_extra_as::<String>("workspace"),
            Some("/tmp/custom".to_string())
        );
        assert_eq!(
            meta.get_extra_as::<String>("thread"),
            Some("s1".to_string())
        );
    }

    #[test]
    fn upsert_conversation_updates_tail_without_growing_chain() {
        let mut conversations = Vec::new();
        let mut seen = HashSet::new();

        upsert_conversation(
            &mut conversations,
            &mut seen,
            Conversation {
                _id: 1,
                status: ConversationStatus::Working,
                ..Default::default()
            },
        )
        .unwrap();
        upsert_conversation(
            &mut conversations,
            &mut seen,
            Conversation {
                _id: 1,
                status: ConversationStatus::Completed,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(conversations.len(), 1);
        assert_eq!(conversations[0].status, ConversationStatus::Completed);
    }

    #[test]
    fn conversation_chain_output_uses_final_assistant_message_and_usage() {
        let root = Conversation {
            _id: 1,
            messages: vec![json!(Message {
                role: "assistant".to_string(),
                content: vec![ContentPart::Text {
                    text: "intermediate".to_string(),
                }],
                ..Default::default()
            })],
            usage: Usage {
                input_tokens: 10,
                output_tokens: 5,
                cached_tokens: 2,
                requests: 1,
            },
            child: Some(2),
            ..Default::default()
        };
        let child = Conversation {
            _id: 2,
            messages: vec![json!(Message {
                role: "assistant".to_string(),
                content: vec![ContentPart::Text {
                    text: "done".to_string(),
                }],
                ..Default::default()
            })],
            usage: Usage {
                input_tokens: 3,
                output_tokens: 4,
                cached_tokens: 1,
                requests: 1,
            },
            status: ConversationStatus::Completed,
            ..Default::default()
        };

        let output = output_from_conversation_chain(AgentOutput::default(), &[root, child]);

        assert_eq!(output.content, "done");
        assert_eq!(output.conversation, Some(2));
        assert_eq!(output.usage.input_tokens, 13);
        assert_eq!(output.usage.output_tokens, 9);
        assert_eq!(output.usage.cached_tokens, 3);
        assert_eq!(output.usage.requests, 2);
        assert_eq!(output.chat_history.len(), 2);
    }

    use anda_core::ByteBufB64;
    use axum::{Router, extract::State, routing};
    use base64::Engine;
    use std::{collections::HashMap, sync::Arc};

    async fn agent_gateway_handler(
        State(state): State<Arc<HashMap<u64, Conversation>>>,
        axum::Json(request): axum::Json<serde_json::Value>,
    ) -> axum::Json<serde_json::Value> {
        let method = request["method"].as_str().unwrap_or_default();
        let rpc: anda_core::http::RPCResponse = if method == "agent_run" {
            let output = AgentOutput {
                conversation: Some(1),
                ..Default::default()
            };
            Ok(ByteBufB64(serde_json::to_vec(&output).unwrap()))
        } else {
            let params = base64::engine::general_purpose::STANDARD
                .decode(request["params"].as_str().unwrap_or_default())
                .unwrap_or_default();
            let (input,): (ToolInput<serde_json::Value>,) =
                serde_json::from_slice(&params).unwrap();
            let id = input.args["_id"].as_u64().unwrap_or_default();
            let conversation = state.get(&id).expect("known conversation");
            let response = KipResponse::Ok {
                result: serde_json::to_value(conversation).unwrap(),
                next_cursor: None,
            };
            let output: anda_core::ToolOutput<KipResponse> = anda_core::ToolOutput::new(response);
            Ok(ByteBufB64(serde_json::to_vec(&output).unwrap()))
        };
        axum::Json(serde_json::to_value(&rpc).unwrap())
    }

    async fn spawn_agent_gateway(conversations: HashMap<u64, Conversation>) -> gateway::Client {
        let app = Router::new()
            .route("/engine/default", routing::post(agent_gateway_handler))
            .with_state(Arc::new(conversations));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        gateway::Client::new(format!("http://{addr}"), "token".to_string())
    }

    fn finished_conversation(id: u64, status: ConversationStatus) -> Conversation {
        Conversation {
            _id: id,
            status,
            messages: vec![
                serde_json::to_value(Message {
                    role: "assistant".to_string(),
                    content: vec![ContentPart::Text {
                        text: format!("final answer {id}"),
                    }],
                    ..Default::default()
                })
                .unwrap(),
            ],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn run_once_waits_for_completion_and_writes_output() {
        let client = spawn_agent_gateway(HashMap::from([(
            1,
            finished_conversation(1, ConversationStatus::Completed),
        )]))
        .await;

        let dir = tempfile::tempdir().unwrap();
        let output_path = dir.path().join("nested").join("output.json");
        run_once(
            &client,
            AgentRunCommand {
                name: String::new(),
                prompt: Some("do the thing".to_string()),
                prompt_file: None,
                workspace: Some(PathBuf::from("relative-ws")),
                session_id: Some("session-1".to_string()),
                meta: Some(r#"{"user":"alice"}"#.to_string()),
                output_json: Some(output_path.clone()),
                wait_timeout_secs: 5,
                poll_interval_ms: 1,
            },
        )
        .await
        .unwrap();

        let written = std::fs::read_to_string(output_path).unwrap();
        assert!(written.contains("final answer 1"));
    }

    #[tokio::test]
    async fn run_once_propagates_agent_failure() {
        let client = spawn_agent_gateway(HashMap::from([(
            1,
            finished_conversation(1, ConversationStatus::Failed),
        )]))
        .await;

        let err = run_once(
            &client,
            AgentRunCommand {
                name: String::new(),
                prompt: Some("do the thing".to_string()),
                prompt_file: None,
                workspace: None,
                session_id: None,
                meta: None,
                output_json: None,
                wait_timeout_secs: 5,
                poll_interval_ms: 1,
            },
        )
        .await
        .map(|_| ())
        .unwrap_err();
        assert!(err.to_string().contains("agent failed"));
    }

    #[tokio::test]
    async fn wait_for_agent_output_times_out_and_detects_cycles() {
        // No conversation id: the initial output is returned untouched.
        let client = spawn_agent_gateway(HashMap::new()).await;
        let initial = AgentOutput {
            content: "direct".to_string(),
            ..Default::default()
        };
        let output = wait_for_agent_output(&client, initial, None, Duration::from_millis(1))
            .await
            .unwrap();
        assert_eq!(output.content, "direct");

        // A conversation stuck in Working trips the timeout.
        let client = spawn_agent_gateway(HashMap::from([(
            1,
            finished_conversation(1, ConversationStatus::Working),
        )]))
        .await;
        let initial = AgentOutput {
            conversation: Some(1),
            ..Default::default()
        };
        let err = wait_for_agent_output(
            &client,
            initial,
            Some(Duration::ZERO),
            Duration::from_millis(1),
        )
        .await
        .map(|_| ())
        .unwrap_err();
        assert!(err.to_string().contains("did not complete"));

        // A self-referencing child chain is detected as a cycle.
        let mut looping = finished_conversation(1, ConversationStatus::Working);
        looping.child = Some(1);
        let client = spawn_agent_gateway(HashMap::from([(1, looping)])).await;
        let initial = AgentOutput {
            conversation: Some(1),
            ..Default::default()
        };
        let err = wait_for_agent_output(&client, initial, None, Duration::from_millis(1))
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("cycle"));
    }

    #[tokio::test]
    async fn read_prompt_validates_sources() {
        let dir = tempfile::tempdir().unwrap();
        let prompt_path = dir.path().join("prompt.txt");
        tokio::fs::write(&prompt_path, "from file").await.unwrap();

        assert_eq!(read_prompt(Some("inline"), None).await.unwrap(), "inline");
        assert_eq!(
            read_prompt(None, Some(&prompt_path)).await.unwrap(),
            "from file"
        );
        assert!(
            read_prompt(Some("inline"), Some(&prompt_path))
                .await
                .is_err()
        );
        assert!(read_prompt(None, None).await.is_err());
    }

    #[test]
    fn meta_and_workspace_helpers_validate_input() {
        assert!(parse_meta(None).unwrap().extra.is_empty());
        assert_eq!(
            parse_meta(Some(r#"{"user":"alice"}"#.to_string()))
                .unwrap()
                .user
                .as_deref(),
            Some("alice")
        );
        assert!(parse_meta(Some("not json".to_string())).is_err());

        assert_eq!(wait_timeout(0), None);
        assert_eq!(wait_timeout(5), Some(Duration::from_secs(5)));

        let absolute = absolute_workspace(Path::new("/tmp/abs")).unwrap();
        assert_eq!(absolute, PathBuf::from("/tmp/abs"));
        let relative = absolute_workspace(Path::new("rel")).unwrap();
        assert!(relative.is_absolute());
        assert!(relative.ends_with("rel"));
    }
}
