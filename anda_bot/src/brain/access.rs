//! Coherence for the Bot's Notes when its native memory changes.
use super::{Host, Journal};
use anda_core::{BoxError, RequestMeta, Tool};
use anda_engine::{
    context::BaseCtx,
    engine::EngineRef,
    extension::note::{NoteArgs, NoteTool},
};
use std::sync::Arc;

#[derive(Clone)]
pub struct MemoryEpoch(pub u64);

pub struct MemoryAccess {
    pub gate: tokio::sync::Mutex<()>,
    pub host: Host,
    journal: Journal,
    engine: Arc<EngineRef>,
    owner: anda_core::Principal,
}
impl MemoryAccess {
    pub fn keep_engine_alive(&self) -> Result<Arc<anda_engine::engine::Engine>, BoxError> {
        self.engine
            .get()
            .ok_or_else(|| "Memory Notes controller is unavailable".into())
    }
    pub fn new(
        host: Host,
        journal: Journal,
        engine: Arc<EngineRef>,
        owner: anda_core::Principal,
    ) -> Self {
        Self {
            gate: tokio::sync::Mutex::new(()),
            host,
            journal,
            engine,
            owner,
        }
    }
    pub async fn synchronize(&self) -> Result<(), BoxError> {
        let _guard = self.gate.lock().await;
        self.synchronize_locked().await.map(|_| ())
    }
    /// The caller holds gate across capture/read or native commit/Notes reset.
    pub async fn synchronize_locked(&self) -> Result<u64, BoxError> {
        let space = self
            .host
            .state
            .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
            .await?;
        if !space.product_available() {
            return Err("memory_change_pending".into());
        }
        let epoch = space.product_epoch();
        let saved = self
            .journal
            .read::<u64>("notes-epoch/v1")
            .await?
            .unwrap_or(0);
        if saved != epoch {
            self.reset_notes_locked().await?;
            self.journal.write("notes-epoch/v1", &epoch).await?;
        }
        Ok(epoch)
    }

    /// Recording repairs change the Nexus without advancing the product
    /// epoch, so their completion must explicitly clear Bot Notes too.
    /// The caller holds `gate` across the change and this reset.
    pub(super) async fn reset_notes_locked(&self) -> Result<(), BoxError> {
        let engine = self.keep_engine_alive()?;
        let ctx = engine.ctx_with(
            self.owner,
            crate::engine::AndaBot::NAME,
            "",
            RequestMeta::default(),
        )?;
        NoteTool::new()
            .call(
                ctx.child_base(NoteTool::NAME)?,
                NoteArgs {
                    op: Some("set".into()),
                    items: Some(vec![]),
                },
                vec![],
            )
            .await?;
        Ok(())
    }
    /// Read-only contexts must not perform a pending Notes migration themselves.
    pub async fn coherent_epoch_locked(&self) -> Result<u64, BoxError> {
        let space = self
            .host
            .state
            .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
            .await?;
        let epoch = space.product_epoch();
        if !space.product_available()
            || self
                .journal
                .read::<u64>("notes-epoch/v1")
                .await?
                .unwrap_or(0)
                != epoch
        {
            return Err("memory_change_pending".into());
        }
        Ok(epoch)
    }
    pub async fn check_locked(&self, ctx: &BaseCtx) -> Result<(), BoxError> {
        let epoch = self.synchronize_locked().await?;
        if ctx
            .get_state::<MemoryEpoch>()
            .is_some_and(|captured| captured.0 != epoch)
        {
            return Err("Memory changed; start a new conversation before updating Notes.".into());
        }
        if let Some(source) = ctx.get_state::<anda_brain::product::SourceIdentity>()
            && !self
                .host
                .state
                .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
                .await?
                .product_source_allowed(&source)
        {
            return Err("This source no longer contributes memory; use /new.".into());
        }
        Ok(())
    }
}
