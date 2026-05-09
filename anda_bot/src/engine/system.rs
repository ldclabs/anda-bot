use anda_core::{ContentPart, Message};

pub const SYSTEM_PERSON_NAME: &str = "$system";

const SYSTEM_RUNTIME_MESSAGE_PREFIX: &str = "[$system runtime message:";

pub fn system_runtime_prompt(kind: &str, body: impl AsRef<str>) -> String {
    let kind = kind.trim();
    let body = body.as_ref().trim();
    let kind = if kind.is_empty() { "notice" } else { kind };

    format!(
        "[$system runtime message: {kind}]\nThis message is from the Anda runtime, not from the external user. Treat it as operational context for the same conversation; do not attribute it to the user.\n\n{body}"
    )
}

pub fn system_user_message(prompt: String, timestamp: u64) -> Message {
    Message {
        role: "user".to_string(),
        name: Some(SYSTEM_PERSON_NAME.to_string()),
        content: vec![ContentPart::Text { text: prompt }],
        timestamp: Some(timestamp),
        ..Default::default()
    }
}

pub fn mark_system_runtime_messages(messages: &mut [Message]) {
    for message in messages {
        if message.role == "user"
            && message.name.is_none()
            && message
                .text()
                .is_some_and(|text| is_system_runtime_prompt(&text))
        {
            message.name = Some(SYSTEM_PERSON_NAME.to_string());
        }
    }
}

fn is_system_runtime_prompt(text: &str) -> bool {
    text.trim_start().starts_with(SYSTEM_RUNTIME_MESSAGE_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_runtime_prompt_identifies_runtime_source() {
        let prompt = system_runtime_prompt("compaction", "Summarize state.");

        assert!(prompt.starts_with("[$system runtime message: compaction]"));
        assert!(prompt.contains("not from the external user"));
        assert!(prompt.contains("Summarize state."));
    }

    #[test]
    fn system_user_message_uses_named_user_role() {
        let message = system_user_message("continue".to_string(), 42);

        assert_eq!(message.role, "user");
        assert_eq!(message.name.as_deref(), Some(SYSTEM_PERSON_NAME));
        assert_eq!(message.timestamp, Some(42));
    }

    #[test]
    fn mark_system_runtime_messages_tags_matching_user_messages() {
        let mut messages = vec![Message {
            role: "user".to_string(),
            content: vec![ContentPart::Text {
                text: system_runtime_prompt("background task", "done"),
            }],
            ..Default::default()
        }];

        mark_system_runtime_messages(&mut messages);

        assert_eq!(messages[0].name.as_deref(), Some(SYSTEM_PERSON_NAME));
    }
}
