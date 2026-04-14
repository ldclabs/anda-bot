use anda_cognitive_nexus::{CognitiveNexus, ConceptPK};
use anda_core::{
    AgentInput, AgentOutput, BoxError, FunctionDefinition, Principal, Resource, Usage,
};
use anda_db::{
    collection::CollectionConfig,
    database::{AndaDB, DBConfig},
    error::DBError,
    index::BTree,
    query::Fv,
    schema::DocumentId,
};
use anda_db_tfs::jieba_tokenizer;
use anda_engine::{
    engine::Engine,
    management::Management,
    memory::{Conversation, MemoryManagement, MemoryReadonly, MemoryTool},
    model::{Models, reqwest},
    rfc3339_datetime_now, unix_ms,
};
use anda_kip::{
    KipError, META_SELF_NAME, PERSON_SELF_KIP, PERSON_SYSTEM_KIP, PERSON_TYPE, parse_kml,
};
use ic_auth_types::ByteBufB64;
use ic_cose_types::cose::{
    SIGN1_TAG, cwt::cwt_from, ed25519::VerifyingKey, sign1::cose_sign1_from, skip_prefix,
};
use object_store::ObjectStore;
use serde_json::json;
use std::{
    collections::BTreeMap,
    str::FromStr,
    sync::{
        Arc, LazyLock, OnceLock, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{OnceCell, RwLock};
use tokio_util::sync::CancellationToken;

use crate::types::{
    AddSpaceTokenInput, CWToken, FormationInput, FormationStatus, MaintenanceInput,
    MaintenanceScope, ModelConfig, RecallInput, SpaceInfo, SpaceTier, SpaceToken, TokenScope,
    UpdateSpaceInput,
};
use crate::{
    agents::{AgentHook, FormationAgent, MaintenanceAgent, RecallAgent},
    model::build_model,
    payload::StringOr,
};

pub static FUNCTION_DEFINITION: LazyLock<FunctionDefinition> = LazyLock::new(|| {
    serde_json::from_value(json!({
        "name": "execute_kip",
        "description": "Executes one or more KIP (Knowledge Interaction Protocol) commands against the Cognitive Nexus to interact with your persistent memory.",
        "parameters": {
            "type": "object",
            "properties": {
                "commands": {
                    "type": "array",
                    "description": "An array of KIP commands for batch execution (reduces round-trips). Commands are executed sequentially; execution stops on first KML error.",
                    "items": {
                        "type": "string"
                    }
                },
                "parameters": {
                    "type": "object",
                    "description": "An optional JSON object of key-value pairs used for safe substitution of placeholders in the command string(s). Placeholders should start with ':' (e.g., :name, :limit). IMPORTANT: A placeholder must represent a complete JSON value token (e.g., name: :name). Do not embed placeholders inside quoted strings (e.g., \"Hello :name\"), because substitution uses JSON serialization."
                },
            },
            "required": ["commands"]
        }
    })).unwrap()
});

pub struct SpaceEntry {
    cell: OnceCell<Arc<Space>>,
    last_access_ms: AtomicU64,
}

impl SpaceEntry {
    fn new() -> Self {
        Self {
            cell: OnceCell::new(),
            last_access_ms: AtomicU64::new(unix_ms()),
        }
    }

    fn touch(&self) {
        self.last_access_ms.store(unix_ms(), Ordering::Relaxed);
    }

    fn last_access_ms(&self) -> u64 {
        self.last_access_ms.load(Ordering::Relaxed)
    }
}

#[derive(Clone)]
pub struct AppState {
    spaces: Arc<RwLock<BTreeMap<String, Arc<SpaceEntry>>>>,
    object_store: Arc<dyn ObjectStore>,
    db_config: Arc<DBConfig>,
    http_client: reqwest::Client,
    models: Arc<Models>,
    ed25519_pubkeys: Vec<VerifyingKey>,
    management: Arc<dyn Management>,

    pub app_name: String,
    pub app_version: String,
    pub sharding: u32,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        object_store: Arc<dyn ObjectStore>,
        db_config: Arc<DBConfig>,
        management: Arc<dyn Management>,
        http_client: reqwest::Client,
        models: Arc<Models>,
        ed25519_pubkeys: Vec<VerifyingKey>,
        app_name: String,
        app_version: String,
        sharding: u32,
    ) -> Self {
        Self {
            spaces: Arc::new(RwLock::new(BTreeMap::new())),
            object_store,
            db_config,
            management,
            http_client,
            models,
            ed25519_pubkeys,
            app_name,
            app_version,
            sharding,
        }
    }

    // 平台管理员权限
    pub fn check_admin(
        &self,
        token: &str,
        audience: &str,
        scope: TokenScope,
        now_ms: u64,
    ) -> Result<CWToken, BoxError> {
        if self.ed25519_pubkeys.is_empty() {
            return Ok(CWToken {
                user: Principal::management_canister(),
                audience: audience.to_string(),
                scope,
            });
        }

        let token = self.check_auth(token, audience, scope, now_ms)?;
        if !self.management.is_manager(&token.user) {
            return Err("admin access required".into());
        }

        Ok(token)
    }

    // 用户权限
    pub fn check_auth_if(
        &self,
        token: &str,
        audience: &str,
        scope: TokenScope,
        now_ms: u64,
    ) -> Result<Option<CWToken>, BoxError> {
        if self.ed25519_pubkeys.is_empty() {
            return Ok(Some(CWToken {
                user: Principal::anonymous(),
                audience: audience.to_string(),
                scope,
            }));
        }

        if token.len() < 60 {
            return Ok(None);
        }

        let token = self.check_auth(token, audience, scope, now_ms)?;
        Ok(Some(token))
    }

    pub fn check_auth(
        &self,
        token: &str,
        audience: &str,
        scope: TokenScope,
        now_ms: u64,
    ) -> Result<CWToken, BoxError> {
        if self.ed25519_pubkeys.is_empty() {
            return Ok(CWToken {
                user: Principal::anonymous(),
                audience: audience.to_string(),
                scope,
            });
        }

        let data = ByteBufB64::from_str(token)?;
        let data = skip_prefix(&SIGN1_TAG, &data);
        let cs1 = cose_sign1_from(data, &[], &[], &self.ed25519_pubkeys)?;
        let claims = cwt_from(&cs1.payload.unwrap_or_default(), (now_ms / 1000) as i64)?;
        let token = CWToken::from_claims(claims)?;
        if token.audience != audience && token.audience != "*" {
            return Err("invalid audience".into());
        }

        if !token.scope.allows(scope) {
            return Err("insufficient scope".into());
        }
        Ok(token)
    }

    pub async fn admin_create_space(
        &self,
        creator: Principal,
        owner: Principal,
        id: String,
        tier: u32,
        now_ms: u64,
    ) -> Result<SpaceInfo, BoxError> {
        {
            let spaces = self.spaces.read().await;
            if spaces
                .get(&id)
                .is_some_and(|entry| entry.cell.initialized())
            {
                return Err(format!("space {} already exists", &id).into());
            }
        }

        let mut db_config = (*self.db_config).clone();
        db_config.name = id;
        Space::create(
            self.object_store.clone(),
            db_config,
            creator,
            owner,
            tier,
            now_ms,
        )
        .await
    }

    pub async fn load_space(&self, space_id: &str, pinned: bool) -> Result<Arc<Space>, BoxError> {
        let entry = {
            let spaces = self.spaces.read().await;
            spaces.get(space_id).cloned()
        };

        let entry = match entry {
            Some(entry) => entry,
            None => {
                let mut spaces = self.spaces.write().await;
                spaces
                    .entry(space_id.to_string())
                    .or_insert_with(|| Arc::new(SpaceEntry::new()))
                    .clone()
            }
        };

        let space = entry
            .cell
            .get_or_try_init(|| async {
                let mut db_config = (*self.db_config).clone();
                db_config.name = space_id.to_string();
                Space::connect(
                    self.object_store.clone(),
                    db_config,
                    self.management.clone(),
                    self.http_client.clone(),
                    self.models.clone(),
                    pinned,
                )
                .await
            })
            .await
            .cloned()?;

        entry.touch();
        Ok(space)
    }

    /// Starts background maintenance tasks:
    /// - Flushes active space databases every 5 minutes.
    /// - Evicts spaces idle for over 20 minutes.
    pub async fn start_background_tasks(&self, cancel_token: CancellationToken) {
        let flush_interval = Duration::from_secs(5 * 60);
        let idle_timeout_ms: u64 = 9 * 60 * 1000;

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    // Flush all spaces before shutting down
                    let spaces = self.spaces.read().await;
                    for (id, entry) in spaces.iter() {
                        if let Some(space) = entry.cell.get()
                            && let Err(err) = space.db.close().await {
                                log::error!(space_id = id; "flush on shutdown failed: {err:?}");
                            }
                    }
                    return;
                }
                _ = tokio::time::sleep(flush_interval) => {}
            }

            let now = unix_ms();

            // Collect entries snapshot under read lock
            let entries: Vec<(String, Arc<SpaceEntry>)> = {
                let spaces = self.spaces.read().await;
                spaces.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
            };

            for (id, entry) in &entries {
                let Some(space) = entry.cell.get() else {
                    continue;
                };

                if !space.pinned
                    && now.saturating_sub(entry.last_access_ms()) > idle_timeout_ms
                    && !space.is_processing()
                {
                    {
                        self.spaces.write().await.remove(id);
                    }
                    if let Err(err) = space.db.close().await {
                        log::error!(space_id = id; "flush before eviction failed: {err:?}");
                    } else {
                        log::warn!(space_id = id; "space evicted due to inactivity");
                    }
                } else {
                    // Periodic flush for active spaces
                    if let Err(err) = space.flush().await {
                        log::error!(space_id = id; "periodic flush failed: {err:?}");
                    }
                }
            }
        }
    }
}

