//! Request metadata and conversation lifecycle helpers shared by the agent
//! entry points, the session runner, and startup recovery.

use anda_core::{ContentPart, Message, RequestMeta};
use anda_engine::memory::{Conversation, ConversationStatus};
use serde_json::{Map, Value};

use crate::engine::{
    is_action_message_value,
    system::{mark_special_user_messages, scoped_external_user_name},
};
use crate::util::request_meta::{keys, request_meta_extra_as};

pub(super) fn request_meta_for_conversation(
    meta: &RequestMeta,
    conversation_id: u64,
) -> RequestMeta {
    let mut meta = meta.clone();
    if conversation_id > 0 {
        meta.extra
            .insert(keys::CONVERSATION.to_string(), conversation_id.into());
    }
    meta
}

pub(super) fn conversation_extra_without_id(meta: &RequestMeta) -> Map<String, Value> {
    let mut extra = meta.extra.clone();
    extra.remove(keys::CONVERSATION);
    extra
}

pub(super) fn scoped_external_user_name_from_meta(meta: &RequestMeta) -> String {
    let source = request_meta_extra_as::<String>(meta, keys::SOURCE).unwrap_or_default();
    let sender = meta.user.as_deref().unwrap_or_default();
    let thread = request_meta_extra_as::<String>(meta, keys::THREAD)
        .map(|thread| thread.trim().to_string())
        .filter(|thread| !thread.is_empty());
    let reply_target = request_meta_extra_as::<String>(meta, keys::REPLY_TARGET)
        .map(|reply_target| reply_target.trim().to_string())
        .filter(|reply_target| !reply_target.is_empty())
        .filter(|reply_target| reply_target != sender);
    let space = thread.as_deref().or(reply_target.as_deref());

    scoped_external_user_name(&source, space, sender)
}

/// Keys that describe the single request that opened a conversation, not the
/// conversation itself. `Conversation::extra` stores the request metadata
/// verbatim, so recovery has to drop them: replaying `cron_job_id` would keep a
/// resumed session permanently marked as an unattended cron run, and replaying
/// `approval_mode` would pin it to the approval policy of a client that is no
/// longer connected. Both grant approval-free shell and MCP access to later
/// turns the user drives by hand.
const TRANSIENT_REQUEST_EXTRA_KEYS: [&str; 4] = [
    keys::CRON_JOB_ID,
    keys::CRON_JOB_NAME,
    keys::CRON_JOB_KIND,
    keys::APPROVAL_MODE,
];

pub(super) fn request_meta_from_conversation(
    conversation: &Conversation,
    source_key: &str,
) -> RequestMeta {
    let mut extra = conversation
        .extra
        .as_ref()
        .and_then(|extra| extra.as_object().cloned())
        .unwrap_or_default();
    for key in TRANSIENT_REQUEST_EXTRA_KEYS {
        extra.remove(key);
    }
    apply_source_key_to_meta_extra(&mut extra, source_key);
    extra.insert(keys::CONVERSATION.to_string(), conversation._id.into());

    RequestMeta {
        extra,
        ..Default::default()
    }
}

fn apply_source_key_to_meta_extra(extra: &mut Map<String, Value>, source_key: &str) {
    if extra.get(keys::SOURCE).is_some() {
        return;
    }

    if let Some((source, route)) = source_key.split_once(":reply_target:") {
        extra.insert(keys::SOURCE.to_string(), source.to_string().into());
        if let Some((reply_target, thread)) = route.split_once(":thread:") {
            extra.insert(
                keys::REPLY_TARGET.to_string(),
                reply_target.to_string().into(),
            );
            if !thread.is_empty() {
                extra.insert(keys::THREAD.to_string(), thread.to_string().into());
            }
        }
    } else if !source_key.is_empty() {
        extra.insert(keys::SOURCE.to_string(), source_key.to_string().into());
    }
}

pub(super) fn conversation_chat_history(conversation: &Conversation) -> Vec<Message> {
    let mut messages = conversation
        .messages
        .iter()
        .filter(|message| !is_action_message_value(message))
        .filter_map(|message| match serde_json::from_value::<Message>(message.clone()) {
            Ok(message) => Some(message),
            Err(err) => {
                log::warn!(conversation = conversation._id; "failed to parse startup conversation message: {err}");
                None
            }
        })
        .collect::<Vec<_>>();
    remove_unanswered_tool_calls(&mut messages);
    mark_special_user_messages(&mut messages);
    messages
}

