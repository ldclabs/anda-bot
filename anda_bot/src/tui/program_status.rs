//! Reports the TUI's state to the terminal with the Program Status Protocol
//! (OSC 7501, <https://www.superlogical.com/rex/docs/build/program-status>), so
//! a terminal that implements it can show on a tab or session list whether
//! Anda is working, waiting for the user, done or failed.
//!
//! Reports go out without feature detection, which the protocol allows:
//! terminals must ignore an OSC they do not know, and crossterm would read a
//! detection reply as key presses.

use anda_engine::memory::ConversationStatus;
use base64::{Engine as _, engine::general_purpose::STANDARD};

use super::App;

/// The longest `msg` the protocol accepts, in bytes before encoding.
const MAX_MESSAGE_BYTES: usize = 2048;

/// One report: the TUI uses the root record, which belongs to the program in
/// the foreground of the terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ProgramStatus {
    Idle,
    Working,
    /// A task finished and the user has not been back at the keyboard since.
    Done,
    Blocked {
        kind: Option<&'static str>,
        message: String,
    },
    Error(String),
}

impl ProgramStatus {
    fn report(&self) -> String {
        let (state, kind, message) = match self {
            Self::Idle => ("idle", None, None),
            Self::Working => ("working", None, None),
            Self::Done => ("done", None, None),
            Self::Blocked { kind, message } => ("blocked", *kind, Some(message.as_str())),
            Self::Error(message) => ("error", None, Some(message.as_str())),
        };
        let mut report = format!("\x1b]7501;state={state}:app=anda");
        if let Some(kind) = kind {
            report.push_str(":kind=");
            report.push_str(kind);
        }
        if let Some(message) = message.and_then(encode_message) {
            report.push_str(":msg=");
            report.push_str(&message);
        }
        report.push_str("\x1b\\");
        report
    }
}

/// What the TUI is doing now, before a finished task counts as seen or not.
#[derive(Debug, PartialEq, Eq)]
enum Activity {
    Working,
    Blocked {
        kind: Option<&'static str>,
        message: String,
    },
    Offline,
    /// Nothing runs; `failure` holds the reason when the conversation failed.
    Resting {
        failure: Option<String>,
    },
}

fn activity(app: &App) -> Activity {
    if app.setup_required() {
        return Activity::Blocked {
            kind: None,
            message: format!("Fill in config.yaml: {}", app.setup.issues.join(", ")),
        };
    }
    if app.chatgpt_login_pending() {
        return Activity::Blocked {
            kind: Some("auth"),
            message: "Continue with ChatGPT in your browser".to_string(),
        };
    }
    if app.bootstrapping() {
        return Activity::Working;
    }
    if !app.daemon_running {
        return Activity::Offline;
    }
    // A card keeps its conversation working while it waits for the user. A
    // pending card in a conversation that stopped can no longer be answered.
    if app.chat.is_thinking()
        && !app.action_response_pending()
        && let Some(action) = app.active_pending_action()
    {
        let kind = if action.is_approval() {
            "permission"
        } else {
            "question"
        };
        return Activity::Blocked {
            kind: Some(kind),
            message: action.display_title(),
        };
    }
    if app.chat.is_thinking() || app.action_response_pending() {
        return Activity::Working;
    }
    let failure = app
        .chat
        .conversation
        .as_ref()
        .filter(|conversation| conversation.status == ConversationStatus::Failed)
        .map(|conversation| {
            conversation
                .failed_reason
                .clone()
                .unwrap_or_else(|| "The task failed".to_string())
        });
    Activity::Resting { failure }
}

/// Writes a report only when the status changes.
#[derive(Default)]
pub(super) struct ProgramStatusReporter {
    reported: Option<ProgramStatus>,
    /// A task ran since the user last pressed a key, so its end is unseen.
    unseen: bool,
}

