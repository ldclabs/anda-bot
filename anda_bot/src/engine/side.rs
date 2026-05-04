use anda_engine::{
    context::{SubAgent, TOOLS_SEARCH_NAME, TOOLS_SELECT_NAME},
    extension::{
        fs::{ReadFileTool, SearchFileTool},
        note::NoteTool,
    },
};

use crate::{brain, cron};

pub fn side_agent(instructions: String) -> SubAgent {
    SubAgent {
        name: "side_agent".to_string(),
        description: "Handles one-off user requests independently from the main conversation."
            .to_string(),
        instructions: format!(
            "{instructions}\n\nBy the way, handle the user's one-off request independently from the main conversation. Do not assume hidden context from the main conversation. Use the available read-only and knowledge tools when useful, keep the answer focused, and avoid changing files or long-running state."
        ),
        tools: vec![
            brain::Client::NAME.to_string(),
            NoteTool::NAME.to_string(),
            TOOLS_SEARCH_NAME.to_string(),
            TOOLS_SELECT_NAME.to_string(),
            ReadFileTool::NAME.to_string(),
            SearchFileTool::NAME.to_string(),
            cron::ListCronJobsTool::NAME.to_string(),
            cron::ListCronRunsTool::NAME.to_string(),
        ],
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {}
