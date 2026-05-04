use anda_core::{AgentOutput, BoxError};
use anda_db::database::AndaDB;
use anda_engine::{
    context::{AgentCtx, Web3SDK},
    engine::{Engine, EngineRef},
    extension::{fs, note, shell, skill, todo},
    management::{BaseManagement, Visibility},
    memory::Conversations,
    model::Models,
    store::Store,
    unix_ms,
};
use anda_engine_server::handler::{AppState, anda_engine};
use anda_web3_client::client::Client as Web3Client;
use async_trait::async_trait;
use axum::{Router, response::IntoResponse, routing};
use serde_json::json;
use sha3::{Digest, Sha3_384};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
};

mod agent;
mod conversation;
mod goal;
mod prompt;
mod side;

use crate::util::{
    http_client::build_http_client,
    key::{ClaimsSetBuilder, Ed25519Key, Ed25519PubKey, iana},
};
use crate::{brain, config, cron, transcription::TranscriptionManager, tts::TtsManager};
use agent::*;

pub use conversation::*;

pub struct Engines {
    state: AppState,
}

#[async_trait]
pub trait CompletionHook: Send + Sync {
    async fn on_completion(&self, _ctx: &AgentCtx, _output: &AgentOutput) {}
}

pub struct EngineConfig {
    pub id_key: Ed25519Key,
    pub managers: Vec<Ed25519PubKey>,
    pub models: Models,
    pub brain_base_url: String,
    pub home_dir: PathBuf,
    pub skills_dir: PathBuf,
    pub workspace_dir: PathBuf,
    pub tts: config::TtsConfig,
    pub transcription: config::TranscriptionConfig,
    pub https_proxy: Option<String>,
}

impl Engines {
    pub async fn new(
        cfg: EngineConfig,
        db: Arc<AndaDB>,
        engine_ref: Arc<EngineRef>,
        cron_runtime: Arc<cron::CronRuntime>,
        completion_hooks: Vec<Arc<dyn CompletionHook>>,
    ) -> Result<Self, BoxError> {
        let root_secret: [u8; 48] = {
            let mut hasher = Sha3_384::new();
            hasher.update(cfg.id_key.as_bytes());
            hasher.finalize().into()
        };
        let outer_http_client = build_http_client(cfg.https_proxy.clone(), |client| client)?;

        // Initialize Web3 client for ICP network interaction
        let web3 = Web3Client::builder()
            .with_identity(cfg.id_key.identity())
            .with_root_secret(root_secret)
            .with_http_client(outer_http_client.clone())
            .build()
            .await?;
        let web3 = Arc::new(web3);
        let my_principal = web3.get_principal();

        let managers = cfg.managers.iter().map(|k| k.id()).collect();
        let management = Arc::new(BaseManagement {
            controller: my_principal,
            managers,
            visibility: Visibility::Protected,
        });

        let web3 = Arc::new(Web3SDK::from_web3(web3));
        let object_store = db.object_store().clone();

        let brain_token = cfg.id_key.sign_cwt(
            ClaimsSetBuilder::new()
                .audience("*".to_string())
                .claim(iana::CwtClaimName::Scope, "*".into())
                .build(),
        )?;
        let brain_http_client = build_http_client(None, |client| client.no_proxy())?;
        let brain_client = brain::Client::new(cfg.brain_base_url, Some(brain_token))
            .with_http_client(brain_http_client);

        let conversations = Conversations::connect(db.clone(), "bot".to_string()).await?;
        let conversations_tool = ConversationsTool::new(conversations.clone());
        let tts_manager = {
            let manager = Arc::new(TtsManager::new(&cfg.tts, outer_http_client.clone())?);
            manager.is_enabled().then_some(manager)
        };
        let transcription_manager = {
            let manager = Arc::new(TranscriptionManager::new(
                &cfg.transcription,
                outer_http_client.clone(),
            )?);
            manager.is_enabled().then_some(manager)
        };

        let shell_tool = {
            let runtime = Arc::new(shell::NativeRuntime::new(cfg.workspace_dir.clone()));
            shell::ShellTool::new(runtime, HashMap::new(), None)
        };
        let skills_tool = Arc::new(skill::SkillManager::new(cfg.skills_dir));
        let bot = AndaBot::new(
            brain_client.clone(),
            conversations,
            completion_hooks,
            cfg.home_dir.clone(),
            skills_tool.clone(),
            tts_manager.clone(),
            transcription_manager.clone(),
        );
        let mut engine_builder = Engine::builder()
            .with_web3_client(web3)
            .with_store(Store::new(object_store))
            .with_management(management)
            .with_models(Arc::new(cfg.models))
            .register_tool(Arc::new(brain_client))?
            .register_tool(Arc::new(shell_tool))?
            .register_tool(skills_tool.clone())?
            .register_tool(Arc::new(note::NoteTool::new()))?
            .register_tool(Arc::new(todo::TodoTool::new()))?
            .register_tool(Arc::new(fs::ReadFileTool::new(cfg.workspace_dir.clone())))?
            .register_tool(Arc::new(fs::SearchFileTool::new(cfg.workspace_dir.clone())))?
            .register_tool(Arc::new(fs::EditFileTool::new(cfg.workspace_dir.clone())))?
            .register_tool(Arc::new(fs::WriteFileTool::new(cfg.workspace_dir.clone())))?
            .register_tool(Arc::new(conversations_tool))?
            .register_tool(Arc::new(cron::CreateCronTool::new(cron_runtime.clone())))?
            .register_tool(Arc::new(cron::ListCronJobsTool::new(cron_runtime.clone())))?
            .register_tool(Arc::new(cron::ManageCronJobTool::new(cron_runtime.clone())))?
            .register_tool(Arc::new(cron::ListCronRunsTool::new(cron_runtime)))?;

        if let Some(manager) = tts_manager {
            engine_builder = engine_builder.register_tool(manager)?;
        }
        if let Some(manager) = transcription_manager {
            engine_builder = engine_builder.register_tool(manager)?;
        }

        let engine = engine_builder
            .register_agent(Arc::new(bot), None)?
            .export_tools(vec![ConversationsTool::NAME.to_string()]);

        // Initialize and start the server
        let engine = engine.build(AndaBot::NAME.to_string()).await?;
        let engine = Arc::new(engine);
        engine_ref.bind(Arc::downgrade(&engine));
        skills_tool.load().await?;
        engine.sub_agents_manager().insert(skills_tool);

        let default_engine = engine.id();
        let mut engines = BTreeMap::new();
        engines.insert(default_engine, engine);
        let engines = Arc::new(engines);

        let state = AppState {
            engines,
            default_engine,
            start_time_ms: unix_ms(),
            extra_info: Arc::new(BTreeMap::new()),
            ed25519_pubkeys: Arc::new(cfg.managers.into_iter().map(|k| k.into()).collect()),
        };
        Ok(Self { state })
    }

    pub fn into_router(self) -> Router<()> {
        let app: Router<()> = Router::new()
            .route("/", routing::get(get_version))
            .route("/engine/{*id}", routing::post(anda_engine))
            .with_state(self.state);
        app
    }
}

pub async fn get_version() -> impl IntoResponse {
    let info = json!({
        "name": config::APP_NAME,
        "version": config::APP_VERSION,

    });
    axum::Json(info)
}