impl ProgramStatusReporter {
    /// The user is at the keyboard, so a finished task has been seen.
    pub(super) fn seen(&mut self) {
        self.unseen = false;
    }

    /// The report to write for the TUI's current state, if it changed.
    pub(super) fn update(&mut self, app: &App) -> Option<String> {
        let status = match activity(app) {
            Activity::Working => {
                self.unseen = true;
                ProgramStatus::Working
            }
            Activity::Blocked { kind, message } => ProgramStatus::Blocked { kind, message },
            Activity::Offline => ProgramStatus::Error("Daemon connection lost".to_string()),
            Activity::Resting { failure } if self.unseen => match failure {
                Some(reason) => ProgramStatus::Error(reason),
                None => ProgramStatus::Done,
            },
            Activity::Resting { .. } => ProgramStatus::Idle,
        };
        if self.reported.as_ref() == Some(&status) {
            return None;
        }
        let report = status.report();
        self.reported = Some(status);
        Some(report)
    }

    /// Removes the record when the user quits, if one was reported.
    pub(super) fn clear(&mut self) -> Option<&'static str> {
        self.reported.take().map(|_| "\x1b]7501;state=clear\x1b\\")
    }
}

/// Whether the terminal parses escape sequences at all; a legacy Windows
/// console would print the report.
#[cfg(windows)]
pub(super) fn terminal_supported() -> bool {
    crossterm::ansi_support::supports_ansi()
}

#[cfg(not(windows))]
pub(super) fn terminal_supported() -> bool {
    true
}

