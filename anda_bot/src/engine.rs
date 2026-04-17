use anda_core::BoxError;
use anda_db::database::AndaDB;
use anda_engine::{
    context::Web3SDK,
    engine::Engine,
    extension::{fs, note, shell, skill, todo},
    management::{BaseManagement, Visibility},
    memory::Conversations,
    model::Models,
    store::Store,
    unix_ms,
};
use anda_engine_server::handler::{AppState, anda_engine};
use anda_hippocampus::{model::build_model, types::ModelConfig};
use anda_web3_client::client::{Client as Web3Client, identity_from_secret};
use axum::{Router, routing};
use sha3::{Digest, Sha3_384};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::PathBuf,
    sync::Arc,
};

mod agent;

use crate::brain;
use crate::util::{cose::to_ed25519_pubkey, http_client::build_http_client};
use agent::*;

pub struct Engines {
    state: AppState,
}

pub struct EngineConfig {
    pub ed25519_secret: [u8; 32],
    pub model: ModelConfig,
    pub brain_base_url: String,
    pub work_dir: PathBuf,
    pub skills_dir: PathBuf,
    pub sandbox_dir: Option<PathBuf>,
    pub https_proxy: Option<String>,
}

impl Engines {
    pub async fn new(cfg: EngineConfig, db: Arc<AndaDB>) -> Result<Self, BoxError> {
        let ed25519_pubkey = to_ed25519_pubkey(&cfg.ed25519_secret);
        let identity = identity_from_secret(cfg.ed25519_secret);
        let root_secret: [u8; 48] = {
            let mut hasher = Sha3_384::new();
            hasher.update(cfg.ed25519_secret);
            hasher.finalize().into()
        };
        let http_client = build_http_client(cfg.https_proxy.clone(), |client| client)?;

        // Initialize Web3 client for ICP network interaction
        let web3 = Web3Client::builder()
            .with_identity(Arc::new(identity))
            .with_root_secret(root_secret)
            .with_http_client(http_client.clone())
            .build()
            .await?;
        let web3 = Arc::new(web3);
        let my_principal = web3.get_principal();

        let managers = BTreeSet::new();
        let management = Arc::new(BaseManagement {
            controller: my_principal,
            managers,
            visibility: Visibility::Protected,
        });

        let models = Models::default();
        models.set_model(build_model(http_client.clone(), cfg.model));

        let web3 = Arc::new(Web3SDK::from_web3(web3));
        let object_store = db.object_store().clone();
        let brain_http_client = build_http_client(None, |client| client.no_proxy())?;
        let brain_client =
            brain::Client::new(cfg.brain_base_url, None).with_http_client(brain_http_client);
        let conversations = Conversations::connect(db.clone(), "bot".to_string()).await?;
        let bot = AndaBot::new(brain_client.clone(), conversations);

        let shell_tool = {
            let runtime: Arc<dyn shell::Executor> = if let Some(sandbox) = cfg.sandbox_dir {
                Arc::new(shell::sandbox::SandboxRuntime::new(sandbox).await?)
            } else {
                Arc::new(shell::NativeRuntime::new(cfg.work_dir.clone()))
            };
            shell::ShellTool::new(runtime, HashMap::new())
        };

        let engine = Engine::builder()
            .with_web3_client(web3)
            .with_store(Store::new(object_store))
            .with_management(management)
            .set_models(Arc::new(models))
            .register_tool(Arc::new(brain_client))?
            .register_tool(Arc::new(shell_tool))?
            .register_tool(Arc::new(note::NoteTool::new()))?
            .register_tool(Arc::new(todo::TodoTool::new()))?
            .register_tool(Arc::new(fs::ReadFileTool::new(cfg.work_dir.clone())))?
            .register_tool(Arc::new(fs::SearchFileTool::new(cfg.work_dir.clone())))?
            .register_tool(Arc::new(fs::EditFileTool::new(cfg.work_dir.clone())))?
            .register_tool(Arc::new(fs::WriteFileTool::new(cfg.work_dir.clone())))?
            .register_agent(Arc::new(bot), None)?;

        // Initialize and start the server
        let engine = engine.build(AndaBot::NAME.to_string()).await?;
        let skills_tool = Arc::new(skill::SkillManager::new(cfg.skills_dir));
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
            ed25519_pubkeys: Arc::new(vec![ed25519_pubkey]),
        };
        Ok(Self { state })
    }

    pub fn into_router(self) -> Router<()> {
        let app: Router<()> = Router::new()
            .route("/engine/{*id}", routing::post(anda_engine))
            .with_state(self.state);
        app
    }
}
