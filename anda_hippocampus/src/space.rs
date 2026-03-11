use anda_cognitive_nexus::{CognitiveNexus, ConceptPK};
use anda_core::{AgentInput, AgentOutput, BoxError, FunctionDefinition, Principal};
use anda_db::{
    database::{AndaDB, DBConfig},
    storage::StorageStats,
};
use anda_engine::{
    engine::Engine,
    management::Management,
    memory::{MemoryManagement, MemoryReadonly, MemoryTool, SearchConversationsTool},
    model::Model,
    unix_ms,
};
use anda_kip::{META_SELF_NAME, PERSON_SELF_KIP, PERSON_SYSTEM_KIP, PERSON_TYPE, parse_kml};
use ic_auth_types::ByteBufB64;
use ic_cose_types::cose::{
    cwt::{ClaimsSet, cwt_from, get_scope},
    ed25519::VerifyingKey,
    sign1::cose_sign1_from,
};
use object_store::ObjectStore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeMap,
    str::FromStr,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{OnceCell, RwLock};
use tokio_util::sync::CancellationToken;

use crate::agents::{FormationAgent, MaintenanceAgent, RecallAgent};
use crate::types::{FormationInput, MaintenanceInput, RecallInput, SpaceId};

pub static FUNCTION_DEFINITION: LazyLock<FunctionDefinition> = LazyLock::new(|| {
    serde_json::from_value(json!({
        "name": "execute_kip",
        "description": "Executes one or more KIP (Knowledge Interaction Protocol) commands against the Cognitive Nexus to interact with your persistent memory.",
        "parameters": {
            "type": "object",
            "properties": {
                "commands": {
                    "type": "array",
                    "description": "An array of KIP commands for batch execution (reduces round-trips). Commands are executed sequentially; execution stops on first error.",
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

pub struct Token {
    pub user: Principal,
    pub audience: String,
    pub scope: String,
}

impl Token {
    pub fn from_claims(claims: ClaimsSet) -> Result<Self, BoxError> {
        let scope = get_scope(&claims).unwrap_or_default();
        let user = claims
            .subject
            .ok_or("missing 'sub' claim")?
            .parse::<Principal>()
            .map_err(|_| "invalid 'sub' claim")?;

        let audience = claims.audience.unwrap_or_default();
        Ok(Self {
            user,
            audience,
            scope,
        })
    }
}

#[derive(Clone)]
pub struct AppState {
    spaces: Arc<RwLock<BTreeMap<String, Arc<SpaceEntry>>>>,
    object_store: Arc<dyn ObjectStore>,
    db_config: Arc<DBConfig>,
    model: Model,
    fallback_model: Model,
    ed25519_pubkeys: Vec<VerifyingKey>,
    management: Arc<dyn Management>,

    pub app_name: String,
    pub app_version: String,
    pub sharding: u32,
}

impl AppState {
    pub fn new(
        object_store: Arc<dyn ObjectStore>,
        db_config: Arc<DBConfig>,
        management: Arc<dyn Management>,
        model: Model,
        fallback_model: Model,
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
            model,
            fallback_model,
            ed25519_pubkeys,
            app_name,
            app_version,
            sharding,
        }
    }

    pub fn check_admin(&self, token: &str, audience: &str, scope: &str) -> Result<Token, BoxError> {
        if self.ed25519_pubkeys.is_empty() {
            return Ok(Token {
                user: Principal::management_canister(),
                audience: audience.to_string(),
                scope: scope.to_string(),
            });
        }

        let token = self.check_auth(token, audience, scope)?;
        if !self.management.is_manager(&token.user) {
            return Err("admin access required".into());
        }

        Ok(token)
    }

    pub fn check_auth(&self, token: &str, audience: &str, scope: &str) -> Result<Token, BoxError> {
        if self.ed25519_pubkeys.is_empty() {
            return Ok(Token {
                user: Principal::anonymous(),
                audience: audience.to_string(),
                scope: scope.to_string(),
            });
        }

        let data = ByteBufB64::from_str(token)?;
        let cs1 = cose_sign1_from(&data, &[], &[], &self.ed25519_pubkeys)?;
        let claims = cwt_from(&cs1.payload.unwrap_or_default(), (unix_ms() / 1000) as i64)?;
        let token = Token::from_claims(claims)?;
        if token.audience != audience && token.audience != "*" {
            return Err("invalid audience".into());
        }

        if !token.scope.contains(scope) && token.scope != "*" {
            return Err("insufficient scope".into());
        }
        Ok(token)
    }

    pub async fn create_space(
        &self,
        creator: Principal,
        owner: Principal,
        id: String,
    ) -> Result<SpaceStatus, BoxError> {
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
            self.sharding,
        )
        .await
    }

    pub async fn load_space(&self, space_id: &str) -> Result<Arc<Space>, BoxError> {
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
                Ok::<Arc<Space>, BoxError>(Arc::new(
                    Space::connect(
                        self.object_store.clone(),
                        db_config,
                        self.management.clone(),
                        self.model.clone(),
                        self.fallback_model.clone(),
                        self.sharding,
                    )
                    .await?,
                ))
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
        let idle_timeout_ms: u64 = 20 * 60 * 1000;

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    // Flush all spaces before shutting down
                    let spaces = self.spaces.read().await;
                    for (id, entry) in spaces.iter() {
                        if let Some(space) = entry.cell.get()
                            && let Err(err) = space.flush().await {
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

            let mut to_evict = Vec::new();
            for (id, entry) in &entries {
                let Some(space) = entry.cell.get() else {
                    continue;
                };

                if now.saturating_sub(entry.last_access_ms()) > idle_timeout_ms {
                    // Flush before eviction
                    if let Err(err) = space.flush().await {
                        log::error!(space_id = id; "flush before eviction failed: {err:?}");
                    }
                    to_evict.push(id.clone());
                } else {
                    // Periodic flush for active spaces
                    if let Err(err) = space.flush().await {
                        log::error!(space_id = id; "periodic flush failed: {err:?}");
                    }
                }
            }

            if !to_evict.is_empty() {
                let mut spaces = self.spaces.write().await;
                for id in &to_evict {
                    spaces.remove(id);
                    log::info!(space_id = id; "evicted idle space");
                }
            }
        }
    }
}

pub struct Space {
    id: String,
    sharding: u32,
    db: Arc<AndaDB>,
    memory: Arc<MemoryManagement>,
    engine: Engine,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SpaceStatus {
    pub space_id: String,
    pub owner: String,
    pub db_stats: StorageStats,
    pub concepts: usize,
    pub propositions: usize,
    pub conversations: usize,
}

impl Space {
    pub fn space_id(&self) -> String {
        SpaceId {
            id: self.id.clone(),
            sharding: self.sharding,
        }
        .to_string()
    }

    pub async fn get_status(&self) -> SpaceStatus {
        SpaceStatus {
            space_id: self.space_id(),
            owner: self
                .db
                .get_extension("owner")
                .and_then(|v| String::try_from(v).ok())
                .unwrap_or_default(),
            db_stats: self.db.stats(),
            concepts: self.memory.nexus.concepts.len(),
            propositions: self.memory.nexus.propositions.len(),
            conversations: self.memory.conversations.len(),
        }
    }

    pub async fn ingest(
        &self,
        user: Principal,
        input: FormationInput,
    ) -> Result<AgentOutput, BoxError> {
        self.engine
            .agent_run(
                user,
                AgentInput {
                    name: FormationAgent::NAME.to_string(),
                    prompt: serde_json::to_string(&input)?,
                    resources: vec![],
                    ..Default::default()
                },
            )
            .await
    }

    pub async fn query(
        &self,
        user: Principal,
        input: RecallInput,
    ) -> Result<AgentOutput, BoxError> {
        self.engine
            .agent_run(
                user,
                AgentInput {
                    name: RecallAgent::NAME.to_string(),
                    prompt: serde_json::to_string(&input)?,
                    resources: vec![],
                    ..Default::default()
                },
            )
            .await
    }

    pub async fn maintenance(
        &self,
        user: Principal,
        input: MaintenanceInput,
    ) -> Result<AgentOutput, BoxError> {
        self.engine
            .agent_run(
                user,
                AgentInput {
                    name: MaintenanceAgent::NAME.to_string(),
                    prompt: serde_json::to_string(&input)?,
                    resources: vec![],
                    ..Default::default()
                },
            )
            .await
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
        sharding: u32,
    ) -> Result<SpaceStatus, BoxError> {
        let id = db_config.name.clone();
        let db = AndaDB::create(object_store.clone(), db_config).await?;

        db.set_extension("creator".to_string(), creator.to_string().into());
        db.set_extension("owner".to_string(), owner.to_string().into());

        let db = Arc::new(db);
        let nexus = CognitiveNexus::connect(db.clone(), async |nexus| {
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
        })
        .await?;

        let nexus = Arc::new(nexus);
        let memory = MemoryManagement::connect(db.clone(), nexus.clone()).await?;
        Ok(SpaceStatus {
            space_id: SpaceId { id, sharding }.to_string(),
            owner: owner.to_string(),
            db_stats: db.stats(),
            concepts: nexus.concepts.len(),
            propositions: nexus.propositions.len(),
            conversations: memory.conversations.len(),
        })
    }

    async fn connect(
        object_store: Arc<dyn ObjectStore>,
        db_config: DBConfig,
        management: Arc<dyn Management>,
        model: Model,
        fallback_model: Model,
        sharding: u32,
    ) -> Result<Self, BoxError> {
        let id = db_config.name.clone();
        let db = Arc::new(AndaDB::connect(object_store.clone(), db_config).await?);
        let nexus = CognitiveNexus::connect(db.clone(), async |nexus| {
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
        })
        .await?;

        let mut memory = MemoryManagement::connect(db.clone(), Arc::new(nexus))
            .await?
            .with_kip_function_definitions(FUNCTION_DEFINITION.clone());
        memory.disable_kip_logging();

        let memory = Arc::new(memory);
        let memory_r = MemoryReadonly::new(memory.clone());
        let memory_tool = MemoryTool::new(memory.clone());
        let search_conversations_tool = SearchConversationsTool::new(memory.clone());

        let formation = FormationAgent::new(memory.clone(), 655350);
        let recall = RecallAgent::new(65535);
        let maintenance = MaintenanceAgent::new(memory.clone());
        // Build agent engine with all configured components
        let engine = Engine::builder()
            .with_management(management)
            .with_model(model)
            .with_fallback_model(fallback_model)
            .register_tool(memory.clone())?
            .register_tool(memory_r)?
            .register_tool(memory_tool)?
            .register_tool(search_conversations_tool)?
            .register_agent(formation, None)?
            .register_agent(recall, None)?
            .register_agent(maintenance, None)?
            .export_tools(vec![MemoryTool::NAME.to_string()]);

        // Initialize and start the server
        let engine = engine.build(RecallAgent::NAME.to_string()).await?;

        Ok(Self {
            id,
            sharding,
            db,
            memory,
            engine,
        })
    }
}
