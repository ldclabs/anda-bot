use super::{Client, HttpError, product::*};
use anda_core::BoxError;
use std::{future::Future, time::Duration};

#[derive(Clone)]
pub struct MemoryService {
    client: Client,
    activity: Option<std::sync::Arc<super::activity::ActivityStore>>,
    mutations: Option<std::sync::Arc<super::mutation::MutationService>>,
    setup: Option<super::setup::InboxSetup>,
    searches: std::sync::Arc<tokio::sync::Semaphore>,
}

impl MemoryService {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            activity: None,
            mutations: None,
            setup: None,
            searches: std::sync::Arc::new(tokio::sync::Semaphore::new(2)),
        }
    }

    pub fn with_activity(mut self, activity: super::activity::ActivityStore) -> Self {
        self.activity = Some(std::sync::Arc::new(activity));
        self
    }

    pub async fn search(
        &self,
        caller: anda_core::Principal,
        bearer: String,
        request: SearchRequest,
    ) -> Result<SearchResult, BoxError> {
        if request.query.len() > 8192 {
            return Err("payload_too_large".into());
        }
        if request.query.trim().is_empty() {
            return Err("invalid_request".into());
        }
        request.budget.validate().map_err(|_| "invalid_request")?;
        let _permit = self.searches.try_acquire().map_err(|_| "capacity")?;
        let result = self
            .client
            .with_auth_token(bearer)
            .recall_structured(&anda_brain::types::RecallInput {
                query: request.query,
                context: Some(anda_brain::types::InputContext {
                    counterparty: Some(caller.to_string()),
                    source: Some("memory-product-search".into()),
                    ..Default::default()
                }),
                budget: Some(request.budget),
            })
            .await
            .map_err(|_| "search_result_unknown")?;
        let budget = result.memory_budget.ok_or("unsupported_capability")?;
        Ok(SearchResult {
            schema_version: 1,
            incomplete: result.failed_reason.is_some(),
            packet: result.answer,
            found: result.found,
            conversation: result.conversation.map(|id| id.to_string()),
            budget,
        })
    }

    pub async fn run_background(&self, cancel: tokio_util::sync::CancellationToken) {
        tokio::join!(
            async {
                if let Some(activity) = &self.activity {
                    activity.run(cancel.clone()).await;
                }
            },
            async {
                if let Some(mutations) = &self.mutations {
                    mutations.run(cancel.clone()).await;
                }
            },
            async {
                if let Some(setup) = &self.setup {
                    setup.run(cancel.clone()).await;
                }
            }
        );
    }

    pub fn with_setup(mut self, setup: super::setup::InboxSetup) -> Self {
        self.setup = Some(setup);
        self
    }
    pub async fn setup_preview(
        &self,
        caller: anda_core::Principal,
    ) -> Result<super::setup::SetupPreview, BoxError> {
        self.setup
            .as_ref()
            .ok_or("unsupported_capability")?
            .prepare(caller)
            .await
    }
    pub async fn setup_commit(
        &self,
        caller: anda_core::Principal,
        digest: &str,
    ) -> Result<super::setup::SetupPreview, BoxError> {
        self.setup
            .as_ref()
            .ok_or("unsupported_capability")?
            .commit(caller, digest)
            .await
    }

    pub async fn watch(
        &self,
        caller: anda_core::Principal,
        input: WatchRequest,
    ) -> Result<anda_brain::runtime_api::RecordWatch, BoxError> {
        if input.operation_id.is_empty() || input.operation_id.len() > 128 {
            return Err("invalid_request".into());
        }
        let host = self
            .client
            .embedded_host()
            .ok_or("unsupported_capability")?;
        let journal = self.client.journal().ok_or("unsupported_capability")?;
        let key = format!(
            "record-watch/{}",
            &anda_cognitive_nexus::content_digest(
                &serde_json::json!({"caller":caller.to_string(),"operation":input.operation_id})
            )?[7..]
        );
        let intent = match journal.read::<WatchIntent>(&key).await? {
            Some(intent) => intent,
            None => {
                let record = self.record(caller, &input.record_id).await?;
                let intent = WatchIntent {
                    operation_id: input.operation_id.clone(),
                    caller: caller.to_string(),
                    record_id: record.id,
                    summary: record.text.chars().take(512).collect(),
                };
                journal.create(&key, &intent).await?;
                journal
                    .read::<WatchIntent>(&key)
                    .await?
                    .ok_or("watch intent missing")?
            }
        };
        if intent.caller != caller.to_string() || intent.record_id != input.record_id {
            return Err("idempotency_conflict".into());
        }
        host.watch_record(caller, input.operation_id, input.record_id, intent.summary)
            .await
    }
    pub async fn cancel_watch(
        &self,
        caller: anda_core::Principal,
        id: String,
    ) -> Result<anda_brain::runtime_api::RecordWatch, BoxError> {
        self.client
            .embedded_host()
            .ok_or("unsupported_capability")?
            .cancel_record_watch(caller, id)
            .await
    }

    pub async fn watches(
        &self,
        caller: anda_core::Principal,
        query: WatchQuery,
    ) -> Result<WatchPage, BoxError> {
        use futures::TryStreamExt;
        use std::collections::BTreeSet;
        let limit = query.limit.unwrap_or(50);
        if !(1..=50).contains(&limit) {
            return Err("invalid_request".into());
        }
        let caller_text = caller.to_string();
        let after = match query.cursor {
            Some(text) if text.len() <= 2048 => {
                let cursor: WatchCursor =
                    serde_json::from_str(&text).map_err(|_| "invalid_cursor")?;
                if cursor.caller != caller_text || !cursor.after.starts_with("record-watch/") {
                    return Err("invalid_cursor".into());
                }
                cursor.after
            }
            Some(_) => return Err("invalid_cursor".into()),
            None => String::new(),
        };
        let journal = self.client.journal().ok_or("unsupported_capability")?;
        let host = self
            .client
            .embedded_host()
            .ok_or("unsupported_capability")?;
        let store = journal.object_store();
        let prefix = object_store::path::Path::from("bot-brain/v1/record-watch/");
        let mut stream = store.list(Some(&prefix));
        // ObjectStore listing order is unspecified. Keep only the next bounded
        // set of keys, with a lookahead, so every skipped history row is resumable.
        let mut keys = BTreeSet::new();
        while let Some(meta) = stream.try_next().await? {
            if let Some(key) = meta.location.as_ref().strip_prefix("bot-brain/v1/")
                && key > after.as_str()
            {
                keys.insert(key.to_string());
                if keys.len() > 1001 {
                    keys.pop_last();
                }
            }
        }
        let mut more = keys.len() > 1000;
        let mut last = after;
        let mut items = Vec::new();
        let mut incomplete = false;
        for key in keys.into_iter().take(1000) {
            if items.len() == limit {
                more = true;
                break;
            }
            last = key.clone();
            let Some(intent) = journal.read::<WatchIntent>(&key).await? else {
                continue;
            };
            if intent.caller != caller_text || intent.operation_id.is_empty() {
                continue;
            }
            match host.record_watch(caller, &intent.operation_id).await {
                Ok(watch) if watch.state != "cancelled" => items.push(watch),
                Ok(_) => {}
                Err(_) => incomplete = true,
            }
        }
        Ok(WatchPage {
            schema_version: 1,
            items,
            complete: !more && !incomplete,
            partial_reason: incomplete.then(|| "watch_status_unavailable".into()),
            next_cursor: if more {
                Some(serde_json::to_string(&WatchCursor {
                    caller: caller_text,
                    after: last,
                })?)
            } else {
                None
            },
        })
    }

    pub fn with_mutations(
        mut self,
        mutations: std::sync::Arc<super::mutation::MutationService>,
    ) -> Self {
        self.mutations = Some(mutations);
        self
    }

    pub async fn prepare_change(
        &self,
        caller: anda_core::Principal,
        input: super::mutation::ChangeRequest,
    ) -> Result<super::mutation::ChangeView, BoxError> {
        let mutations = self.mutations.as_ref().ok_or("unsupported_capability")?;
        if let Some(existing) = mutations.existing(caller, &input).await? {
            return Ok(existing);
        }
        let before = self.record(caller, &input.record_id).await?;
        mutations.prepare(caller, input, before).await
    }
    pub async fn commit_change(
        &self,
        caller: anda_core::Principal,
        id: String,
        request: super::mutation::CommitRequest,
    ) -> Result<super::mutation::ChangeView, BoxError> {
        self.mutations
            .as_ref()
            .ok_or("unsupported_capability")?
            .commit(caller, id, request.preview_digest)
            .await
    }
    pub async fn change(
        &self,
        caller: anda_core::Principal,
        id: &str,
    ) -> Result<super::mutation::ChangeView, BoxError> {
        self.mutations
            .as_ref()
            .ok_or("unsupported_capability")?
            .get(caller, id)
            .await
    }

    pub async fn discard_change(
        &self,
        caller: anda_core::Principal,
        id: &str,
    ) -> Result<(), BoxError> {
        self.mutations
            .as_ref()
            .ok_or("unsupported_capability")?
            .discard(caller, id)
            .await
    }

    pub async fn activity(
        &self,
        caller: anda_core::Principal,
        query: super::activity::ActivityQuery,
    ) -> Result<super::activity::ActivityPage, BoxError> {
        self.activity
            .as_ref()
            .ok_or("unsupported_capability")?
            .page(caller, query)
            .await
    }

    pub async fn records(
        &self,
        caller: anda_core::Principal,
        query: super::catalog::RecordQuery,
    ) -> Result<(super::catalog::RecordPage, Option<String>), BoxError> {
        super::catalog::list(
            &self
                .client
                .embedded_host()
                .ok_or("unsupported_capability")?,
            self.activity.as_ref().ok_or("unsupported_capability")?,
            caller,
            query,
        )
        .await
    }

    pub async fn record(
        &self,
        caller: anda_core::Principal,
        id: &str,
    ) -> Result<super::catalog::MemoryRecordView, BoxError> {
        super::catalog::get(
            &self
                .client
                .embedded_host()
                .ok_or("unsupported_capability")?,
            self.activity.as_ref().ok_or("unsupported_capability")?,
            caller,
            id,
        )
        .await
    }

    /// Only the transport's verified original bearer is accepted here. Never
    /// use the daemon credential for a caller's native runtime requests.
    pub async fn overview(&self, bearer: String) -> MemoryOverview {
        let client = self.client.with_auth_token(bearer);
        let (memory, runtime) = tokio::join!(
            read_status(client.brain_status()),
            read_status(client.runtime_status()),
        );
        let memory = match memory {
            Ok(status) => MemoryStatus {
                state: ReadState::Reachable,
                formation_active: Some(status.formation_processing),
                maintenance_active: Some(status.maintenance_processing),
                reason: None,
            },
            Err(state) => MemoryStatus {
                state,
                formation_active: None,
                maintenance_active: None,
                reason: Some("memory_status_unavailable".into()),
            },
        };
        let learning = runtime.as_ref().ok().map(|status| status.learning.clone());
        let runtime_read_failed = runtime.is_err();
        let inbox = match runtime {
            Ok(status) => InboxStatus {
                state: if status.configured && status.attention_enabled {
                    ReadState::Available
                } else {
                    ReadState::NotConfigured
                },
                visible_items: status.configured.then_some(status.visible_items),
                inventory_complete: status.inventory_complete,
                reason: if !status.configured {
                    Some("runtime_bindings_not_installed".into())
                } else if !status.attention_enabled {
                    Some("attention_disabled".into())
                } else {
                    None
                },
            },
            Err(state) => InboxStatus {
                state,
                visible_items: None,
                inventory_complete: false,
                reason: Some("inbox_status_unavailable".into()),
            },
        };
        let mut capabilities = [
            ("activity", "activity_projection_not_installed"),
            ("records", "native_record_contract_not_verified"),
            ("changes", "native_mutation_contract_not_verified"),
            ("memory_policy", "memory_policy_not_enforced"),
            ("evaluation", "evaluation_runner_not_installed"),
            ("learning", "learning_template_contract_not_verified"),
        ]
        .into_iter()
        .map(|(name, reason)| (name.into(), Capability::unsupported(reason)))
        .collect::<std::collections::BTreeMap<_, _>>();
        capabilities.insert(
            "inbox".into(),
            Capability {
                state: match inbox.state {
                    ReadState::Available => CapabilityState::Available,
                    ReadState::Forbidden | ReadState::Unauthorized => CapabilityState::Forbidden,
                    ReadState::NotConfigured => CapabilityState::NotConfigured,
                    _ => CapabilityState::Unavailable,
                },
                reason: inbox.reason.clone(),
            },
        );
        if self.activity.is_some() {
            capabilities.insert(
                "activity".into(),
                Capability {
                    state: CapabilityState::Available,
                    reason: None,
                },
            );
        }
        if self.activity.is_some() && self.client.embedded_host().is_some() {
            capabilities.insert(
                "records".into(),
                Capability {
                    state: CapabilityState::Available,
                    reason: None,
                },
            );
        }
        if self.mutations.is_some() {
            capabilities.insert(
                "changes".into(),
                Capability {
                    state: CapabilityState::Available,
                    reason: None,
                },
            );
        }
        if self.setup.is_some() {
            capabilities.insert(
                "inbox_setup".into(),
                Capability {
                    state: CapabilityState::Available,
                    reason: None,
                },
            );
        }
        if self.client.embedded_host().is_some() && inbox.state == ReadState::Available {
            capabilities.insert(
                "record_watches".into(),
                Capability {
                    state: CapabilityState::Available,
                    reason: None,
                },
            );
        }
        for name in ["memory_policy", "search"] {
            capabilities.insert(
                name.into(),
                Capability {
                    state: CapabilityState::Available,
                    reason: None,
                },
            );
        }
        capabilities.insert(
            "evaluation".into(),
            Capability {
                state: if cfg!(feature = "mib") {
                    CapabilityState::Available
                } else {
                    CapabilityState::Unsupported
                },
                reason: Some(
                    if cfg!(feature = "mib") {
                        "explicit_isolated_cli_only"
                    } else {
                        "not_compiled"
                    }
                    .into(),
                ),
            },
        );
        let readiness = learning.as_ref().and_then(|value|value.get("product_readiness")).cloned().unwrap_or_else(||serde_json::json!({"state":if cfg!(feature="learning"){"services_missing"}else{"not_compiled"},"next_step":if cfg!(feature="learning"){"install_supported_business_template"}else{"use_learning_enabled_build_for_business_workflows"}}));
        let mut readiness = if runtime_read_failed && cfg!(feature = "learning") {
            serde_json::json!({"state":"unavailable","next_step":"restore_runtime_status_access"})
        } else {
            readiness
        };
        readiness["template_contract"] =
            serde_json::from_str(include_str!("../../assets/memory/workflow-template.json"))
                .expect("bundled workflow contract");
        readiness["scope"] = "isolated_learning".into();
        readiness["business_application_authorized"] = false.into();
        capabilities.insert(
            "learning".into(),
            Capability {
                state: if readiness["state"] == "ready" {
                    CapabilityState::Available
                } else if readiness["state"] == "unavailable" {
                    CapabilityState::Unavailable
                } else if !cfg!(feature = "learning") {
                    CapabilityState::Unsupported
                } else {
                    CapabilityState::NotConfigured
                },
                reason: readiness["state"].as_str().map(str::to_string),
            },
        );
        MemoryOverview {
            caller: None,
            learning: readiness,
            schema_version: 1,
            observed_at: anda_engine::unix_ms(),
            memory,
            inbox,
            capabilities,
        }
    }
}

