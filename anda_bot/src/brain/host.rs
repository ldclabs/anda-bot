//! Trusted embedded access. The caller comes from Engine authentication; model
//! arguments contain neither credentials nor native principals.
use anda_brain::{
    runtime_api::{
        config::{ConfigCredential, RuntimeConfig},
        *,
    },
    space::AppState,
};
use anda_cognitive_nexus::governance::AuthContext;
use anda_core::{BoxError, Principal};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone)]
pub struct Host {
    pub state: AppState,
    subjects: Arc<BTreeMap<Principal, String>>,
    pub(super) journal: Option<super::Journal>,
}
impl Host {
    pub fn new(state: AppState, config: Option<&RuntimeConfig>) -> Result<Self, BoxError> {
        let mut subjects = BTreeMap::new();
        if let Some(config) = config.and_then(|c| c.spaces.get(crate::config::ANDA_BOT_SPACE_ID)) {
            for mapping in &config.subjects {
                if let ConfigCredential::CwtSubject { subject } = &mapping.credential {
                    subjects.insert(Principal::from_text(subject)?, mapping.principal.clone());
                }
            }
        }
        Ok(Self {
            state,
            subjects: Arc::new(subjects),
            journal: None,
        })
    }

    pub fn with_journal(mut self, journal: super::Journal) -> Self {
        self.journal = Some(journal);
        self
    }

    pub async fn associate_attention(
        &self,
        page: &AttentionPage,
        caller: Principal,
        conversation: Option<u64>,
        meta: &anda_core::RequestMeta,
    ) -> Result<(), BoxError> {
        if let Some(journal) = &self.journal {
            let recipient = self.subjects.get(&caller).ok_or(RuntimeError::Forbidden)?;
            journal
                .associate_attention(page, caller, recipient, conversation, meta)
                .await?;
        }
        Ok(())
    }

    async fn runtime(
        &self,
        caller: Principal,
    ) -> Result<(Arc<MemoryRuntime>, RuntimeCaller), BoxError> {
        if caller == Principal::anonymous() {
            return Err(RuntimeError::Unauthorized.into());
        }
        let principal = self.subjects.get(&caller).ok_or(RuntimeError::Forbidden)?;
        let runtime = self
            .state
            .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
            .await?
            .memory_runtime()
            .ok_or_else(|| {
                RuntimeError::Unavailable("runtime bindings are not installed".into())
            })?;
        let mut auth = AuthContext::principal(principal);
        auth.auth_method = "bot:authenticated-engine-caller".into();
        let caller = runtime.authenticated_caller(auth)?;
        Ok((runtime, caller))
    }

    /// Whether the optional runtime bindings (the durable inbox) are installed.
    pub async fn runtime_installed(&self) -> Result<bool, BoxError> {
        Ok(self
            .state
            .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
            .await?
            .memory_runtime()
            .is_some())
    }

    pub async fn attention(
        &self,
        caller: Principal,
        query: AttentionQuery,
    ) -> Result<AttentionPage, BoxError> {
        let (runtime, caller) = self.runtime(caller).await?;
        Ok(runtime.inbox(&caller, query).await?)
    }

    pub async fn watch_record(
        &self,
        caller: Principal,
        operation_id: String,
        target: String,
        summary: String,
    ) -> Result<anda_brain::runtime_api::RecordWatch, BoxError> {
        let (runtime, caller) = self.runtime(caller).await?;
        runtime
            .create_record_watch(caller, operation_id, target, summary)
            .await
    }
    pub async fn record_watch(
        &self,
        caller: Principal,
        operation_id: &str,
    ) -> Result<anda_brain::runtime_api::RecordWatch, BoxError> {
        let (runtime, caller) = self.runtime(caller).await?;
        runtime.record_watch(&caller, operation_id).await
    }
    pub async fn cancel_record_watch(
        &self,
        caller: Principal,
        operation_id: String,
    ) -> Result<anda_brain::runtime_api::RecordWatch, BoxError> {
        let (runtime, caller) = self.runtime(caller).await?;
        runtime.cancel_record_watch(caller, operation_id).await
    }
    pub async fn respond(
        &self,
        caller: Principal,
        id: String,
        response: AttentionResponse,
    ) -> Result<ResponseReceipt, BoxError> {
        let (runtime, caller) = self.runtime(caller).await?;
        Ok(runtime.respond(caller, id, response).await?)
    }
    pub async fn status(&self, caller: Principal) -> Result<RuntimeStatus, BoxError> {
        if caller == Principal::anonymous() {
            return Err(RuntimeError::Unauthorized.into());
        }
        let space = self
            .state
            .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
            .await?;
        if space.memory_runtime().is_none() {
            return Ok(RuntimeStatus {
                supported: true,
                configured: false,
                scope: None,
                attention_enabled: false,
                actions_enabled: false,
                observation_enabled: false,
                observer_authenticated: false,
                blocked_reasons: vec!["runtime_bindings_not_installed".into()],
                visible_items: 0,
                inventory_complete: false,
                utility: Default::default(),
                trust: Default::default(),
                semantic_attention: Default::default(),
                learning: serde_json::json!({"compiled":cfg!(feature="learning"),"registered":false,"bindings_ready":false,"automatic_allowed":false}),
            });
        }
        let (runtime, caller) = self.runtime(caller).await?;
        Ok(runtime.status(&caller, true).await?)
    }
    pub async fn recall_receipt(
        &self,
        conversation: u64,
    ) -> Result<Option<anda_brain::recall_receipt::RecallReceiptRef>, BoxError> {
        self.state
            .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
            .await?
            .recall_receipts()
            .for_conversation(conversation)
            .await
    }
}
