//! In-memory config cache for `typing_ticket` with TTL and exponential backoff retry.

use std::sync::Arc;

use dashmap::DashMap;

use crate::api::client::HttpApiClient;
use crate::types::CONFIG_CACHE_TTL_MS;

const INITIAL_RETRY_MS: u64 = 2_000;
const MAX_RETRY_MS: u64 = 3_600_000;

struct CacheEntry {
    typing_ticket: String,
    next_fetch_at: u64,
    retry_delay_ms: u64,
}

/// Per-user `getConfig` cache with periodic refresh and exponential-backoff retry.
pub(crate) struct ConfigCache {
    api: Arc<HttpApiClient>,
    cache: DashMap<String, CacheEntry>,
}

impl ConfigCache {
    /// Create a new cache backed by the given API client.
    pub fn new(api: Arc<HttpApiClient>) -> Self {
        Self {
            api,
            cache: DashMap::new(),
        }
    }

    /// Get the cached `typing_ticket` for a user, refreshing if stale.
    pub async fn get_typing_ticket(
        &self,
        user_id: &str,
        context_token: Option<&str>,
    ) -> Option<String> {
        let now = now_ms();
        let should_fetch = self
            .cache
            .get(user_id)
            .is_none_or(|e| now >= e.next_fetch_at);

        if should_fetch {
            match self.api.get_config(user_id, context_token).await {
                Ok(resp) if resp.ret.unwrap_or(-1) == 0 => {
                    let ticket = resp.typing_ticket.unwrap_or_default();
                    // Jitter within TTL for staggered refresh
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        clippy::cast_precision_loss
                    )]
                    let jitter = (rand::random::<f64>() * CONFIG_CACHE_TTL_MS as f64) as u64;
                    self.cache.insert(
                        user_id.to_owned(),
                        CacheEntry {
                            typing_ticket: ticket,
                            next_fetch_at: now + jitter,
                            retry_delay_ms: INITIAL_RETRY_MS,
                        },
                    );
                }
                _ => {
                    // On failure, apply exponential backoff
                    let mut entry = self.cache.entry(user_id.to_owned()).or_insert(CacheEntry {
                        typing_ticket: String::new(),
                        next_fetch_at: now + INITIAL_RETRY_MS,
                        retry_delay_ms: INITIAL_RETRY_MS,
                    });
                    let next_delay = (entry.retry_delay_ms * 2).min(MAX_RETRY_MS);
                    entry.next_fetch_at = now + next_delay;
                    entry.retry_delay_ms = next_delay;
                }
            }
        }

        self.cache.get(user_id).map(|e| e.typing_ticket.clone())
    }
}

use crate::util::now_ms;