pub struct Space {
    id: String,
    engine: Engine,
    http_client: reqwest::Client,
    models: Arc<Models>,
    recall: Arc<RecallAgent>,
    formation: Arc<FormationAgent>,
    maintenance: Arc<MaintenanceAgent>,
    pinned: bool,
    pub db: Arc<AndaDB>,
    pub memory: Arc<MemoryManagement>,
}

impl Space {
    pub fn is_processing(&self) -> bool {
        self.formation.is_processing() || self.maintenance.is_processing()
    }

    pub fn get_tier(&self) -> SpaceTier {
        self.db.get_extension_as("tier").unwrap_or_default()
    }

    pub async fn admin_update_tier(&self, tier: u32, now_ms: u64) -> Result<SpaceTier, BoxError> {
        let tier = SpaceTier {
            tier,
            updated_at: now_ms,
        };
        self.db
            .save_extension_from("tier".to_string(), &tier.to_ref())
            .await?;
        Ok(tier)
    }

    pub async fn add_space_token(
        &self,
        token: String,
        input: AddSpaceTokenInput,
        now_ms: u64,
    ) -> Result<SpaceToken, BoxError> {
        let count = self
            .db
            .extensions_with(|kv| kv.keys().filter(|k| k.starts_with("ST")).count());
        if count >= 100 {
            return Err("space token limit reached".into());
        }

        let sp = SpaceToken {
            token: token.clone(),
            scope: input.scope,
            name: input.name,
            expires_at: input.expires_at,
            created_at: now_ms,
            updated_at: now_ms,
            ..Default::default()
        };

        self.db.save_extension_from(token, &sp.to_ref()).await?;
        Ok(sp)
    }

