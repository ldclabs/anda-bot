//! Host routing associations for the native durable inbox. These records are an
//! audit index, never an execution queue: Brain owns Decisions and Attempts.
use super::{AttentionPage, Journal};
use anda_core::{BoxError, Principal, RequestMeta};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttentionAssociation {
    pub scope: anda_cognitive_nexus::attention::RuntimeScope,
    pub caller: String,
    pub recipient_principal: String,
    pub bot_conversation: Option<u64>,
    pub channel: Option<String>,
    pub reply_target: Option<String>,
    pub thread: Option<String>,
    pub item: anda_brain::runtime_api::AttentionItem,
    pub seen_at: u64,
}

impl Journal {
    pub async fn associate_attention(
        &self,
        page: &AttentionPage,
        caller: Principal,
        recipient: &str,
        conversation: Option<u64>,
        meta: &RequestMeta,
    ) -> Result<(), BoxError> {
        use crate::util::request_meta::{keys, request_meta_extra_as};
        for item in &page.items {
            let row = AttentionAssociation {
                scope: page.scope.clone(),
                caller: caller.to_string(),
                recipient_principal: recipient.into(),
                bot_conversation: conversation,
                channel: request_meta_extra_as(meta, keys::SOURCE),
                reply_target: request_meta_extra_as(meta, keys::REPLY_TARGET),
                thread: request_meta_extra_as(meta, keys::THREAD),
                item: item.clone(),
                seen_at: anda_engine::unix_ms(),
            };
            let digest = anda_cognitive_nexus::content_digest(
                &serde_json::json!({"scope":row.scope,"caller":row.caller,"conversation":conversation,"channel":row.channel,"reply_target":row.reply_target,"thread":row.thread,"wake":item.wake_ref}),
            )?;
            self.write(&format!("attention/{}", &digest[7..]), &row)
                .await?;
        }
        Ok(())
    }
}