/// `text` without control characters, which make a terminal drop the report,
/// cut to the protocol's limit and Base64-encoded. `None` when it is blank.
fn encode_message(text: &str) -> Option<String> {
    let text = text
        .split(|c: char| c.is_control() || c.is_whitespace())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() {
        return None;
    }
    Some(STANDARD.encode(&text[..text.floor_char_boundary(MAX_MESSAGE_BYTES)]))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use anda_core::{ContentPart, Message};
    use anda_engine::memory::Conversation;

    use super::*;
    use crate::{config::Config, gateway};

    fn ready_app() -> App {
        let client = gateway::Client::new("http://127.0.0.1:8042".to_string(), String::new());
        let mut app = App::new(PathBuf::from("."), Config::default(), client, false);
        app.daemon_running = true;
        app
    }

    fn set_status(app: &mut App, status: ConversationStatus) {
        app.chat.conversation = Some(Conversation {
            status,
            ..Default::default()
        });
        app.chat.mark_changed();
    }

    fn push_action(app: &mut App, kind: &str, title: &str) {
        app.chat.messages.push(Message {
            role: "assistant".to_string(),
            name: Some("$action".to_string()),
            content: vec![ContentPart::Action {
                name: format!("anda.{kind}"),
                payload: serde_json::json!({
                    "id": "act_1",
                    "kind": kind,
                    "title": title,
                    "status": "pending"
                }),
                recipients: None,
                signature: None,
            }],
            ..Default::default()
        });
        app.chat.mark_changed();
    }

    /// The report's pairs as `key=value`, with `msg` decoded.
    fn decode(report: &str) -> Vec<String> {
        let body = report
            .strip_prefix("\x1b]7501;")
            .and_then(|rest| rest.strip_suffix("\x1b\\"))
            .expect("an OSC 7501 report terminated by ST");
        body.split(':')
            .map(|pair| match pair.strip_prefix("msg=") {
                Some(msg) => format!(
                    "msg={}",
                    String::from_utf8(STANDARD.decode(msg).unwrap()).unwrap()
                ),
                None => pair.to_string(),
            })
            .collect()
    }

    #[test]
    fn a_task_reports_working_then_done_until_the_user_returns() {
        let mut app = ready_app();
        let mut reporter = ProgramStatusReporter::default();
        assert_eq!(
            reporter.update(&app).as_deref(),
            Some("\x1b]7501;state=idle:app=anda\x1b\\")
        );
        assert_eq!(
            reporter.update(&app),
            None,
            "an unchanged state is not resent"
        );

        set_status(&mut app, ConversationStatus::Working);
        assert_eq!(
            decode(&reporter.update(&app).unwrap()),
            ["state=working", "app=anda"]
        );
        // Typing while the agent works does not make its result seen.
        reporter.seen();
        assert_eq!(reporter.update(&app), None);

        set_status(&mut app, ConversationStatus::Idle);
        assert_eq!(
            decode(&reporter.update(&app).unwrap()),
            ["state=done", "app=anda"]
        );
        reporter.seen();
        assert_eq!(
            decode(&reporter.update(&app).unwrap()),
            ["state=idle", "app=anda"]
        );
    }

    #[test]
    fn a_restored_conversation_rests_without_an_unseen_result() {
        let mut app = ready_app();
        set_status(&mut app, ConversationStatus::Failed);
        push_action(&mut app, "choice", "Expired card");
        let mut reporter = ProgramStatusReporter::default();
        assert_eq!(
            decode(&reporter.update(&app).unwrap()),
            ["state=idle", "app=anda"]
        );
    }

    #[test]
    fn a_failure_seen_live_reports_its_reason() {
        let mut app = ready_app();
        let mut reporter = ProgramStatusReporter::default();
        set_status(&mut app, ConversationStatus::Working);
        reporter.update(&app);
        app.chat.conversation = Some(Conversation {
            status: ConversationStatus::Failed,
            failed_reason: Some("model\nrejected the request".to_string()),
            ..Default::default()
        });
        assert_eq!(
            decode(&reporter.update(&app).unwrap()),
            ["state=error", "app=anda", "msg=model rejected the request"]
        );
    }

    #[test]
    fn pending_cards_block_on_permission_or_question() {
        let mut app = ready_app();
        set_status(&mut app, ConversationStatus::Working);
        push_action(&mut app, "shell_command", "Approve shell command");
        let mut reporter = ProgramStatusReporter::default();
        assert_eq!(
            decode(&reporter.update(&app).unwrap()),
            [
                "state=blocked",
                "app=anda",
                "kind=permission",
                "msg=Approve shell command"
            ]
        );

        let mut app = ready_app();
        set_status(&mut app, ConversationStatus::Working);
        push_action(&mut app, "choice", "Pick a branch");
        assert_eq!(
            decode(&reporter.update(&app).unwrap()),
            [
                "state=blocked",
                "app=anda",
                "kind=question",
                "msg=Pick a branch"
            ]
        );
    }

    #[test]
    fn a_lost_daemon_is_an_error() {
        let mut app = ready_app();
        app.daemon_running = false;
        let report = ProgramStatusReporter::default().update(&app).unwrap();
        assert_eq!(
            decode(&report),
            ["state=error", "app=anda", "msg=Daemon connection lost"]
        );
    }

    #[test]
    fn quitting_clears_only_a_reported_record() {
        let mut reporter = ProgramStatusReporter::default();
        assert_eq!(reporter.clear(), None);
        reporter.update(&ready_app());
        assert_eq!(reporter.clear(), Some("\x1b]7501;state=clear\x1b\\"));
        assert_eq!(reporter.clear(), None);
    }

    #[test]
    fn messages_drop_control_characters_and_fit_the_limit() {
        assert_eq!(encode_message(" \x1b\x07\n\t"), None);
        let decoded = |text: &str| {
            String::from_utf8(STANDARD.decode(encode_message(text).unwrap()).unwrap()).unwrap()
        };
        assert_eq!(decoded("a\x1b[31mb\u{9b}c\r\nd"), "a [31mb c d");
        let cut = decoded(&"界".repeat(1000));
        assert!(cut.len() <= MAX_MESSAGE_BYTES && cut.len() > MAX_MESSAGE_BYTES - 3);
        assert!(cut.chars().all(|c| c == '界'));
    }
}