    pub fn verify_space_token(
        &self,
        token: String,
        scope: TokenScope,
        now_ms: u64,
    ) -> Result<(), BoxError> {
        let token = self
            .db
            .set_extension_from_with::<_, SpaceToken>(token, |v| {
                if let Some(mut st) = v
                    && st.expires_at.map(|exp| exp > now_ms).unwrap_or(true)
                    && st.scope.allows(scope)
                {
                    st.usage = st.usage.saturating_add(1);
                    st.updated_at = now_ms;
                    return Some(st);
                }
                None
            });

        if token.is_none() {
            return Err("invalid space token".into());
        }
        Ok(())
    }

    pub async fn revoke_space_token(&self, token: &str) -> Result<bool, BoxError> {
        let rt = self.db.remove_extension(token).await?;
        Ok(rt.is_some())
    }

    pub fn list_space_tokens(&self) -> Result<Vec<SpaceToken>, BoxError> {
        let tokens: Vec<SpaceToken> = self.db.extensions_with(|kvs| {
            kvs.iter()
                .filter_map(|(k, v)| {
                    if k.starts_with("ST")
                        && let Ok(mut st) = v.clone().deserialized::<SpaceToken>()
                    {
                        st.token = k.clone();
                        Some(st)
                    } else {
                        None
                    }
                })
                .collect()
        });

        Ok(tokens)
    }

