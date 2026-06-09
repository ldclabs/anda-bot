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
use axum::{
    Json as AxumJson, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing,
};
use serde_json::json;
use sha3::{Digest, Sha3_384};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

mod agent;
mod browser;
mod browser_ws;
mod conversation;
mod goal;
mod model_retry;
mod multimodal;
mod prompt;
mod resources;
mod shell_runtime;
mod side;
mod system;

use crate::util::{
    http_client::{NO_PROXY, build_http_client},
    key::{ClaimsSetBuilder, Ed25519Key, Ed25519PubKey, iana},
};
use crate::{
    auto_update::AutoUpdater, brain, config, cron, transcription::TranscriptionManager,
    tts::TtsManager,
};
use browser_ws::{BrowserVoiceCapabilities, BrowserWebSocketState, browser_websocket};

pub use agent::{AndaBot, AndaBotToolArgs, SessionRequestMeta, SessionState, SessionSummary};
pub use browser::*;
pub use conversation::*;
pub use goal::GoalTool;
pub use multimodal::MediaUnderstandingAgent;
pub(crate) use prompt::PromptCommand;
pub use resources::ResourceStore;
pub(crate) use system::{external_user_prompt_with_space, system_runtime_prompt};

// Empty model labels resolve through Models::get_model(), which tracks the active model.
const ACTIVE_MODEL_LABEL: &str = "";

pub struct Engines {
    state: AppState,
    browser_bridge: Arc<BrowserBridge>,
    voice_capabilities: BrowserVoiceCapabilities,
    auto_updater: Arc<AutoUpdater>,
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
    pub auto_updater: Arc<AutoUpdater>,
}

#[derive(Clone)]
struct AutoUpdateRouteState {
    app: AppState,
    auto_updater: Arc<AutoUpdater>,
}

#[derive(Clone)]
struct DaemonControlRouteState {
    app: AppState,
    cancel_token: CancellationToken,
}

