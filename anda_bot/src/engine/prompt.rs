use anda_engine::extension::skill::SkillManager;

use super::skill_library::SkillLibrary;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PromptCommand {
    #[default]
    Ping,
    // Ordinary text, including '/loop' (case-insensitive), which only has to
    // carry a prompt: interval and recurrence interpretation is left to the
    // model so localized time expressions can be handled naturally.
    Plain {
        prompt: String,
    },
    // '/goal', case-insensitive.
    // Intended for long-running tasks. When the main agent becomes idle after a turn,
    // a dedicated goal subagent evaluates whether the task is complete and can resume
    // the main agent via runner.follow_up when more work is needed.
    Goal {
        prompt: String,
    },
    // '/side' | '/btw', case-insensitive.
    // Runs the user's prompt in a separate subagent with a limited tool set, including
    // brain. It does not share context with the main agent and does not create a
    // conversation, so it is useful for handling temporary side requests without
    // interrupting the main agent's flow.
    Side {
        prompt: String,
    },
    // '/steer', case-insensitive.
    // Stops the next tool calls and uses a new prompt to redirect the model's
    // reasoning, typically to correct mistakes or adjust strategy instead of
    // continuing down the current path.
    Steer {
        prompt: String,
    },
    // '/skill', case-insensitive, followed by the skill name and prompt.
    // '$skill-name prompt' is a shorthand for the same behavior.
    // Routes the prompt through the named skill: a skill declaring
    // `execution: subagent` is delegated to its callable, and an inline one is
    // read with `skills_manager` and followed in this conversation.
    Skill {
        skill: String,
        prompt: String,
    },
    // '/stop', case-insensitive, with an optional reason.
    // Stops the current in-flight task while keeping the conversation runner idle
    // and reusable for later input.
    Stop {
        reason: String,
    },
    // '/cancel', case-insensitive, with an optional reason.
    // Cancels the active conversation runner; the reason becomes the
    // failed_reason.
    Cancel {
        reason: String,
    },
    // '/new' | '/clear', case-insensitive.
    // Starts a new conversation, completing the current one if it exists, and
    // optionally uses the provided prompt as the new conversation's first message.
    New {
        prompt: Option<String>,
    },
    Invalid {
        reason: String,
    },
}

impl From<String> for PromptCommand {
    fn from(prompt: String) -> Self {
        let trimmed = prompt.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("/ping") {
            return Self::Ping;
        }

        if let Some(rest) = trimmed.strip_prefix('$') {
            return match split_skill(rest) {
                Some(skill) => Self::Skill {
                    skill: skill.to_string(),
                    prompt: trimmed.to_string(),
                },
                None => Self::Plain {
                    prompt: trimmed.to_string(),
                },
            };
        }

        let Some(stripped) = trimmed.strip_prefix('/') else {
            return Self::Plain { prompt };
        };
        let command_end = stripped.find(char::is_whitespace).unwrap_or(stripped.len());
        let command = stripped[..command_end].to_ascii_lowercase();
        let rest = stripped[command_end..].trim();
        let full = || trimmed.to_string();

        match command.as_str() {
            "goal" | "loop" | "side" | "btw" | "steer" if rest.is_empty() => Self::Invalid {
                reason: format!("/{command} requires a prompt"),
            },
            "goal" => Self::Goal { prompt: full() },
            "side" | "btw" => Self::Side { prompt: full() },
            "steer" => Self::Steer { prompt: full() },
            "skill" => match split_skill(rest) {
                Some(skill) => Self::Skill {
                    skill: skill.to_string(),
                    prompt: full(),
                },
                None => Self::Invalid {
                    reason: if rest.is_empty() {
                        "/skill requires a skill name"
                    } else {
                        "/skill requires a prompt after the skill name"
                    }
                    .to_string(),
                },
            },
            "stop" => Self::Stop {
                reason: rest.to_string(),
            },
            "cancel" => Self::Cancel {
                reason: rest.to_string(),
            },
            "new" | "clear" => Self::New {
                prompt: (!rest.is_empty()).then(full),
            },
            // '/loop' and unknown commands, such as a '/tmp/...' path.
            _ => Self::Plain { prompt: full() },
        }
    }
}

/// The skill name of `name prompt`, the text after `/skill` or `$`, when both
/// parts are present.
fn split_skill(input: &str) -> Option<&str> {
    let (skill, prompt) = input.split_once(char::is_whitespace)?;
    (!skill.is_empty() && !prompt.trim().is_empty()).then_some(skill)
}

