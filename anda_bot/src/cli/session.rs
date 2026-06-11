use anda_core::{BoxError, ToolInput};
use anda_kip::Response as KipResponse;
use clap::{Args, Subcommand};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    engine::{AndaBot, AndaBotToolArgs, SessionState, SessionSummary},
    gateway,
};

#[derive(Args)]
pub struct SessionCommand {
    /// Output JSON for scripting.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Option<SessionsSubcommand>,
}

#[derive(Subcommand)]
enum SessionsSubcommand {
    /// List currently active sessions.
    List,
    /// Inspect one active session by session id.
    Get {
        /// Session id returned by `anda sessions`.
        session_id: String,
    },
}

pub async fn run(client: &gateway::Client, cmd: SessionCommand) -> Result<(), BoxError> {
    let SessionCommand { json, command } = cmd;
    match command.unwrap_or(SessionsSubcommand::List) {
        SessionsSubcommand::List => list_sessions(client, json).await,
        SessionsSubcommand::Get { session_id } => get_session(client, session_id, json).await,
    }
}

async fn list_sessions(client: &gateway::Client, json_output: bool) -> Result<(), BoxError> {
    let response = call_sessions_tool(client, AndaBotToolArgs::ListSessions {}).await?;
    let sessions: Vec<SessionSummary> = decode_ok(response)?;

    if json_output {
        print_json(&sessions)?;
    } else {
        print_session_list(&sessions);
    }
    Ok(())
}

async fn get_session(
    client: &gateway::Client,
    session_id: String,
    json_output: bool,
) -> Result<(), BoxError> {
    let response = call_sessions_tool(client, AndaBotToolArgs::GetSession { session_id }).await?;
    let session: SessionState = decode_ok(response)?;

    if json_output {
        print_json(&session)?;
    } else {
        print_session_state(&session);
    }
    Ok(())
}

async fn call_sessions_tool(
    client: &gateway::Client,
    args: AndaBotToolArgs,
) -> Result<KipResponse, BoxError> {
    let output = client
        .tool_call::<AndaBotToolArgs, KipResponse>(&ToolInput::new(
            format!("{}_api", AndaBot::NAME),
            args,
        ))
        .await?;
    Ok(output.output)
}

fn decode_ok<T>(response: KipResponse) -> Result<T, BoxError>
where
    T: DeserializeOwned,
{
    match response {
        KipResponse::Ok { result, .. } => Ok(serde_json::from_value(result)?),
        other => Err(format!("anda_bot sessions API returned an error: {other:?}").into()),
    }
}

