use anda_core::{Agent, BoxError, Message, Usage};
use anda_engine::context::{AgentCtx, CompletionRunner, SubAgent};
use serde::{Deserialize, Serialize};
use serde_json::json;

const EVALUATION_HISTORY_LIMIT: usize = 21;

#[derive(Clone)]
pub struct GoalState {
    supervisor: SubAgent,
    objective: String,
    prev_objective: Option<String>,
    prev_evaluation: Option<GoalEvaluation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GoalEvaluation {
    #[serde(default)]
    pub complete: bool,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub follow_up: String,
}

pub struct GoalProgressCheck {
    pub action: GoalAction,
    pub usage: Usage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoalAction {
    Complete,
    Continue(String),
}

pub fn supervisor_agent() -> SubAgent {
    SubAgent {
        name: "supervisor_agent".to_string(),
        description: "Evaluates the progress of a long-running objective and provides follow-up instructions when necessary."
            .to_string(),
        instructions: "You are a goal supervisor. Evaluate whether the main agent has completed the user's long-running objective. Be strict about observable completion, but do not invent extra requirements. Return only JSON matching the schema. If the goal is incomplete, provide one concise follow_up instruction that helps the main agent continue from the current state."
		.to_string(),
        output_schema: Some(json!({
            "type": "object",
            "properties": {
                "complete": {
                    "type": "boolean",
                    "description": "Whether the objective is completed."
                },
                "reason": {
                    "type": "string",
                    "description": "Brief reason for the decision."
                },
                "follow_up": {
                    "type": "string",
                    "description": "Instruction for the main agent when complete is false. Empty when complete is true."
                }
            },
            "required": ["complete", "reason", "follow_up"],
            "additionalProperties": false
        })),
        ..Default::default()
    }
}

impl GoalState {
    pub fn new(objective: String) -> Self {
        Self {
            supervisor: supervisor_agent(),
            objective,
            prev_objective: None,
            prev_evaluation: None,
        }
    }

    pub fn update_objective(&mut self, new_objective: String) {
        self.prev_objective = Some(self.objective.clone());
        self.objective = new_objective;
    }

    pub async fn check_progress(
        &mut self,
        runner: &CompletionRunner,
        ctx: &AgentCtx,
    ) -> Result<GoalProgressCheck, BoxError> {
        let messages = runner.chat_history();
        let prompt = self.evaluation_prompt(messages)?;
        let output = self
            .supervisor
            .run(
                ctx.child(&self.supervisor.name, &self.supervisor.name)?,
                prompt,
                vec![],
            )
            .await?;
        let usage = output.usage.clone();
        if let Some(reason) = output.failed_reason {
            return Err(reason.into());
        }

        let evaluation = parse_goal_evaluation(&output.content)?;
        let action = if evaluation.complete {
            GoalAction::Complete
        } else {
            GoalAction::Continue(continuation_prompt(&self.objective, &evaluation))
        };
        self.prev_evaluation = Some(evaluation);
        Ok(GoalProgressCheck { action, usage })
    }

    fn evaluation_prompt(&self, messages: &[Message]) -> Result<String, serde_json::Error> {
        let start = messages.len().saturating_sub(EVALUATION_HISTORY_LIMIT);
        let recent_messages = messages
            .iter()
            .skip(start)
            .map(|m| {
                let mut msg = m.clone();
                msg.prune_content();
                msg
            })
            .collect::<Vec<_>>();

        let mut prompt = format!("Objective:\n{}", serde_json::to_string(&self.objective)?);
        if let Some(prev_objective) = &self.prev_objective {
            prompt.push_str(&format!(
                "\n\nPrevious objective:\n{prev_objective}",
                prev_objective = serde_json::to_string(prev_objective)?
            ));
        }
        if let Some(prev_evaluation) = &self.prev_evaluation {
            prompt.push_str(&format!(
                "\n\nPrevious evaluation:\n{prev_evaluation}",
                prev_evaluation = serde_json::to_string(prev_evaluation)?
            ));
        }

        Ok(format!(
            "{prompt}\n\nRecent conversation history:\n{history}\n\n---\n\nDecide whether the objective is complete. If it is not complete, write the next follow-up instruction for the main agent.",
            history = serde_json::to_string(&recent_messages)?
        ))
    }
}

fn continuation_prompt(objective: &str, evaluation: &GoalEvaluation) -> String {
    let follow_up = if evaluation.follow_up.trim().is_empty() {
        format!(
            "Continue working toward this objective:\n{}",
            serde_json::to_string(objective).unwrap_or_else(|_| objective.to_string())
        )
    } else {
        format!(
            "Continue working toward this objective:\n{}\n\nFollow-up instruction from the supervisor:\n{}",
            serde_json::to_string(objective).unwrap_or_else(|_| objective.to_string()),
            evaluation.follow_up.trim()
        )
    };

    let reason = evaluation.reason.trim();
    if reason.is_empty() {
        follow_up
    } else {
        format!("Goal supervisor review: {reason}\n\n---\n\n{follow_up}",)
    }
}

fn parse_goal_evaluation(content: &str) -> Result<GoalEvaluation, BoxError> {
    let trimmed = content.trim();
    match serde_json::from_str::<GoalEvaluation>(trimmed) {
        Ok(evaluation) => Ok(evaluation),
        Err(original_err) => {
            let Some(start) = trimmed.find('{') else {
                return Err(format!("failed to parse goal evaluation JSON: {original_err}").into());
            };
            let Some(end) = trimmed.rfind('}') else {
                return Err(format!("failed to parse goal evaluation JSON: {original_err}").into());
            };
            if start >= end {
                return Err(format!("failed to parse goal evaluation JSON: {original_err}").into());
            }

            serde_json::from_str::<GoalEvaluation>(&trimmed[start..=end]).map_err(|err| {
                format!(
                    "failed to parse goal evaluation JSON: {err}; original parse error: {original_err}"
                )
                .into()
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_prompt_uses_fallback_when_follow_up_is_empty() {
        let evaluation = GoalEvaluation {
            complete: false,
            reason: "Need more verification".to_string(),
            follow_up: "  ".to_string(),
        };

        let prompt = continuation_prompt("ship it", &evaluation);

        assert!(prompt.contains("Goal supervisor review: Need more verification"));
        assert!(prompt.contains("Continue working toward this objective:"));
        assert!(prompt.contains("\"ship it\""));
    }

    #[test]
    fn parse_goal_evaluation_accepts_plain_json() {
        let evaluation = parse_goal_evaluation(
            r#"{"complete":false,"reason":"not done","follow_up":"keep going"}"#,
        )
        .expect("evaluation should parse");

        assert!(!evaluation.complete);
        assert_eq!(evaluation.reason, "not done");
        assert_eq!(evaluation.follow_up, "keep going");
    }

    #[test]
    fn parse_goal_evaluation_accepts_json_with_surrounding_text() {
        let evaluation = parse_goal_evaluation(
            "```json\n{\"complete\":true,\"reason\":\"done\",\"follow_up\":\"\"}\n```",
        )
        .expect("evaluation should parse");

        assert!(evaluation.complete);
        assert_eq!(evaluation.reason, "done");
        assert!(evaluation.follow_up.is_empty());
    }
}