impl Engines {
    pub async fn new(
        cfg: EngineConfig,
        db: Arc<AndaDB>,
        engine_ref: Arc<EngineRef>,
        cron_runtime: Arc<cron::CronRuntime>,
        completion_hooks: Vec<Arc<dyn CompletionHook>>,
        active_im_channels: Vec<String>,
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
        let resource_store = Arc::new(ResourceStore::connect(db.clone()).await?);
        let conversations_tool = Arc::new(ConversationsTool::new(
            conversations.clone(),
            default_workspace.to_string_lossy().to_string(),
        ));
        let browser_bridge = Arc::new(BrowserBridge::new());
        let chrome_tabs_tool = Arc::new(
            ChromeBrowserTool::tabs(browser_bridge.clone())
                .with_screenshot_workspace(default_workspace.clone()),
        );
        let chrome_page_tool = Arc::new(
            ChromeBrowserTool::page(browser_bridge.clone())
                .with_screenshot_workspace(default_workspace.clone()),
        );
        let chrome_input_tool = Arc::new(
            ChromeBrowserTool::input(browser_bridge.clone())
                .with_screenshot_workspace(default_workspace.clone()),
        );
        let chrome_script_tool = Arc::new(
            ChromeBrowserTool::script(browser_bridge.clone())
                .with_screenshot_workspace(default_workspace.clone()),
        );
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
            let runtime =
                Arc::new(shell_runtime::NativeShellRuntime::new(default_workspace).insecure());
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
        let additional_skills_dirs = std::env::home_dir()
            .map(|home_dir| vec![home_dir.join(".agents").join("skills")])
            .unwrap_or_default();
        let skills_tool = Arc::new(
            skill::SkillManager::new_with_dirs(cfg.skills_dir, additional_skills_dirs)
                .with_default_skill_tools(vec![
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
            resource_store.clone(),
            completion_hooks,
            skills_tool.clone(),
            chrome_tabs_tool.clone(),
            tts_manager.clone(),
            transcription_manager.clone(),
            active_im_channels,
        ));
        let image_understanding_agent = Arc::new(
            MediaUnderstandingAgent::image(cfg.workspaces.clone())
                .with_http_client(outer_http_client.clone()),
        );
        let audio_understanding_agent = Arc::new(
            MediaUnderstandingAgent::audio(cfg.workspaces.clone())
                .with_http_client(outer_http_client.clone()),
        );
        let video_understanding_agent = Arc::new(
            MediaUnderstandingAgent::video(cfg.workspaces.clone())
                .with_http_client(outer_http_client.clone()),
        );
        let other_understanding_agent = Arc::new(
            MediaUnderstandingAgent::other(cfg.workspaces.clone())
                .with_http_client(outer_http_client.clone()),
        );
        let voice_capabilities = BrowserVoiceCapabilities {
            transcription: transcription_manager
                .as_ref()
                .map(|manager| manager.supported_audio_formats())
                .unwrap_or_default(),
            tts: tts_manager
                .as_ref()
                .map(|manager| manager.supported_audio_formats())
                .unwrap_or_default(),
        };
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
            .register_tool(Arc::new(cron::UpdateCronJobTool::new(cron_runtime.clone())))?
            .register_tool(Arc::new(cron::ManageCronJobTool::new(cron_runtime.clone())))?
            .register_tool(Arc::new(cron::ListCronRunsTool::new(cron_runtime)))?
            .register_tool(chrome_tabs_tool)?
            .register_tool(chrome_page_tool)?
            .register_tool(chrome_input_tool)?
            .register_tool(chrome_script_tool)?
            .register_tool(skills_tool.clone())?
            .register_tool(resource_store.clone())?
            .register_tool(conversations_tool.clone())?
            .register_tool(bot.clone())?;

        if let Some(manager) = tts_manager {
            engine_builder = engine_builder.register_tool(manager)?;
        }
        if let Some(manager) = transcription_manager {
            engine_builder = engine_builder.register_tool(manager)?;
        }

        let engine = engine_builder
            .register_agent(
                image_understanding_agent.clone(),
                Some(image_understanding_agent.model_label().to_string()),
            )?
            .register_agent(
                audio_understanding_agent.clone(),
                Some(audio_understanding_agent.model_label().to_string()),
            )?
            .register_agent(
                video_understanding_agent.clone(),
                Some(video_understanding_agent.model_label().to_string()),
            )?
            .register_agent(
                other_understanding_agent.clone(),
                Some(other_understanding_agent.model_label().to_string()),
            )?
            .register_agent(bot.clone(), Some(ACTIVE_MODEL_LABEL.to_string()))?
            .export_tools(vec![
                ConversationsTool::NAME.to_string(),
                ResourceStore::NAME.to_string(),
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
        Ok(Self {
            state,
            browser_bridge,
            voice_capabilities,
            auto_updater: cfg.auto_updater,
        })
    }

    pub fn into_router(self, cancel_token: CancellationToken) -> Router<()> {
        let auto_update_route_state = AutoUpdateRouteState {
            app: self.state.clone(),
            auto_updater: self.auto_updater.clone(),
        };
        let daemon_control_route_state = DaemonControlRouteState {
            app: self.state.clone(),
            cancel_token,
        };
        let browser_ws_state = BrowserWebSocketState {
            app: self.state.clone(),
            bridge: self.browser_bridge,
            voice_capabilities: self.voice_capabilities,
            auto_updater: self.auto_updater,
        };
        let browser_ws_router = Router::new()
            .route("/ws/engine/{*id}", routing::get(browser_websocket))
            .with_state(browser_ws_state);
        let auto_update_router = Router::new()
            .route("/auto_update", routing::get(auto_update_status))
            .route("/auto_update/check", routing::post(auto_update_check))
            .route(
                "/auto_update/install_and_restart",
                routing::post(auto_update_install_and_restart),
            )
            .with_state(auto_update_route_state);
        let daemon_control_router = Router::new()
            .route("/daemon/shutdown", routing::post(daemon_shutdown))
            .with_state(daemon_control_route_state);

        let app: Router<()> = Router::new()
            .route("/", routing::get(get_version))
            .route("/engine/{*id}", routing::post(anda_engine))
            .with_state(self.state)
            .merge(browser_ws_router)
            .merge(auto_update_router)
            .merge(daemon_control_router);
        app
    }
}

async fn auto_update_status(
    State(state): State<AutoUpdateRouteState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = verify_update_request(&state.app, &headers) {
        return *response;
    }
    AxumJson(state.auto_updater.state()).into_response()
}

async fn auto_update_check(
    State(state): State<AutoUpdateRouteState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = verify_update_request(&state.app, &headers) {
        return *response;
    }
    AxumJson(state.auto_updater.check_if_due().await).into_response()
}

async fn auto_update_install_and_restart(
    State(state): State<AutoUpdateRouteState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = verify_update_request(&state.app, &headers) {
        return *response;
    }
    match state.auto_updater.install_and_restart().await {
        Ok(state) => AxumJson(state).into_response(),
        Err(err) => (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    }
}

async fn daemon_shutdown(
    State(state): State<DaemonControlRouteState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = verify_authenticated_request(&state.app, &headers) {
        return *response;
    }

    state.cancel_token.cancel();
    AxumJson(json!({ "status": "shutting_down" })).into_response()
}

fn verify_update_request(
    app: &AppState,
    headers: &HeaderMap,
) -> Result<(), Box<axum::response::Response>> {
    verify_authenticated_request(app, headers)
}

fn verify_authenticated_request(
    app: &AppState,
    headers: &HeaderMap,
) -> Result<(), Box<axum::response::Response>> {
    let caller = app.verify_user(headers, unix_ms(), None, None);
    if caller == Principal::anonymous() {
        return Err(Box::new(
            (StatusCode::UNAUTHORIZED, "invalid or missing bearer token").into_response(),
        ));
    }
    Ok(())
}

pub async fn get_version() -> impl IntoResponse {
    let info = json!({
        "name": config::APP_NAME,
        "version": config::APP_VERSION,

    });
    axum::Json(info)
}