async fn read_status<T>(future: impl Future<Output = Result<T, BoxError>>) -> Result<T, ReadState> {
    match tokio::time::timeout(Duration::from_secs(10), future).await {
        Err(_) => Err(ReadState::Timeout),
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(
            match error.downcast_ref::<HttpError>().map(|e| e.status.as_u16()) {
                Some(401) => ReadState::Unauthorized,
                Some(403) => ReadState::Forbidden,
                _ => ReadState::Unavailable,
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, routing::get};
    use serde_json::json;

    #[tokio::test]
    async fn memory_search_never_retries_a_received_error_and_checks_utf8_budget_first() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let app = Router::new().route(
            "/recall_structured",
            axum::routing::post(
                move |headers: http::HeaderMap, Json(input): Json<serde_json::Value>| {
                    let count = count.clone();
                    async move {
                        assert_eq!(
                            headers[http::header::AUTHORIZATION],
                            "Bearer original-owner"
                        );
                        assert_eq!(input["budget"]["max_tokens"], 4096);
                        count.fetch_add(1, Ordering::SeqCst);
                        (
                            http::StatusCode::SERVICE_UNAVAILABLE,
                            Json(json!({"error":"accepted work result unavailable"})),
                        )
                    }
                },
            ),
        );
        let url = crate::test_support::spawn_http_mock(app).await;
        let client = Client::new(url, None);
        let service = MemoryService::new(client);
        let caller = anda_core::Principal::management_canister();
        let query = SearchRequest {
            query: "a preference".into(),
            budget: Default::default(),
        };
        assert!(
            service
                .search(caller, "original-owner".into(), query)
                .await
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let query = SearchRequest {
            query: "中".repeat(3000),
            budget: Default::default(),
        };
        assert_eq!(
            service
                .search(caller, "original-owner".into(), query)
                .await
                .unwrap_err()
                .to_string(),
            "payload_too_large"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn memory_overview_keeps_memory_when_runtime_forbids_and_forwards_original_bearer() {
        let app = Router::new()
            .route("/formation_status", get(|headers: http::HeaderMap| async move {
                assert_eq!(headers[http::header::AUTHORIZATION], "Bearer original-owner");
                Json(json!({"result": super::super::FormationStatus { formation_processing: true, ..Default::default() }}))
            }))
            .route("/runtime/status", get(|headers: http::HeaderMap| async move {
                assert_eq!(headers[http::header::AUTHORIZATION], "Bearer original-owner");
                (http::StatusCode::FORBIDDEN, Json(json!({"error":"private details must not be displayed"})))
            }));
        let url = crate::test_support::spawn_http_mock(app).await;
        let overview = MemoryService::new(Client::new(url, Some("daemon-secret".into())))
            .overview("original-owner".into())
            .await;
        assert!(overview.is_connected());
        assert_eq!(overview.inbox.state, ReadState::Forbidden);
        assert_eq!(overview.inbox.visible_items, None);
        assert_eq!(overview.memory.formation_active, Some(true));
        assert!(
            !serde_json::to_string(&overview)
                .unwrap()
                .contains("private details")
        );
    }

    #[tokio::test]
    async fn memory_overview_never_accepts_partial_rpc_error_as_success() {
        let app = Router::new().route("/formation_status", get(|| async {
            Json(json!({"error":{"code":500,"message":"failed"},"result":super::super::FormationStatus::default()}))
        }));
        let url = crate::test_support::spawn_http_mock(app).await;
        let overview = MemoryService::new(Client::new(url, None))
            .overview("owner".into())
            .await;
        assert!(!overview.is_connected());
        assert_eq!(overview.memory.formation_active, None);
        assert_eq!(overview.inbox.state, ReadState::Unavailable);
        assert_eq!(
            overview.capabilities["inbox"].state,
            CapabilityState::Unavailable
        );
    }

    #[tokio::test]
    async fn memory_overview_unconfigured_inbox_is_optional() {
        let runtime = json!({"supported":true,"configured":false,"scope":null,"attention_enabled":false,"actions_enabled":false,"observation_enabled":false,"observer_authenticated":false,"blocked_reasons":["runtime_bindings_not_installed"],"visible_items":0,"inventory_complete":false});
        let app = Router::new()
            .route(
                "/formation_status",
                get(|| async { Json(json!({"result":super::super::FormationStatus::default()})) }),
            )
            .route(
                "/runtime/status",
                get(move || {
                    let runtime = runtime.clone();
                    async move { Json(json!({"result":runtime})) }
                }),
            );
        let url = crate::test_support::spawn_http_mock(app).await;
        let overview = MemoryService::new(Client::new(url, None))
            .overview("owner".into())
            .await;
        assert!(overview.is_connected());
        assert_eq!(overview.inbox.state, ReadState::NotConfigured);
        assert_eq!(overview.inbox.visible_items, None);
        assert!(overview.render().contains("无需额外配置"));
    }
}