/// Builds the runtime instruction a `/skill` command turns into, plus the
/// callable name when the skill opted into subagent execution.
///
/// Only a skill declaring `execution: subagent` is dispatchable; the rest run
/// inline, which means the agent reads SKILL.md through `skills_manager` and
/// follows it in this conversation, so the instruction has to say which.
/// `None` when no active skill has that name: the prompt then reaches the
/// model as typed, since `$HOME ...` names a variable far more often than a
/// skill.
pub fn skill_command_directive(
    skills: &SkillLibrary,
    skill: &str,
) -> Option<(Option<String>, String)> {
    if let Some(subagent) = skills.skill_subagent(skill) {
        let directive = format!(
            "Use the {} skill subagent to handle this request",
            subagent.name
        );
        return Some((Some(subagent.name), directive));
    }
    skills.has_active_skill(skill).then(|| {
        (
            None,
            format!(
                "Use the {skill} skill to handle this request: read it with {} and follow it in this conversation",
                SkillManager::NAME
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_command_parses_known_commands() {
        assert_eq!(PromptCommand::from("".to_string()), PromptCommand::Ping);
        assert_eq!(
            PromptCommand::from(" /GOAL ship the feature ".to_string()),
            PromptCommand::Goal {
                prompt: "/GOAL ship the feature".to_string()
            }
        );
        assert_eq!(
            PromptCommand::from("/btw what is my status?".to_string()),
            PromptCommand::Side {
                prompt: "/btw what is my status?".to_string()
            }
        );
        assert_eq!(
            PromptCommand::from("/skill frontend-design polish this".to_string()),
            PromptCommand::Skill {
                skill: "frontend-design".to_string(),
                prompt: "/skill frontend-design polish this".to_string()
            }
        );
        assert_eq!(
            PromptCommand::from("$frontend-design polish this".to_string()),
            PromptCommand::Skill {
                skill: "frontend-design".to_string(),
                prompt: "$frontend-design polish this".to_string()
            }
        );
        assert_eq!(
            PromptCommand::from("/stop because it is wrong".to_string()),
            PromptCommand::Stop {
                reason: "because it is wrong".to_string()
            }
        );
        assert_eq!(
            PromptCommand::from("/STOP".to_string()),
            PromptCommand::Stop {
                reason: String::new()
            }
        );
        assert_eq!(
            PromptCommand::from("/cancel because it is wrong".to_string()),
            PromptCommand::Cancel {
                reason: "because it is wrong".to_string()
            }
        );
        assert_eq!(
            PromptCommand::from("/new fresh start".to_string()),
            PromptCommand::New {
                prompt: Some("/new fresh start".to_string())
            }
        );
        assert_eq!(
            PromptCommand::from("/clear".to_string()),
            PromptCommand::New { prompt: None }
        );
    }

    #[test]
    fn prompt_command_keeps_loop_and_unknown_slash_text_plain() {
        for text in [
            "/loop 5m /side check status",
            "/loop 每5分钟 /side 检查状态",
            "/tmp/workspace path",
            "/unknown command",
        ] {
            assert_eq!(
                PromptCommand::from(text.to_string()),
                PromptCommand::Plain {
                    prompt: text.to_string()
                }
            );
        }
    }

    #[test]
    fn prompt_command_rejects_missing_required_arguments() {
        for command in ["/goal", "/loop", "/side", "/BTW", "/steer"] {
            assert_eq!(
                PromptCommand::from(command.to_string()),
                PromptCommand::Invalid {
                    reason: format!("{} requires a prompt", command.to_lowercase())
                }
            );
        }
        assert_eq!(
            PromptCommand::from("/skill".to_string()),
            PromptCommand::Invalid {
                reason: "/skill requires a skill name".to_string()
            }
        );
        assert_eq!(
            PromptCommand::from("/skill frontend-design".to_string()),
            PromptCommand::Invalid {
                reason: "/skill requires a prompt after the skill name".to_string()
            }
        );
        // A bare or name-only `$` is ordinary text.
        for text in ["$", "$frontend-design", "$ frontend-design polish"] {
            assert_eq!(
                PromptCommand::from(text.to_string()),
                PromptCommand::Plain {
                    prompt: text.to_string()
                }
            );
        }
    }

    #[test]
    fn prompt_command_parses_remaining_aliases() {
        assert!(matches!(
            PromptCommand::from("/ping".to_string()),
            PromptCommand::Ping
        ));
        assert!(matches!(
            PromptCommand::from("/side check the weather".to_string()),
            PromptCommand::Side { .. }
        ));
        assert!(matches!(
            PromptCommand::from("/steer focus on tests".to_string()),
            PromptCommand::Steer { .. }
        ));
    }

    #[tokio::test]
    async fn skill_directive_routes_only_active_skills() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("skills").join("frontend-design");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: frontend-design\ndescription: Polish UI\n---\n\n# frontend-design\n",
        )
        .unwrap();
        let skills = SkillLibrary::for_test(home.path().to_path_buf());
        skills.reload().await.unwrap();

        let (callable, directive) = skill_command_directive(&skills, "frontend-design").unwrap();
        assert!(callable.is_none());
        assert!(directive.contains("read it with skills_manager"));

        // `$HOME is unset` has the shape of a skill command but names no skill,
        // so it gets no directive and reaches the model as typed.
        assert_eq!(
            PromptCommand::from("$HOME is unset".to_string()),
            PromptCommand::Skill {
                skill: "HOME".to_string(),
                prompt: "$HOME is unset".to_string()
            }
        );
        assert!(skill_command_directive(&skills, "HOME").is_none());
    }
}
