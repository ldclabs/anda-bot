//! Coherence between conversations and the Bot's native memory epoch.
use super::Host;
use anda_core::BoxError;

#[derive(Clone)]
pub struct MemoryEpoch(pub u64);

pub struct MemoryAccess {
    pub gate: tokio::sync::Mutex<()>,
    pub host: Host,
}
impl MemoryAccess {
    pub fn new(host: Host) -> Self {
        Self {
            gate: tokio::sync::Mutex::new(()),
            host,
        }
    }
    /// The caller holds `gate` across capture/read or a native commit.
    pub async fn epoch_locked(&self) -> Result<u64, BoxError> {
        let space = self
            .host
            .state
            .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
            .await?;
        if !space.product_available() {
            return Err("memory_change_pending".into());
        }
        Ok(space.product_epoch())
    }
}
