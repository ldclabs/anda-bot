use anda_engine::{
    extension::{
        fs::{ReadFileTool, SearchFileTool},
        shell::ShellTool,
    },
    subagent::SubAgent,
};

use crate::{brain, cron};

static SIDE_INSTRUCTIONS: &str = include_str!("../../assets/SideInstructions.md");

/// The side agent's whole allowlist. It has no discovery tool: the runner also
/// admits any tool found through `tools_select`, which would reopen the write
/// tools a side request must not have.
pub fn side_tool_names() -> Vec<String> {
    vec![
        brain::Client::NAME.to_string(),
        ShellTool::NAME.to_string(),
        crate::engine::agent::SHELL_SESSION_NAME.to_string(),
        ReadFileTool::NAME.to_string(),
        SearchFileTool::NAME.to_string(),
        cron::ListCronJobsTool::NAME.to_string(),
        cron::ListCronRunsTool::NAME.to_string(),
    ]
}

pub fn side_agent(instructions: String) -> SubAgent {
    SubAgent {
        name: "side_agent".to_string(),
        description:
            "Handles one-off read-only user requests independently from the main conversation."
                .to_string(),
        instructions: format!("{instructions}\n\n{SIDE_INSTRUCTIONS}"),
        tools: side_tool_names(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anda_engine::context::TOOLS_SELECT_NAME;

    #[test]
    fn side_agent_prompt_is_independent_and_read_only() {
        let agent = side_agent("base instructions".to_string());

        assert!(agent.instructions.starts_with("base instructions"));
        assert!(agent.instructions.contains("Do not assume hidden context"));
        assert!(agent.instructions.contains("available read-only tools"));
        assert!(agent.instructions.contains("Do not change files"));
        assert!(agent.instructions.contains("Keep the answer focused"));
    }

    #[test]
    fn side_agent_cannot_discover_tools_beyond_its_allowlist() {
        let agent = side_agent(String::new());

        assert_eq!(agent.tools, side_tool_names());
        assert!(!agent.tools.iter().any(|tool| tool == TOOLS_SELECT_NAME));
        assert!(
            !agent
                .tools
                .iter()
                .any(|tool| tool == "write_file" || tool == "apply_patch")
        );
    }
}