    pub async fn update(&self, input: UpdateSpaceInput, now_ms: u64) -> Result<(), BoxError> {
        let mut changed = false;
        if let Some(name) = input.name {
            changed = true;
            self.db.set_extension_from("name".to_string(), name);
        }
        if let Some(description) = input.description {
            changed = true;
            self.db
                .set_extension_from("description".to_string(), description);
        }
        if let Some(public) = input.public {
            changed = true;
            self.db.set_extension_from("public".to_string(), public);
        }
        if changed {
            self.db.flush_metadata(now_ms).await?;
        }
        Ok(())
    }

    pub fn get_byok(&self) -> Option<ModelConfig> {
        self.db.get_extension_as("byok")
    }

    pub async fn update_byok(&self, model_config: ModelConfig) -> Result<(), BoxError> {
        self.db
            .save_extension_from("byok".to_string(), &model_config.to_ref())
            .await?;
        let model = build_model(self.http_client.clone(), model_config);
        self.models.set_model(model);
        Ok(())
    }

    pub fn is_public(&self) -> bool {
        self.db.get_extension_as("public").unwrap_or(false)
    }

    pub fn get_info(&self) -> SpaceInfo {
        let mut info = SpaceInfo {
            id: self.id.clone(),
            db_stats: self.db.stats(),
            concepts: self.memory.nexus.concepts.len(),
            propositions: self.memory.nexus.propositions.len(),
            conversations: self.memory.conversations.len(),
            formation_processed_id: self.formation.get_processed().unwrap_or_default(),
            maintenance_processed_id: self.maintenance.get_processed().unwrap_or_default(),
            maintenance_at: self.maintenance.get_processed_at(),
            ..Default::default()
        };

        self.db.extensions_with(|kv| {
            info.name = kv
                .get("name")
                .and_then(|v| String::try_from(v.clone()).ok());
            info.description = kv
                .get("description")
                .and_then(|v| String::try_from(v.clone()).ok());
            info.owner = kv
                .get("owner")
                .and_then(|v| String::try_from(v.clone()).ok())
                .unwrap_or_default();
            info.public = kv
                .get("public")
                .and_then(|v| bool::try_from(v.clone()).ok())
                .unwrap_or(false);
            info.tier = kv
                .get("tier")
                .and_then(|v| v.clone().deserialized::<SpaceTier>().ok())
                .unwrap_or_default();
            info.formation_usage = kv
                .get("formation_usage")
                .and_then(|v| v.clone().deserialized::<Usage>().ok())
                .unwrap_or_default();
            info.recall_usage = kv
                .get("recall_usage")
                .and_then(|v| v.clone().deserialized::<Usage>().ok())
                .unwrap_or_default();
            info.maintenance_usage = kv
                .get("maintenance_usage")
                .and_then(|v| v.clone().deserialized::<Usage>().ok())
                .unwrap_or_default();
        });
        info
    }