fn remove_unanswered_tool_calls(messages: &mut Vec<Message>) {
    let mut pending: Vec<(usize, usize, String, Option<String>)> = Vec::new();

    for (message_idx, message) in messages.iter().enumerate() {
        for (part_idx, part) in message.content.iter().enumerate() {
            match part {
                ContentPart::ToolCall { name, call_id, .. } => {
                    pending.push((message_idx, part_idx, name.clone(), call_id.clone()));
                }
                ContentPart::ToolOutput { name, call_id, .. } => {
                    if let Some(pos) =
                        pending.iter().position(|(_, _, pending_name, pending_id)| {
                            tool_call_matches_output(
                                pending_name,
                                pending_id.as_deref(),
                                name,
                                call_id.as_deref(),
                            )
                        })
                    {
                        pending.remove(pos);
                    }
                }
                _ => {}
            }
        }
    }

    if pending.is_empty() {
        return;
    }

    let unanswered = pending
        .into_iter()
        .map(|(message_idx, part_idx, _, _)| (message_idx, part_idx))
        .collect::<std::collections::HashSet<_>>();
    for (message_idx, message) in messages.iter_mut().enumerate() {
        let mut part_idx = 0;
        message.content.retain(|_| {
            let keep = !unanswered.contains(&(message_idx, part_idx));
            part_idx += 1;
            keep
        });
    }
    messages.retain(|message| !message.content.is_empty());
}

fn tool_call_matches_output(
    pending_name: &str,
    pending_call_id: Option<&str>,
    output_name: &str,
    output_call_id: Option<&str>,
) -> bool {
    match (pending_call_id, output_call_id) {
        (Some(pending), Some(output)) => pending == output,
        (None, None) => pending_name == output_name,
        _ => false,
    }
}

pub(super) fn should_continue_conversation(status: &ConversationStatus) -> bool {
    matches!(
        status,
        ConversationStatus::Submitted | ConversationStatus::Working | ConversationStatus::Idle
    )
}

