use anda_engine::{
    extension::skill::{SkillManager, normalise_skill_agent_name},
    subagent::{SubAgent, SubAgentSet},
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PromptCommand {
    #[default]
    Ping,
    Plain {
        prompt: String,
    },
    // '/goal' | '/loop', case-insensitive.
    // Intended for long-running tasks. When the main agent becomes idle after a turn,
    // a dedicated goal subagent evaluates whether the task is complete and can resume
    // the main agent via runner.follow_up when more work is needed.
    // If the context grows too large, such as input_tokens > ctx.model.context_window / 2
    // (possibly combined with another threshold like runner.turns() >= 81), the main
    // agent should summarize its current progress and continue in a new child
    // conversation created from that summary.
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
    // Uses the provided skill name to route the prompt to a specific skill-based
    // subagent.
    Skill {
        skill: String,
        prompt: String,
    },
    // '/stop' | '/cancel', case-insensitive.
    // Cancels immediately. If a prompt is provided, it becomes the failed_reason.
    Stop {
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

        let Some(stripped) = trimmed.strip_prefix('/') else {
            return Self::Plain { prompt };
        };
        let command_end = stripped.find(char::is_whitespace).unwrap_or(stripped.len());
        let command = &stripped[..command_end];
        let rest = stripped[command_end..].trim();

        match command.to_ascii_lowercase().as_str() {
            "goal" | "loop" => {
                required_prompt_command(command, rest, trimmed, |prompt| Self::Goal {
                    prompt: prompt.to_string(),
                })
            }
            "side" | "btw" => {
                required_prompt_command(command, rest, trimmed, |prompt| Self::Side {
                    prompt: prompt.to_string(),
                })
            }
            "steer" => required_prompt_command(command, rest, trimmed, |prompt| Self::Steer {
                prompt: prompt.to_string(),
            }),
            "skill" => parse_skill_command(rest, trimmed),
            "stop" | "cancel" => Self::Stop {
                prompt: (!trimmed.is_empty()).then(|| trimmed.to_string()),
            },
            _ => Self::Plain { prompt },
        }
    }
}

pub fn skill_subagent(skill_manager: &SkillManager, skill: &str) -> Option<SubAgent> {
    skill_manager.get_lowercase(&normalise_skill_agent_name(
        skill.strip_prefix("skill_").unwrap_or(skill),
    ))
}

fn required_prompt_command<F>(
    command: &str,
    rest: &str,
    full_prompt: &str,
    build: F,
) -> PromptCommand
where
    F: FnOnce(&str) -> PromptCommand,
{
    if rest.is_empty() {
        PromptCommand::Invalid {
            reason: format!("/{command} requires a prompt"),
        }
    } else {
        build(full_prompt)
    }
}

fn parse_skill_command(rest: &str, full_prompt: &str) -> PromptCommand {
    let mut parts = rest.splitn(2, char::is_whitespace);
    let skill = parts.next().unwrap_or_default().trim();
    let prompt = parts.next().unwrap_or_default().trim();
    if skill.is_empty() {
        return PromptCommand::Invalid {
            reason: "/skill requires a skill name".to_string(),
        };
    }
    if prompt.is_empty() {
        return PromptCommand::Invalid {
            reason: "/skill requires a prompt after the skill name".to_string(),
        };
    }

    PromptCommand::Skill {
        skill: skill.to_string(),
        prompt: full_prompt.to_string(),
    }
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
            PromptCommand::from("/stop because it is wrong".to_string()),
            PromptCommand::Stop {
                prompt: Some("/stop because it is wrong".to_string())
            }
        );
    }

    #[test]
    fn prompt_command_keeps_unknown_slash_text_plain() {
        assert_eq!(
            PromptCommand::from("/tmp/workspace path".to_string()),
            PromptCommand::Plain {
                prompt: "/tmp/workspace path".to_string()
            }
        );
    }

    #[test]
    fn prompt_command_rejects_missing_required_arguments() {
        assert!(matches!(
            PromptCommand::from("/goal".to_string()),
            PromptCommand::Invalid { .. }
        ));
        assert!(matches!(
            PromptCommand::from("/skill frontend-design".to_string()),
            PromptCommand::Invalid { .. }
        ));
    }
}