    pub fn formation_status(&self) -> FormationStatus {
        FormationStatus {
            id: self.id.clone(),
            concepts: self.memory.nexus.concepts.len(),
            propositions: self.memory.nexus.propositions.len(),
            conversations: self.memory.conversations.len(),
            formation_processing: self.formation.is_processing(),
            maintenance_processing: self.maintenance.is_processing(),
            formation_processed_id: self.formation.get_processed().unwrap_or_default(),
            maintenance_processed_id: self.maintenance.get_processed().unwrap_or_default(),
            maintenance_at: self.maintenance.get_processed_at(),
        }
    }

    pub async fn ingest(
        &self,
        user: Principal,
        input: StringOr<FormationInput>,
    ) -> Result<AgentOutput, BoxError> {
        let nodes = self
            .memory
            .nexus
            .concepts
            .len()
            .max(self.memory.conversations.len()) as u64;
        let tier = self.get_tier();
        if tier.allow_nodes() < nodes {
            return Err(format!(
                "node limit exceeded: {} nodes vs tier limit {}",
                nodes,
                tier.allow_nodes()
            )
            .into());
        }

        self.engine
            .agent_run(
                user,
                AgentInput {
                    name: FormationAgent::NAME.to_string(),
                    prompt: input.to_string(),
                    resources: vec![],
                    ..Default::default()
                },
            )
            .await
    }

    pub async fn query(
        &self,
        user: Principal,
        input: StringOr<RecallInput>,
    ) -> Result<AgentOutput, BoxError> {
        self.engine
            .agent_run(
                user,
                AgentInput {
                    name: RecallAgent::NAME.to_string(),
                    prompt: input.to_string(),
                    resources: vec![],
                    ..Default::default()
                },
            )
            .await
    }

    pub async fn maintenance(
        &self,
        user: Principal,
        mut input: MaintenanceInput,
    ) -> Result<AgentOutput, BoxError> {
        input.formation_id = self.formation.get_processed().unwrap_or_default();
        let rt = self
            .engine
            .agent_run(
                user,
                AgentInput {
                    name: MaintenanceAgent::NAME.to_string(),
                    prompt: StringOr::Value(&input).to_string(),
                    resources: vec![],
                    ..Default::default()
                },
            )
            .await?;

        self.maintenance
            .set_processed_at(input.scope, input.formation_id);

        Ok(rt)
    }

    pub async fn restart_formation(
        &self,
        user: Principal,
        conversation: u64,
    ) -> Result<(), BoxError> {
        let ctx = self.engine.ctx_with(
            user,
            "formation_memory",
            "formation_memory",
            Default::default(),
        )?;
        self.formation.start_process(ctx, conversation).await
    }

    pub async fn execute_kip_readonly(
        &self,
        mut req: anda_kip::Request,
    ) -> Result<anda_kip::Response, BoxError> {
        req.readonly = true;
        let (_, res) = req.execute(self.memory.nexus.as_ref()).await;
        Ok(res)
    }

    pub async fn get_conversation(
        &self,
        collection: Option<String>,
        id: u64,
    ) -> Result<Conversation, BoxError> {
        let collection = match collection {
            Some(name) if name == "recall" => self.recall.conversations.clone(),
            Some(name) if name == "maintenance" => self.maintenance.conversations.clone(),
            _ => self.memory.conversations.clone(),
        };

        let rt: Conversation = collection.get_as(id).await?;
        Ok(rt)
    }

