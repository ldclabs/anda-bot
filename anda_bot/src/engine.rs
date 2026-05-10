use anda_core::{AgentOutput, BoxError, Principal, Tool};
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
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};

mod agent;
mod conversation;
mod goal;
mod prompt;
mod side;
mod system;

use crate::util::{
    http_client::{NO_PROXY, build_http_client},
    key::{ClaimsSetBuilder, Ed25519Key, Ed25519PubKey, iana},
};
use crate::{brain, config, cron, transcription::TranscriptionManager, tts::TtsManager};

pub use agent::{AndaBot, AndaBotToolArgs, SessionState, SessionSummary};
pub use conversation::*;
pub use goal::GoalTool;
pub(crate) use system::{external_user_prompt, system_runtime_prompt};

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
    pub workspaces: Vec<PathBuf>,
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

        let mut managers = BTreeSet::from([Principal::management_canister()]);
        managers.extend(cfg.managers.iter().map(|k| k.id()));
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

        let default_workspace = cfg
            .workspaces
            .first()
            .cloned()
            .ok_or("At least one workspace must be provided")?;
        let conversations = Conversations::connect(db.clone(), "bot".to_string()).await?;
        let conversations_tool = Arc::new(ConversationsTool::new(
            conversations.clone(),
            default_workspace.to_string_lossy().to_string(),
        ));
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
            let runtime = Arc::new(shell::NativeRuntime::new(default_workspace));
            let mut envs = vec![shell::CustomEnv {
                key: "ANDA_HOME".to_string(),
                value: cfg.home_dir.to_string_lossy().to_string(),
                default: true,
                description:
                    "The home directory for AndaBot, used for storing data and configuration."
                        .to_string(),
            }];

            if let Some(proxy) = &cfg.https_proxy {
                envs.push(shell::CustomEnv {
                    key: "http_proxy".to_string(),
                    value: proxy.clone(),
                    default: true,
                    description: "Proxy server for HTTP requests.".to_string(),
                });
                envs.push(shell::CustomEnv {
                    key: "https_proxy".to_string(),
                    value: proxy.clone(),
                    default: true,
                    description: "Proxy server for HTTPS requests.".to_string(),
                });
                envs.push(shell::CustomEnv {
                    key: "no_proxy".to_string(),
                    value: NO_PROXY.to_string(),
                    default: true,
                    description: "Comma-separated list of hosts that should bypass the proxy."
                        .to_string(),
                });
            }
            shell::ShellTool::new_with_custom_envs(runtime, envs, None)
        };
        let skills_tool = Arc::new(
            skill::SkillManager::new(cfg.skills_dir).with_default_skill_tools(vec![
                "shell".to_string(),
                "read_file".to_string(),
                "search_file".to_string(),
                "note".to_string(),
                "tools_select".to_string(),
            ]),
        );
        let bot = Arc::new(AndaBot::new(
            brain_client.clone(),
            cfg.home_dir.clone(),
            conversations_tool.clone(),
            completion_hooks,
            skills_tool.clone(),
            tts_manager.clone(),
            transcription_manager.clone(),
        ));
        let mut engine_builder = Engine::builder()
            .with_web3_client(web3)
            .with_store(Store::new(object_store))
            .with_management(management)
            .with_models(Arc::new(cfg.models))
            .register_tool(Arc::new(brain_client))?
            .register_tool(Arc::new(shell_tool))?
            .register_tool(Arc::new(note::NoteTool::new()))?
            .register_tool(Arc::new(GoalTool::new()))?
            .register_tool(Arc::new(todo::TodoTool::new()))?
            .register_tool(Arc::new(fs::ReadFileTool::with_workspaces(
                cfg.workspaces.clone(),
            )))?
            .register_tool(Arc::new(fs::SearchFileTool::with_workspaces(
                cfg.workspaces.clone(),
            )))?
            .register_tool(Arc::new(fs::EditFileTool::with_workspaces(
                cfg.workspaces.clone(),
            )))?
            .register_tool(Arc::new(fs::WriteFileTool::with_workspaces(
                cfg.workspaces.clone(),
            )))?
            .register_tool(Arc::new(cron::CreateCronTool::new(cron_runtime.clone())))?
            .register_tool(Arc::new(cron::ListCronJobsTool::new(cron_runtime.clone())))?
            .register_tool(Arc::new(cron::ManageCronJobTool::new(cron_runtime.clone())))?
            .register_tool(Arc::new(cron::ListCronRunsTool::new(cron_runtime)))?
            .register_tool(skills_tool.clone())?
            .register_tool(conversations_tool.clone())?
            .register_tool(bot.clone())?;

        if let Some(manager) = tts_manager {
            engine_builder = engine_builder.register_tool(manager)?;
        }
        if let Some(manager) = transcription_manager {
            engine_builder = engine_builder.register_tool(manager)?;
        }

        let engine = engine_builder
            .register_agent(bot.clone(), None)?
            .export_tools(vec![
                ConversationsTool::NAME.to_string(),
                Tool::name(bot.as_ref()),
            ]);

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