pub(super) fn is_terminal_conversation_status(status: &ConversationStatus) -> bool {
    matches!(
        status,
        ConversationStatus::Completed | ConversationStatus::Cancelled | ConversationStatus::Failed
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::system::{SYSTEM_PERSON_NAME, system_runtime_prompt};
    use serde_json::json;

    #[test]
    fn request_meta_for_conversation_sets_current_conversation() {
        let mut extra = serde_json::Map::new();
        extra.insert("conversation".to_string(), 0.into());
        extra.insert("source".to_string(), "cli:/tmp/workspace".into());
        let meta = RequestMeta {
            user: Some("alice".to_string()),
            extra,
            ..Default::default()
        };

        let meta = request_meta_for_conversation(&meta, 140);

        assert_eq!(meta.user.as_deref(), Some("alice"));
        assert_eq!(meta.get_extra_as::<u64>("conversation"), Some(140));
        assert_eq!(
            meta.get_extra_as::<String>("source"),
            Some("cli:/tmp/workspace".to_string())
        );
    }

    #[test]
    fn scoped_external_user_name_from_meta_includes_channel_and_sender() {
        let mut extra = serde_json::Map::new();
        extra.insert("source".to_string(), "wechat:mom".into());
        let meta = RequestMeta {
            user: Some("wxid_123".to_string()),
            extra,
            ..Default::default()
        };

        assert_eq!(
            scoped_external_user_name_from_meta(&meta),
            "$external_user:\"wechat:mom/wxid_123\""
        );
    }

    #[test]
    fn scoped_external_user_name_from_meta_includes_discussion_space() {
        let mut extra = serde_json::Map::new();
        extra.insert("source".to_string(), "wechat:agents".into());
        extra.insert("thread".to_string(), "room-7".into());
        extra.insert("reply_target".to_string(), "wxid_123".into());
        let meta = RequestMeta {
            user: Some("agent-a".to_string()),
            extra,
            ..Default::default()
        };

        assert_eq!(
            scoped_external_user_name_from_meta(&meta),
            "$external_user:\"wechat:agents/room-7/agent-a\""
        );
    }

    #[test]
    fn request_meta_from_conversation_recovers_route_from_source_key() {
        let conversation = Conversation {
            _id: 77,
            extra: Some(json!({"workspace": "/tmp/channels/telegram"})),
            ..Default::default()
        };

        let meta = request_meta_from_conversation(
            &conversation,
            "telegram:reply_target:chat-1:thread:topic-2",
        );

        assert_eq!(meta.get_extra_as::<u64>("conversation"), Some(77));
        assert_eq!(
            meta.get_extra_as::<String>("workspace"),
            Some("/tmp/channels/telegram".to_string())
        );
        assert_eq!(
            meta.get_extra_as::<String>("source"),
            Some("telegram".to_string())
        );
        assert_eq!(
            meta.get_extra_as::<String>("reply_target"),
            Some("chat-1".to_string())
        );
        assert_eq!(
            meta.get_extra_as::<String>("thread"),
            Some("topic-2".to_string())
        );
    }

    #[test]
    fn conversation_chat_history_marks_startup_runtime_messages() {
        let conversation = Conversation {
            _id: 88,
            messages: vec![json!(Message {
                role: "user".to_string(),
                content: vec![system_runtime_prompt("startup", "resume").into()],
                ..Default::default()
            })],
            ..Default::default()
        };

        let messages = conversation_chat_history(&conversation);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].name.as_deref(), Some(SYSTEM_PERSON_NAME));
    }

    #[test]
    fn conversation_chat_history_filters_action_messages() {
        let conversation = Conversation {
            _id: 89,
            messages: vec![
                json!({
                    "role": "assistant",
                    "name": "$action",
                    "content": [{
                        "type": "Action",
                        "name": "anda.user_choice",
                        "payload": {"id": "act_1", "status": "pending"}
                    }]
                }),
                json!(Message {
                    role: "user".to_string(),
                    content: vec!["continue".to_string().into()],
                    ..Default::default()
                }),
            ],
            ..Default::default()
        };

        let messages = conversation_chat_history(&conversation);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
    }

    #[test]
    fn conversation_extra_without_id_drops_conversation_key() {
        let mut extra = serde_json::Map::new();
        extra.insert("conversation".to_string(), 9.into());
        extra.insert("source".to_string(), "cli".into());
        let meta = RequestMeta {
            extra,
            ..Default::default()
        };

        let stripped = conversation_extra_without_id(&meta);
        assert!(!stripped.contains_key("conversation"));
        assert!(stripped.contains_key("source"));
    }

    #[test]
    fn request_meta_from_conversation_drops_transient_request_keys() {
        // A conversation opened by a cron job persists the job markers, and one
        // opened by a full-access client persists its approval mode. Replaying
        // either would keep the recovered session running without approval
        // cards for turns the user drives by hand.
        let conversation = Conversation {
            _id: 91,
            extra: Some(json!({
                "source": "telegram",
                "reply_target": "chat-1",
                "cron_job_id": 7,
                "cron_job_name": "nightly",
                "cron_job_kind": "agent",
                "approval_mode": "full_access",
            })),
            ..Default::default()
        };

        let meta = request_meta_from_conversation(&conversation, "telegram");

        assert_eq!(meta.get_extra_as::<u64>("cron_job_id"), None);
        assert_eq!(meta.get_extra_as::<String>("cron_job_name"), None);
        assert_eq!(meta.get_extra_as::<String>("cron_job_kind"), None);
        assert_eq!(meta.get_extra_as::<String>("approval_mode"), None);
        // Routing keys still have to survive so replies return to the source.
        assert_eq!(
            meta.get_extra_as::<String>("source"),
            Some("telegram".to_string())
        );
        assert_eq!(
            meta.get_extra_as::<String>("reply_target"),
            Some("chat-1".to_string())
        );
        assert_eq!(meta.get_extra_as::<u64>("conversation"), Some(91));
    }

    #[test]
    fn request_meta_from_conversation_keeps_existing_source() {
        let conversation = Conversation {
            _id: 5,
            extra: Some(json!({"source": "preset"})),
            ..Default::default()
        };
        let meta =
            request_meta_from_conversation(&conversation, "telegram:reply_target:c:thread:t");
        // The pre-existing source is preserved and route keys are not applied.
        assert_eq!(
            meta.get_extra_as::<String>("source"),
            Some("preset".to_string())
        );
        assert_eq!(meta.get_extra_as::<String>("reply_target"), None);
    }

    #[test]
    fn request_meta_from_conversation_handles_plain_and_empty_thread() {
        // Plain source key (no reply_target marker) lands in `source`.
        let plain = request_meta_from_conversation(&Conversation::default(), "lark");
        assert_eq!(
            plain.get_extra_as::<String>("source"),
            Some("lark".to_string())
        );

        // A reply target with an empty thread omits the thread key.
        let empty_thread =
            request_meta_from_conversation(&Conversation::default(), "tg:reply_target:c:thread:");
        assert_eq!(
            empty_thread.get_extra_as::<String>("reply_target"),
            Some("c".to_string())
        );
        assert_eq!(empty_thread.get_extra_as::<String>("thread"), None);

        // An empty source key inserts nothing.
        let empty = request_meta_from_conversation(&Conversation::default(), "");
        assert_eq!(empty.get_extra_as::<String>("source"), None);
    }

    #[test]
    fn conversation_chat_history_pops_trailing_tool_calls_and_skips_malformed() {
        use anda_core::ContentPart;

        let tool_call_message = Message {
            role: "assistant".to_string(),
            content: vec![ContentPart::ToolCall {
                name: "shell".to_string(),
                args: json!({}),
                call_id: Some("call-1".to_string()),
            }],
            ..Default::default()
        };
        let conversation = Conversation {
            _id: 12,
            messages: vec![
                json!(Message {
                    role: "user".to_string(),
                    content: vec![ContentPart::Text {
                        text: "hello".to_string()
                    }],
                    ..Default::default()
                }),
                json!(tool_call_message),
                // A malformed message that cannot deserialize into Message.
                json!({"role": 12345}),
            ],
            ..Default::default()
        };

        let messages = conversation_chat_history(&conversation);
        // The trailing tool-call message is dropped; the malformed one is skipped.
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text().as_deref(), Some("hello"));
    }

    #[test]
    fn conversation_chat_history_strips_unanswered_tool_calls_before_stop_message() {
        use anda_core::ContentPart;

        let conversation = Conversation {
            _id: 12,
            messages: vec![
                json!(Message {
                    role: "user".to_string(),
                    content: vec![ContentPart::Text {
                        text: "start".to_string()
                    }],
                    ..Default::default()
                }),
                json!(Message {
                    role: "assistant".to_string(),
                    content: vec![
                        ContentPart::Text {
                            text: "I will inspect it.".to_string()
                        },
                        ContentPart::ToolCall {
                            name: "shell".to_string(),
                            args: json!({"command": "pwd"}),
                            call_id: Some("call-stop".to_string()),
                        },
                    ],
                    ..Default::default()
                }),
                json!(Message {
                    role: "user".to_string(),
                    name: Some(SYSTEM_PERSON_NAME.to_string()),
                    content: vec![ContentPart::Text {
                        text: system_runtime_prompt("task stopped", "Current task stopped")
                    }],
                    ..Default::default()
                }),
            ],
            ..Default::default()
        };

        let messages = conversation_chat_history(&conversation);

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1].text().as_deref(), Some("I will inspect it."));
        assert!(messages[1].tool_calls().is_empty());
        assert_eq!(messages[2].role, "user");
    }

    #[test]
    fn conversation_status_predicates_cover_every_variant() {
        assert!(should_continue_conversation(&ConversationStatus::Submitted));
        assert!(should_continue_conversation(&ConversationStatus::Working));
        assert!(should_continue_conversation(&ConversationStatus::Idle));
        assert!(!should_continue_conversation(
            &ConversationStatus::Completed
        ));

        assert!(is_terminal_conversation_status(
            &ConversationStatus::Completed
        ));
        assert!(is_terminal_conversation_status(
            &ConversationStatus::Cancelled
        ));
        assert!(is_terminal_conversation_status(&ConversationStatus::Failed));
        assert!(!is_terminal_conversation_status(
            &ConversationStatus::Working
        ));
    }
}