    pub async fn list_conversations(
        &self,
        collection: Option<String>,
        cursor: Option<String>,
        limit: Option<usize>,
    ) -> Result<(Vec<Conversation>, Option<String>), BoxError> {
        use anda_db::query::{Filter, Query, RangeQuery};

        let collection = match collection {
            Some(name) if name == "recall" => self.recall.conversations.clone(),
            Some(name) if name == "maintenance" => self.maintenance.conversations.clone(),
            _ => self.memory.conversations.clone(),
        };
        let limit = limit.unwrap_or(10).min(100);
        let cursor = match BTree::from_cursor::<u64>(&cursor)? {
            Some(cursor) => cursor,
            None => collection.max_document_id() + 1,
        };

        let filter = Some(Filter::Field((
            "_id".to_string(),
            RangeQuery::Lt(Fv::U64(cursor)),
        )));

        let rt: Vec<Conversation> = collection
            .search_as(Query {
                search: None,
                filter,
                limit: Some(limit),
            })
            .await?;
        let cursor = if rt.len() >= limit {
            BTree::to_cursor(&rt.first().unwrap()._id)
        } else {
            None
        };
        Ok((rt, cursor))
    }

    async fn flush(&self) -> Result<(), BoxError> {
        self.db.flush().await?;
        Ok(())
    }

    async fn create(
        object_store: Arc<dyn ObjectStore>,
        db_config: DBConfig,
        creator: Principal,
        owner: Principal,
        tier: u32,
        now_ms: u64,
    ) -> Result<SpaceInfo, BoxError> {
        let id = db_config.name.clone();
        let db = AndaDB::create(object_store.clone(), db_config).await?;
        let tier = SpaceTier {
            tier,
            updated_at: now_ms,
        };

        db.set_extension_from("creator".to_string(), creator.to_string());
        db.set_extension_from("owner".to_string(), owner.to_string());
        db.set_extension_from("tier".to_string(), &tier);

        let db = Arc::new(db);
        let nexus =
            CognitiveNexus::connect(db.clone(), async |nexus| init_nexus_kip(nexus).await).await?;

        let nexus = Arc::new(nexus);
        let memory = MemoryManagement::connect(db.clone(), nexus.clone()).await?;
        Ok(SpaceInfo {
            id: id.clone(),
            name: None,
            description: None,
            owner: owner.to_string(),
            db_stats: db.stats(),
            concepts: nexus.concepts.len(),
            propositions: nexus.propositions.len(),
            conversations: memory.conversations.len(),
            public: false,
            tier,
            ..Default::default()
        })
    }