fn print_json<T>(value: &T) -> Result<(), BoxError>
where
    T: Serialize,
{
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn print_session_list(sessions: &[SessionSummary]) {
    if sessions.is_empty() {
        println!("No active sessions.");
        return;
    }

    println!("Active sessions:");
    for session in sessions {
        println!(
            "- {} caller={} source={} conversation={} idle={} goal={} background_tasks={}",
            session.id,
            session.caller,
            session.source,
            session.conversation_id,
            format_duration_ms(session.idle_ms),
            session.has_goal,
            session.background_task_count,
        );
    }
}

fn print_session_state(session: &SessionState) {
    let summary = &session.summary;
    println!("Session {}", summary.id);
    println!("  caller: {}", summary.caller);
    println!("  source: {}", summary.source);
    println!("  workspace: {}", summary.workspace);
    println!("  conversation: {}", summary.conversation_id);
    println!("  active_at: {}", summary.active_at);
    println!("  idle: {}", format_duration_ms(summary.idle_ms));
    println!("  submit_formation_at: {}", session.submit_formation_at);

    if let Some(context) = &session.formation_context {
        println!(
            "  formation_context: counterparty={} agent={} source={} topic={}",
            optional_str(&context.counterparty),
            optional_str(&context.agent),
            optional_str(&context.source),
            optional_str(&context.topic),
        );
    }

    match &session.goal {
        Some(goal) => {
            println!("  goal: {}", single_line(&goal.objective));
            if let Some(prev_objective) = &goal.prev_objective {
                println!("  previous_goal: {}", single_line(prev_objective));
            }
            if let Some(evaluation) = &goal.prev_evaluation {
                println!(
                    "  previous_evaluation: complete={} reason={}",
                    evaluation.complete,
                    single_line(&evaluation.reason),
                );
                if !evaluation.follow_up.trim().is_empty() {
                    println!("  follow_up: {}", single_line(&evaluation.follow_up));
                }
            }
        }
        None => println!("  goal: none"),
    }

    if session.background_tasks.is_empty() {
        println!("  background_tasks: none");
        return;
    }

    println!("  background_tasks:");
    let mut tasks = session.background_tasks.iter().collect::<Vec<_>>();
    tasks.sort_by_key(|(left, _)| *left);
    for (task_id, task) in tasks {
        println!(
            "    - {} agent={} tool={} progress={}",
            task_id,
            task.agent_name,
            optional_str(&task.tool_name),
            optional_str(&task.progress_message),
        );
    }
}

fn optional_str(value: &Option<String>) -> &str {
    value
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("-")
}

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn format_duration_ms(ms: u64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        return format!("{secs}s");
    }

    let mins = secs / 60;
    if mins < 60 {
        return format!("{}m{}s", mins, secs % 60);
    }

    let hours = mins / 60;
    format!("{}h{}m", hours, mins % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_str_returns_dash_for_missing_or_blank_values() {
        assert_eq!(optional_str(&None), "-");
        assert_eq!(optional_str(&Some(String::new())), "-");
        assert_eq!(optional_str(&Some("   ".to_string())), "-");
        assert_eq!(optional_str(&Some("alice".to_string())), "alice");
    }

    #[test]
    fn single_line_collapses_all_whitespace() {
        assert_eq!(single_line("hello\n  brave\tworld"), "hello brave world");
        assert_eq!(single_line("  already   spaced  "), "already spaced");
    }

    #[test]
    fn format_duration_ms_uses_seconds_minutes_or_hours() {
        assert_eq!(format_duration_ms(0), "0s");
        assert_eq!(format_duration_ms(59_999), "59s");
        assert_eq!(format_duration_ms(60_000), "1m0s");
        assert_eq!(format_duration_ms(125_000), "2m5s");
        assert_eq!(format_duration_ms(3_600_000), "1h0m");
        assert_eq!(format_duration_ms(7_260_000), "2h1m");
    }

    use anda_core::{ByteBufB64, ToolOutput, http::RPCResponse};
    use axum::{Router, routing};
    use serde_json::{Value, json};

    fn session_summary_json(id: &str) -> Value {
        json!({
            "id": id,
            "caller": "anonymous",
            "workspace": "/tmp/ws",
            "source": "cli:/tmp/ws",
            "conversation_id": 7,
            "active_at": 1_750_000_000_000u64,
            "idle_ms": 65_000,
            "has_goal": true,
            "background_task_count": 1,
        })
    }

    fn session_state_json(id: &str) -> Value {
        json!({
            "summary": session_summary_json(id),
            "formation_context": {
                "counterparty": "alice",
                "agent": null,
                "source": "wechat:mom",
                "topic": "  ",
            },
            "goal": {
                "objective": "finish the\nrelease notes",
                "prev_objective": "draft the outline",
                "prev_evaluation": {
                    "complete": false,
                    "reason": "missing\nsections",
                    "follow_up": "add changelog",
                },
            },
            "background_tasks": {
                "task-1": {
                    "agent_name": "anda",
                    "tool_name": "shell",
                    "progress_message": "running",
                },
            },
            "submit_formation_at": 1_750_000_100_000u64,
        })
    }

    async fn spawn_sessions_gateway(kip_response: Value) -> gateway::Client {
        let output: ToolOutput<KipResponse> =
            ToolOutput::new(serde_json::from_value(kip_response).unwrap());
        let payload = ByteBufB64(serde_json::to_vec(&output).unwrap());
        let rpc: RPCResponse = Ok(payload);
        let body = serde_json::to_value(&rpc).unwrap();
        let app = Router::new().route(
            "/engine/default",
            routing::post(move || {
                let body = body.clone();
                async move { axum::Json(body) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        gateway::Client::new(format!("http://{addr}"), "token".to_string())
    }

    #[tokio::test]
    async fn run_lists_sessions_in_text_and_json() {
        let client = spawn_sessions_gateway(json!({
            "result": [session_summary_json("session-1")],
        }))
        .await;

        run(
            &client,
            SessionCommand {
                json: false,
                command: None,
            },
        )
        .await
        .unwrap();

        run(
            &client,
            SessionCommand {
                json: true,
                command: Some(SessionsSubcommand::List),
            },
        )
        .await
        .unwrap();

        // An empty session list prints the placeholder message.
        let client = spawn_sessions_gateway(json!({"result": []})).await;
        run(
            &client,
            SessionCommand {
                json: false,
                command: Some(SessionsSubcommand::List),
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn run_gets_session_state_in_text_and_json() {
        let client = spawn_sessions_gateway(json!({
            "result": session_state_json("session-1"),
        }))
        .await;

        for json_output in [false, true] {
            run(
                &client,
                SessionCommand {
                    json: json_output,
                    command: Some(SessionsSubcommand::Get {
                        session_id: "session-1".to_string(),
                    }),
                },
            )
            .await
            .unwrap();
        }

        // Minimal session state exercises the "none" print branches.
        let client = spawn_sessions_gateway(json!({
            "result": {
                "summary": session_summary_json("session-2"),
                "formation_context": null,
                "goal": null,
                "background_tasks": {},
                "submit_formation_at": 0,
            },
        }))
        .await;
        run(
            &client,
            SessionCommand {
                json: false,
                command: Some(SessionsSubcommand::Get {
                    session_id: "session-2".to_string(),
                }),
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn run_surfaces_kip_error_responses() {
        let client = spawn_sessions_gateway(json!({
            "error": {
                "code": "KIP_2001",
                "message": "sessions unavailable",
            },
        }))
        .await;

        let err = run(
            &client,
            SessionCommand {
                json: false,
                command: Some(SessionsSubcommand::List),
            },
        )
        .await
        .map(|_| ())
        .unwrap_err();
        assert!(err.to_string().contains("sessions API returned an error"));
    }
}
