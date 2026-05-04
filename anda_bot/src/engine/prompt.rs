use anda_core::{Document, Documents, Resource, select_resources};
use anda_engine::{
    context::{SubAgent, SubAgentSet},
    extension::skill::{SkillManager, normalise_skill_agent_name},
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PromptCommand {
    #[default]
    Ping,
    Plain {
        prompt: String,
    },
    // '/goal' | '/loop', case insensitive
    // 长程任务，实现思路：1. 主 agent 处理完当前轮次的任务进入 idle 状态后，一个专门的 subagent（goal agent）评估任务是否完成，如果没完成则根据评估结果用 runner.follow_up 让主 agent 继续；2. 进入每轮 idle 状态时，需要判断 runner.current_usage().input_tokens 是否大于 ctx.model.max_output 的 1/2（可能还需要配合其它条件，比如 runner.turns() >= 42 也触发？），是则告知主 agent 压缩当前任务进展，运行时用压缩结果创建一个新的 conversation 继续处理，新的 conversation 是原 conversation 的 child。
    Goal {
        prompt: String,
    },
    // '/side' | '/btw', case insensitive
    // side 模式会使用一个单独的 subagent 来处理用户的 prompt。该 subagent 仅能使用包括 brain 在内的少数工具，且与主 agent 之间没有上下文共享，不会产生 conversation。适用于用户想要在不打断主 agent 思路的情况下，临时处理一些问题。
    Side {
        prompt: String,
    },
    // '/steer', case insensitive
    // steer 会让模型停止接下来的工具调用，转而使用新的 prompt 来引导模型调整思路或者修正错误，而不是继续沿着原来的思路往下走。
    Steer {
        prompt: String,
    },
    // '/skill', case insensitive, followed by the skill name and the prompt。通过 skill name 来引导模型使用特定的 skill （subagent）处理 prompt。
    Skill {
        skill: String,
        prompt: String,
    },
    // '/stop' | '/cancel', case insensitive. 直接 cancel，如果 prompt 存在成为 failed_reason
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
            "goal" | "loop" => required_prompt_command(command, rest, |prompt| Self::Goal {
                prompt: prompt.to_string(),
            }),
            "side" | "btw" => required_prompt_command(command, rest, |prompt| Self::Side {
                prompt: prompt.to_string(),
            }),
            "steer" => required_prompt_command(command, rest, |prompt| Self::Steer {
                prompt: prompt.to_string(),
            }),
            "skill" => parse_skill_command(rest),
            "stop" | "cancel" => Self::Stop {
                prompt: (!rest.is_empty()).then(|| rest.to_string()),
            },
            _ => Self::Plain { prompt },
        }
    }
}

pub fn skill_subagent(skill_manager: &SkillManager, skill: &str) -> Option<SubAgent> {
    skill_manager.get_lowercase(&normalise_skill_agent_name(skill))
}

pub fn prompt_with_resources(prompt: String, resources: &mut Vec<Resource>) -> String {
    let user_resources = text_resource_documents(resources);
    if user_resources.is_empty() {
        prompt
    } else {
        format!(
            "{prompt}\n\n{}",
            Documents::new("attachments".to_string(), user_resources)
        )
    }
}

pub fn text_resource_documents(resources: &mut Vec<Resource>) -> Vec<Document> {
    let res = select_resources(resources, &["text".to_string(), "md".to_string()]);
    let mut user_resources: Vec<Document> = Vec::with_capacity(res.len());
    for resource in res {
        if let Some(content) = resource
            .blob
            .and_then(|blob| String::from_utf8(blob.0).ok())
        {
            user_resources.push(Document::from_text(
                resource._id.to_string().as_str(),
                &content,
            ));
        }
    }

    user_resources
}

fn required_prompt_command<F>(command: &str, rest: &str, build: F) -> PromptCommand
where
    F: FnOnce(&str) -> PromptCommand,
{
    if rest.is_empty() {
        PromptCommand::Invalid {
            reason: format!("/{command} requires a prompt"),
        }
    } else {
        build(rest)
    }
}

fn parse_skill_command(rest: &str) -> PromptCommand {
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
        prompt: prompt.to_string(),
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
                prompt: "ship the feature".to_string()
            }
        );
        assert_eq!(
            PromptCommand::from("/btw what is my status?".to_string()),
            PromptCommand::Side {
                prompt: "what is my status?".to_string()
            }
        );
        assert_eq!(
            PromptCommand::from("/skill frontend-design polish this".to_string()),
            PromptCommand::Skill {
                skill: "frontend-design".to_string(),
                prompt: "polish this".to_string()
            }
        );
        assert_eq!(
            PromptCommand::from("/stop because it is wrong".to_string()),
            PromptCommand::Stop {
                prompt: Some("because it is wrong".to_string())
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