    async fn connect(
        object_store: Arc<dyn ObjectStore>,
        db_config: DBConfig,
        management: Arc<dyn Management>,
        http_client: reqwest::Client,
        models: Arc<Models>,
        pinned: bool,
    ) -> Result<Arc<Self>, BoxError> {
        let id = db_config.name.clone();
        let db = Arc::new(AndaDB::open(object_store.clone(), db_config).await?);
        let nexus =
            CognitiveNexus::connect(db.clone(), async |nexus| init_nexus_kip(nexus).await).await?;
        let mut schema = Conversation::schema()?;
        schema.with_version(3);

        let conversations = db
            .open_or_create_collection(
                schema.clone(),
                CollectionConfig {
                    name: "conversations".to_string(),
                    description: "conversations collection".to_string(),
                },
                async |collection| {
                    // set tokenizer
                    collection.set_tokenizer(jieba_tokenizer());
                    // create BTree index if not exists
                    collection.create_btree_index_nx(&["user"]).await?;
                    // remove old indexes if exists
                    collection.remove_btree_index(&["thread"]).await?;
                    collection.remove_btree_index(&["period"]).await?;
                    collection
                        .remove_bm25_index(&["messages", "resources", "artifacts"])
                        .await?;

                    Ok::<(), DBError>(())
                },
            )
            .await?;

        let recall_conversations = db
            .open_or_create_collection(
                schema.clone(),
                CollectionConfig {
                    name: "recall".to_string(),
                    description: "Recall conversations collection".to_string(),
                },
                async |collection| {
                    // set tokenizer
                    collection.set_tokenizer(jieba_tokenizer());
                    // create BTree index if not exists
                    collection.create_btree_index_nx(&["user"]).await?;
                    // remove old indexes if exists
                    collection.remove_btree_index(&["thread"]).await?;
                    collection.remove_btree_index(&["period"]).await?;
                    collection
                        .remove_bm25_index(&["messages", "resources", "artifacts"])
                        .await?;

                    Ok::<(), DBError>(())
                },
            )
            .await?;

        let maintenance_conversations = db
            .open_or_create_collection(
                schema.clone(),
                CollectionConfig {
                    name: "maintenance".to_string(),
                    description: "Maintenance conversations collection".to_string(),
                },
                async |collection| {
                    // set tokenizer
                    collection.set_tokenizer(jieba_tokenizer());
                    // create BTree index if not exists
                    collection.create_btree_index_nx(&["user"]).await?;
                    // remove old indexes if exists
                    collection.remove_btree_index(&["thread"]).await?;
                    collection.remove_btree_index(&["period"]).await?;
                    collection
                        .remove_bm25_index(&["messages", "resources", "artifacts"])
                        .await?;

                    Ok::<(), DBError>(())
                },
            )
            .await?;

        let resources = db
            .open_or_create_collection(
                Resource::schema()?,
                CollectionConfig {
                    name: "resources".to_string(),
                    description: "Resources collection".to_string(),
                },
                async |collection| {
                    // set tokenizer
                    collection.set_tokenizer(jieba_tokenizer());
                    // create BTree indexes if not exists
                    collection.create_btree_index_nx(&["tags"]).await?;
                    collection.create_btree_index_nx(&["hash"]).await?;
                    collection.create_btree_index_nx(&["mime_type"]).await?;
                    // remove old BM25 index if exists
                    collection
                        .remove_bm25_index(&["name", "description", "metadata"])
                        .await?;

                    Ok::<(), DBError>(())
                },
            )
            .await?;

        let memory = MemoryManagement {
            nexus: Arc::new(nexus),
            conversations,
            resources,
            kip_function_definitions: FUNCTION_DEFINITION.clone(),
        };

        // create a new models instance for each space to allow per-space customization in the future (e.g., different model providers or credentials)
        let models = Arc::new(Models::from_clone(models.as_ref()));
        let memory = Arc::new(memory);
        let memory_r = MemoryReadonly::new(memory.clone());
        let memory_tool = MemoryTool::new(memory.clone());

        let hooks = Arc::new(Hooks::new(db.clone()));
        let formation = Arc::new(FormationAgent::new(memory.clone(), hooks.clone(), 655350));
        let recall = Arc::new(RecallAgent::new(
            memory.clone(),
            recall_conversations,
            hooks.clone(),
            65535,
        ));
        let maintenance = Arc::new(MaintenanceAgent::new(
            memory.clone(),
            maintenance_conversations,
            hooks.clone(),
        ));
        // Build agent engine with all configured components
        let engine = Engine::builder()
            .with_management(management)
            .set_models(models.clone())
            .register_tool(memory.clone())?
            .register_tool(Arc::new(memory_r))?
            .register_tool(Arc::new(memory_tool))?
            .register_agent(formation.clone(), None)?
            .register_agent(recall.clone(), None)?
            .register_agent(maintenance.clone(), None)?
            .export_tools(vec![MemoryTool::NAME.to_string()])
            .export_agents(vec![
                RecallAgent::NAME.to_string(),
                FormationAgent::NAME.to_string(),
                MaintenanceAgent::NAME.to_string(),
            ]);

        // Initialize and start the server
        let engine = engine.build(RecallAgent::NAME.to_string()).await?;
        let this = Arc::new(Self {
            id,
            db: db.clone(),
            http_client,
            models,
            formation,
            recall,
            maintenance,
            memory,
            engine,
            pinned,
        });
        hooks.bind_space(Arc::downgrade(&this));

        if let Some(cfg) = db.get_extension_as::<ModelConfig>("byok") {
            let model = build_model(this.http_client.clone(), cfg);
            this.models.set_model(model);
        }
        if let Some(conversation) = this.formation.get_processed() {
            // Resume formation process if it was interrupted before
            let _ = this
                .restart_formation(Principal::anonymous(), conversation + 1)
                .await;
        }
        Ok(this)
    }
}

