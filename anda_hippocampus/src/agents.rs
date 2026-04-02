mod formation;
mod maintenance;
mod recall;

use anda_db::schema::DocumentId;
use anda_engine::memory::Conversation;

pub use formation::*;
pub use maintenance::*;
pub use recall::*;

#[async_trait::async_trait]
pub trait AgentHook: Send + Sync {
    async fn on_conversation_end(&self, agent_name: &str, conversation: &Conversation);
    async fn try_start_formation(&self);
    async fn try_start_maintenance(&self, formation_id: DocumentId) -> Option<DocumentId>;
}

pub static SYSTEM_PROMPT_DYNAMIC_BOUNDARY: &str = "__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__";
