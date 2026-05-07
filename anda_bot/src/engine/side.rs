use anda_engine::{
    context::TOOLS_SELECT_NAME,
    extension::{
        fs::{ReadFileTool, SearchFileTool},
        note::NoteTool,
        shell::ShellTool,
    },
    subagent::SubAgent,
};

use crate::{brain, cron};

static SIDE_INSTRUCTIONS: &str = include_str!("../../assets/SideInstructions.md");

pub fn side_agent(instructions: String) -> SubAgent {
    SubAgent {
        name: "side_agent".to_string(),
        description:
            "Handles one-off read-only user requests independently from the main conversation."
                .to_string(),
        instructions: format!("{instructions}\n\n{SIDE_INSTRUCTIONS}"),
        tools: vec![
            brain::Client::NAME.to_string(),
            NoteTool::NAME.to_string(),
            TOOLS_SELECT_NAME.to_string(),
            ShellTool::NAME.to_string(),
            ReadFileTool::NAME.to_string(),
            SearchFileTool::NAME.to_string(),
            cron::ListCronJobsTool::NAME.to_string(),
            cron::ListCronRunsTool::NAME.to_string(),
        ],
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_agent_prompt_is_independent_and_read_only() {
        let agent = side_agent("base instructions".to_string());

        assert!(agent.instructions.starts_with("base instructions"));
        assert!(agent.instructions.contains("Do not assume hidden context"));
        assert!(agent.instructions.contains("available read-only tools"));
        assert!(agent.instructions.contains("Do not change files"));
        assert!(agent.instructions.contains("Keep the answer focused"));
    }
}