struct Hooks {
    db: Arc<AndaDB>,
    space: OnceLock<Weak<Space>>,
}

impl Hooks {
    fn new(db: Arc<AndaDB>) -> Self {
        Self {
            db,
            space: OnceLock::new(),
        }
    }

    fn bind_space(&self, space: Weak<Space>) {
        let _ = self.space.set(space);
    }

    fn space(&self) -> Option<Arc<Space>> {
        self.space.get().and_then(Weak::upgrade)
    }
}

#[async_trait::async_trait]
impl AgentHook for Hooks {
    async fn on_conversation_end(&self, agent_name: &str, conversation: &Conversation) {
        match agent_name {
            "recall_memory" => {
                let _ = self
                    .db
                    .set_extension_from_with("recall_memory".to_string(), |v| {
                        let mut usage: Usage = v.unwrap_or_default();
                        usage.accumulate(&conversation.usage);
                        Some(usage)
                    });
            }
            "maintenance_memory" => {
                let _ = self
                    .db
                    .set_extension_from_with("maintenance_usage".to_string(), |v| {
                        let mut usage: Usage = v.unwrap_or_default();
                        usage.accumulate(&conversation.usage);
                        Some(usage)
                    });
            }
            "formation_memory" => {
                let _ = self
                    .db
                    .set_extension_from_with("formation_usage".to_string(), |v| {
                        let mut usage: Usage = v.unwrap_or_default();
                        usage.accumulate(&conversation.usage);
                        Some(usage)
                    });
            }
            _ => {}
        }
    }

    async fn try_start_formation(&self) {
        let space = match self.space() {
            Some(space) => space,
            None => return,
        };

        if let Some(id) = space.formation.get_processed() {
            let _ = space
                .restart_formation(Principal::anonymous(), id + 1)
                .await;
        }
    }

    async fn try_start_maintenance(&self, formation_id: DocumentId) -> Option<DocumentId> {
        let space = match self.space() {
            Some(space) => space,
            None => return None,
        };

        let at = space.maintenance.get_processed_at();
        let timestamp = Some(rfc3339_datetime_now());
        let input = if formation_id >= at.full + 168 {
            Some(MaintenanceInput {
                trigger: "scheduled".to_string(),
                scope: MaintenanceScope::Full,
                timestamp,
                parameters: None,
                formation_id,
            })
        } else if formation_id >= at.quick.max(at.full) + 42 {
            Some(MaintenanceInput {
                trigger: "scheduled".to_string(),
                scope: MaintenanceScope::Quick,
                timestamp,
                parameters: None,
                formation_id,
            })
        } else if formation_id >= at.daydream.max(at.quick).max(at.full) + 21 {
            Some(MaintenanceInput {
                trigger: "scheduled".to_string(),
                scope: MaintenanceScope::Daydream,
                timestamp,
                parameters: None,
                formation_id,
            })
        } else {
            None
        };

        if let Some(input) = input {
            match space.maintenance(Principal::anonymous(), input).await {
                Ok(rt) => {
                    return rt.conversation;
                }
                Err(err) => {
                    log::error!(formation_id; "scheduled maintenance failed to start: {}", err);
                }
            }
        }

        None
    }
}

async fn init_nexus_kip(nexus: &CognitiveNexus) -> Result<(), KipError> {
    if !nexus
        .has_concept(&ConceptPK::Object {
            r#type: PERSON_TYPE.to_string(),
            name: META_SELF_NAME.to_string(),
        })
        .await
    {
        // uuc56-gyb: Principal::from_slice(&[1])
        let kml = &[
            &PERSON_SELF_KIP.replace("$self_reserved_principal_id", "uuc56-gyb"),
            PERSON_SYSTEM_KIP,
        ]
        .join("\n");

        let result = nexus.execute_kml(parse_kml(kml)?, false).await?;
        log::info!(result:serde = result; "Init $self and $system");
    }
    Ok(())
}
