mod formation;
mod maintenance;
mod recall;

use anda_engine::memory::Conversation;

pub use formation::*;
pub use maintenance::*;
pub use recall::*;

#[async_trait::async_trait]
pub trait AgentHooks: Send + Sync {
    async fn on_conversation_end(&self, agent_name: &str, conversation: &Conversation);
}
