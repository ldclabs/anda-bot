use anda_core::{AgentOutput, BoxError, Principal, Tool};
use anda_db::database::AndaDB;
use anda_engine::{
    context::{AgentCtx, Web3SDK},
    engine::{Engine, EngineRef},
    extension::{fs, mcp, note, shell, skill, todo},
    management::{BaseManagement, Visibility},
    memory::Conversations,
    model::{Model, Models, reqwest},
    store::Store,
    subagent::SubAgentManager,
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
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha3::{Digest, Sha3_384};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

mod action;
mod agent;
mod bookmark;
mod browser;
mod browser_ws;
mod conversation;
mod goal;
mod idle;
mod mcp_server;
mod multimodal;
mod prompt;
mod resources;
mod shell_runtime;
mod side;
mod skill_library;
mod system;

use crate::util::{
    http_client::{NO_PROXY, build_http_client},
    key::{Claims, Ed25519Key, Ed25519PubKey, iana},
};
use crate::{
    auto_update::AutoUpdater, brain, channel, config, cron, transcription::TranscriptionManager,
    tts::TtsManager,
};
use browser_ws::{BrowserVoiceCapabilities, BrowserWebSocketState, browser_websocket};

pub(crate) use action::{
    ActionEvent, ActionRuntime, ActionSession, ActionsTool, AskUserChoiceTool,
    action_id_from_message, action_id_from_message_value, apply_action_resolution_to_chat_message,
    apply_action_resolution_to_message, is_action_message, is_action_message_value,
};
pub use agent::{
    AndaBot, AndaBotStatus, AndaBotToolArgs, SessionRequestMeta, SessionState, SessionSummary,
};
pub use bookmark::{BookmarkStore, BookmarksTool};
pub use browser::*;
pub use conversation::*;
pub use goal::GoalTool;
pub use idle::{BrainSleepIdleHook, IdleHook};
pub(crate) use mcp_server::McpServerTool;
pub use multimodal::MediaUnderstandingAgent;
pub(crate) use prompt::PromptCommand;
pub use resources::ResourceStore;
pub use skill_library::SkillLibrary;
pub(crate) use system::{external_user_prompt_with_space, system_runtime_prompt};

// Empty model labels resolve through Models::get_model(), which tracks the active model.
const ACTIVE_MODEL_LABEL: &str = "";

pub struct Engines {
    state: AppState,
    bot: Arc<AndaBot>,
    brain: brain::Client,
    browser_bridge: Arc<BrowserBridge>,
    voice_capabilities: BrowserVoiceCapabilities,
    auto_updater: Arc<AutoUpdater>,
    runtime_models: RuntimeModels,
    config_write_lock: Arc<Mutex<()>>,
    home_dir: PathBuf,
}

#[async_trait]
pub trait CompletionHook: Send + Sync {
    async fn on_completion(&self, _ctx: &AgentCtx, _output: &AgentOutput) {}
}

pub struct EngineConfig {
    pub id_key: Ed25519Key,
    pub managers: Vec<Ed25519PubKey>,
    pub models: Arc<Models>,
    pub brain_models: Arc<Models>,
    pub brain_base_url: String,
    pub home_dir: PathBuf,
    pub skills_dir: PathBuf,
    pub workspaces: Vec<PathBuf>,
    pub tts: config::TtsConfig,
    pub transcription: config::TranscriptionConfig,
    pub mcp: config::McpSettings,
    pub https_proxy: Option<String>,
    pub auto_updater: Arc<AutoUpdater>,
}

#[derive(Clone)]
struct AutoUpdateRouteState {
    app: AppState,
    auto_updater: Arc<AutoUpdater>,
}

#[derive(Clone)]
pub(crate) struct RuntimeModels {
    models: Arc<Models>,
    brain_models: Arc<Models>,
    config_path: PathBuf,
    http_client: reqwest::Client,
    view: Arc<RwLock<DaemonModelsResponse>>,
    reload_lock: Arc<Mutex<()>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonModelsResponse {
    active_model: Option<String>,
    model_names: Vec<String>,
}

#[derive(Clone)]
struct DaemonControlRouteState {
    app: AppState,
    bot: Arc<AndaBot>,
    cancel_token: CancellationToken,
    runtime_models: RuntimeModels,
    // Serializes config updates so concurrent PUTs cannot interleave
    // the backup check, backup copy, and file write.
    config_write_lock: Arc<Mutex<()>>,
}

#[derive(Serialize)]
struct DaemonConfigResponse {
    path: String,
    content: String,
    config: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    models: Option<DaemonModelsResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    models_error: Option<String>,
}

#[derive(Deserialize)]
struct DaemonConfigUpdateRequest {
    content: String,
}

impl RuntimeModels {
    pub(crate) fn new(
        models: Arc<Models>,
        brain_models: Arc<Models>,
        config_path: PathBuf,
        http_client: reqwest::Client,
    ) -> Self {
        let view = daemon_models_response(models.as_ref());
        Self {
            models,
            brain_models,
            config_path,
            http_client,
            view: Arc::new(RwLock::new(view)),
            reload_lock: Arc::new(Mutex::new(())),
        }
    }

    pub(crate) async fn current(&self) -> DaemonModelsResponse {
        self.view.read().await.clone()
    }

    pub(crate) async fn set_active_model(&self, active_model: String) -> DaemonModelsResponse {
        let mut view = self.view.write().await;
        if !view.model_names.iter().any(|name| name == &active_model) {
            view.model_names.push(active_model.clone());
            view.model_names.sort();
        }
        view.active_model = Some(active_model);
        view.clone()
    }

    pub(crate) async fn reload_from_config(&self) -> Result<DaemonModelsResponse, BoxError> {
        let _reload_guard = self.reload_lock.lock().await;
        let config = config::Config::from_file(&self.config_path).await?;
        let model_issues = model_setup_issues(&config);
        if !model_issues.is_empty() {
            return Err(format!(
                "model configuration is incomplete: {}",
                model_issues.join(", ")
            )
            .into());
        }

        let next_models = config.models(self.http_client.clone());
        if next_models.get_model().is_none() {
            return Err("No model found in config.yaml".into());
        }
        let brain_model = brain_model_from_models(&next_models)
            .ok_or("No model found for brain in config.yaml")?;

        self.models.as_ref().replace(&next_models);
        self.brain_models.as_ref().replace(&next_models);
        self.brain_models.set_model(brain_model);
        let response = daemon_models_response(&next_models);
        *self.view.write().await = response.clone();
        Ok(response)
    }
}

fn model_setup_issues(config: &config::Config) -> Vec<String> {
    config
        .setup_issues()
        .into_iter()
        .filter(|issue| issue.starts_with("model."))
        .collect()
}

pub(crate) fn brain_model_from_models(models: &Models) -> Option<Model> {
    models
        .get("brain")
        .or_else(|| models.get("memory"))
        .or_else(|| models.get_model())
}

pub(crate) fn daemon_models_response(models: &Models) -> DaemonModelsResponse {
    DaemonModelsResponse {
        active_model: models.get_model().map(|model| model.model_name()),
        model_names: models.model_names().into_iter().collect(),
    }
}

impl Engines {
    pub async fn new(
        cfg: EngineConfig,
        db: Arc<AndaDB>,
        engine_ref: Arc<EngineRef>,
        cron_runtime: Arc<cron::CronRuntime>,
        completion_hooks: Vec<Arc<dyn CompletionHook>>,
        channel_sender: channel::ChannelSender,
    ) -> Result<Self, BoxError> {
        let active_im_channels = channel_sender.channels();
        let config_path = config::Config::file_path(&cfg.home_dir);
        let mcp_config_path = config::McpSettings::file_path(&cfg.home_dir);
        let config_write_lock = Arc::new(Mutex::new(()));
        let root_secret: [u8; 48] = {
            let mut hasher = Sha3_384::new();
            hasher.update(cfg.id_key.as_bytes());
            hasher.finalize().into()
        };
        let outer_http_client = build_http_client(cfg.https_proxy.clone(), |client| client)?;
        let runtime_models = RuntimeModels::new(
            cfg.models.clone(),
            cfg.brain_models.clone(),
            config_path.clone(),
            outer_http_client.clone(),
        );

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

        let mut claims = Claims {
            audience: Some("*".to_string()),
            ..Default::default()
        };
        claims.extra.insert(iana::CWTClaimScope, "*");
        let brain_token = cfg.id_key.sign_cwt(claims)?;
        let brain_http_client = build_http_client(None, |client| client.no_proxy())?;
        let brain_client = brain::Client::new(cfg.brain_base_url, Some(brain_token))
            .with_http_client(brain_http_client);

        let default_workspace = cfg
            .workspaces
            .first()
            .cloned()
            .ok_or("At least one workspace must be provided")?;
        let anda_conversations = Conversations::connect(db.clone(), "bot".to_string()).await?;
        let subagent_conversations =
            Conversations::connect(db.clone(), "subagent".to_string()).await?;
        let resource_store = Arc::new(ResourceStore::connect(db.clone()).await?);
        let conversations_tool = Arc::new(ConversationsTool::new(
            anda_conversations,
            default_workspace.to_string_lossy().to_string(),
        ));
        let bookmarks_tool = Arc::new(BookmarksTool::with_models(
            BookmarkStore::connect(db.clone()).await?,
            cfg.models.clone(),
        ));
        let browser_bridge = Arc::new(BrowserBridge::new());
        let browser_tabs_tool = Arc::new(
            ChromeBrowserTool::tabs(browser_bridge.clone())
                .with_screenshot_workspace(default_workspace.clone()),
        );
        let browser_page_tool = Arc::new(
            ChromeBrowserTool::page(browser_bridge.clone())
                .with_screenshot_workspace(default_workspace.clone()),
        );
        let browser_input_tool = Arc::new(
            ChromeBrowserTool::input(browser_bridge.clone())
                .with_screenshot_workspace(default_workspace.clone()),
        );
        let browser_script_tool = Arc::new(
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
            let runtime = Arc::new(
                shell_runtime::NativeShellRuntime::new(default_workspace.clone()).insecure(),
            );
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
        let shared_skills_dirs = std::env::home_dir()
            .map(|home_dir| vec![home_dir.join(".agents").join("skills")])
            .unwrap_or_default();
        let bundled_skills_dir = cfg.home_dir.join("bundled-skills");
        let mut additional_skills_dirs = vec![bundled_skills_dir.clone()];
        additional_skills_dirs.extend(shared_skills_dirs.clone());
        let default_skill_tools = vec![
            "shell".to_string(),
            "read_file".to_string(),
            "search_file".to_string(),
            "note".to_string(),
            "tools_groups".to_string(),
            "tools_select".to_string(),
            AskUserChoiceTool::NAME.to_string(),
        ];
        let skills_tool = Arc::new(
            skill::SkillManager::new_with_dirs(cfg.skills_dir.clone(), additional_skills_dirs)
                .with_default_skill_tools(default_skill_tools.clone()),
        );
        let mut known_skill_tools = BTreeSet::from_iter(default_skill_tools.iter().cloned());
        known_skill_tools.extend(
            [
                brain::Client::NAME,
                note::NoteTool::NAME,
                GoalTool::NAME,
                todo::TodoTool::NAME,
                fs::ReadFileTool::NAME,
                fs::SearchFileTool::NAME,
                fs::EditFileTool::NAME,
                fs::WriteFileTool::NAME,
                cron::CreateCronTool::NAME,
                cron::ListCronJobsTool::NAME,
                cron::UpdateCronJobTool::NAME,
                cron::ManageCronJobTool::NAME,
                cron::ListCronRunsTool::NAME,
                ChromeBrowserTool::TABS_NAME,
                ChromeBrowserTool::PAGE_NAME,
                ChromeBrowserTool::INPUT_NAME,
                ChromeBrowserTool::SCRIPT_NAME,
                skill::SkillManager::NAME,
                SkillLibrary::NAME,
                McpServerTool::NAME,
                ResourceStore::NAME,
                ConversationsTool::NAME,
                AskUserChoiceTool::NAME,
                BookmarksTool::NAME,
                SubAgentManager::NAME,
                AndaBot::NAME,
                TtsManager::NAME,
                TranscriptionManager::NAME,
            ]
            .into_iter()
            .map(str::to_string),
        );
        let tools_usage_conversations = conversations_tool.clone();
        let skill_library = Arc::new(
            SkillLibrary::new(
                cfg.home_dir.clone(),
                cfg.skills_dir.clone(),
                bundled_skills_dir,
                shared_skills_dirs,
                skills_tool.clone(),
                default_skill_tools,
                known_skill_tools,
            )
            .with_tools_usage_reader(move || tools_usage_conversations.tools_usage()),
        );
        // Put the brain to sleep (full maintenance) once the bot has been
        // fully idle and the last sleep is more than 12 hours old.
        let idle_hooks: Vec<Arc<dyn IdleHook>> =
            vec![Arc::new(BrainSleepIdleHook::new(brain_client.clone()))];
        let bot = Arc::new(AndaBot::new(
            brain_client.clone(),
            cfg.models.clone(),
            cfg.home_dir.clone(),
            conversations_tool.clone(),
            resource_store.clone(),
            completion_hooks,
            idle_hooks,
            skill_library.clone(),
            browser_tabs_tool.clone(),
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
        let mcp_provider = {
            let servers = cfg
                .mcp
                .server_configs(&cfg.home_dir, Some(default_workspace.as_path()))?;
            Arc::new(mcp::McpToolProvider::new(servers)?)
        };
        let add_mcp_server_tool = Arc::new(McpServerTool::new(
            mcp_provider.clone(),
            cfg.home_dir.clone(),
            Some(default_workspace.clone()),
            mcp_config_path,
            config_write_lock.clone(),
        ));
        let mut engine_builder = Engine::builder()
            .with_web3_client(web3)
            .with_store(Store::new(object_store))
            .with_management(management)
            .with_models(cfg.models.clone())
            .with_subagent_conversations(subagent_conversations)
            .register_tool(Arc::new(brain_client.clone()))?
            .register_tool(Arc::new(shell_tool))?
            .register_tool(Arc::new(ActionsTool::new(bot.action_runtime())))?
            .register_tool(Arc::new(AskUserChoiceTool))?
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
            .register_tool(browser_tabs_tool)?
            .register_tool(browser_page_tool)?
            .register_tool(browser_input_tool)?
            .register_tool(browser_script_tool)?
            .register_tool(skills_tool.clone())?
            .register_tool(skill_library.clone())?
            .register_tool(add_mcp_server_tool)?
            .register_tool(resource_store.clone())?
            .register_tool(conversations_tool.clone())?
            .register_tool(bookmarks_tool.clone())?
            .register_tool(bot.clone())?;

        if let Some(manager) = tts_manager {
            engine_builder = engine_builder.register_tool(manager)?;
        }
        if let Some(manager) = transcription_manager {
            engine_builder = engine_builder.register_tool(manager)?;
        }
        if !channel_sender.is_empty() {
            engine_builder = engine_builder
                .register_tool(Arc::new(channel::SendImMessageTool::new(
                    channel_sender.clone(),
                )))?
                .register_tool(Arc::new(channel::ListImChannelsTool::new(channel_sender)))?;
        }
        engine_builder = engine_builder.register_tool_provider(mcp_provider)?;

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
                ActionsTool::NAME.to_string(),
                ResourceStore::NAME.to_string(),
                BookmarksTool::NAME.to_string(),
                SkillLibrary::NAME.to_string(),
                Tool::name(bot.as_ref()),
            ]);

        // Initialize and start the server
        let engine = engine.build(AndaBot::NAME.to_string()).await?;
        let engine = Arc::new(engine);
        engine_ref.bind(Arc::downgrade(&engine));
        // A failure scanning the skills directories (e.g. permissions on the
        // shared ~/.agents/skills) should not prevent the daemon from starting.
        if let Err(err) = skill_library.reload().await {
            log::error!("failed to load skills, continuing without them: {err}");
        }
        engine.sub_agents_manager().insert(skill_library);

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
            bot,
            brain: brain_client,
            browser_bridge,
            voice_capabilities,
            auto_updater: cfg.auto_updater,
            runtime_models,
            config_write_lock,
            home_dir: cfg.home_dir,
        })
    }

    pub fn into_router(self, cancel_token: CancellationToken) -> Router<()> {
        let auto_update_route_state = AutoUpdateRouteState {
            app: self.state.clone(),
            auto_updater: self.auto_updater.clone(),
        };
        let daemon_control_route_state = DaemonControlRouteState {
            app: self.state.clone(),
            bot: self.bot.clone(),
            cancel_token,
            runtime_models: self.runtime_models.clone(),
            config_write_lock: self.config_write_lock.clone(),
        };
        let browser_ws_state = BrowserWebSocketState {
            app: self.state.clone(),
            brain: self.brain,
            bridge: self.browser_bridge,
            voice_capabilities: self.voice_capabilities,
            auto_updater: self.auto_updater,
            home_dir: self.home_dir,
            runtime_models: self.runtime_models.clone(),
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
            .route("/daemon/status", routing::get(get_status))
            .route(
                "/daemon/config",
                routing::get(get_daemon_config).put(update_daemon_config),
            )
            .route("/daemon/models/reload", routing::post(reload_daemon_models))
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
    if let Err(response) = verify_authenticated_request(&state.app, &headers) {
        return *response;
    }
    AxumJson(state.auto_updater.state()).into_response()
}

async fn auto_update_check(
    State(state): State<AutoUpdateRouteState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = verify_authenticated_request(&state.app, &headers) {
        return *response;
    }
    AxumJson(state.auto_updater.check_if_due().await).into_response()
}

async fn auto_update_install_and_restart(
    State(state): State<AutoUpdateRouteState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = verify_authenticated_request(&state.app, &headers) {
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

async fn get_status(State(state): State<DaemonControlRouteState>) -> impl IntoResponse {
    match state.bot.status().await {
        Ok(status) => AxumJson(status).into_response(),
        Err(err) => {
            log::warn!("failed to get daemon status: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get status: {err}"),
            )
                .into_response()
        }
    }
}

async fn get_daemon_config(
    State(state): State<DaemonControlRouteState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = verify_authenticated_request(&state.app, &headers) {
        return *response;
    }

    let content = match crate::util::text::read_text_file(&state.runtime_models.config_path).await {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            config::Config::default_template().to_string()
        }
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    };

    match daemon_config_response(&state.runtime_models.config_path, content) {
        Ok(mut response) => {
            response.models = Some(state.runtime_models.current().await);
            AxumJson(response).into_response()
        }
        Err(err) => (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    }
}

async fn update_daemon_config(
    State(state): State<DaemonControlRouteState>,
    headers: HeaderMap,
    AxumJson(request): AxumJson<DaemonConfigUpdateRequest>,
) -> impl IntoResponse {
    if let Err(response) = verify_authenticated_request(&state.app, &headers) {
        return *response;
    }

    let content = normalize_config_file_content(request.content);
    let response = match daemon_config_response(&state.runtime_models.config_path, content.clone())
    {
        Ok(response) => response,
        Err(err) => return (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    };

    if let Some(parent) = state.runtime_models.config_path.parent()
        && let Err(err) = tokio::fs::create_dir_all(parent).await
    {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    let _write_guard = state.config_write_lock.lock().await;

    match daemon_config_needs_backup(&state.runtime_models.config_path, content.as_bytes()).await {
        Ok(true) => {
            if let Err(err) = backup_daemon_config(&state.runtime_models.config_path).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
            }
        }
        Ok(false) => {}
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }

    if let Err(err) =
        write_daemon_config_atomically(&state.runtime_models.config_path, content.as_bytes()).await
    {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    let mut response = response;
    match state.runtime_models.reload_from_config().await {
        Ok(models) => response.models = Some(models),
        Err(err) => {
            log::warn!("failed to reload daemon models after config update: {err}");
            response.models = Some(state.runtime_models.current().await);
            response.models_error = Some(err.to_string());
        }
    }

    AxumJson(response).into_response()
}

async fn reload_daemon_models(
    State(state): State<DaemonControlRouteState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = verify_authenticated_request(&state.app, &headers) {
        return *response;
    }

    match state.runtime_models.reload_from_config().await {
        Ok(models) => AxumJson(models).into_response(),
        Err(err) => (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    }
}

// Write via a temp file in the same directory plus fsync and rename, so a
// crash mid-write cannot leave a truncated config that blocks the next
// daemon start.
async fn write_daemon_config_atomically(path: &Path, content: &[u8]) -> Result<(), BoxError> {
    use tokio::io::AsyncWriteExt;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(config::CONFIG_FILE_NAME);
    let temp_path = path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));

    let result = async {
        let mut file = tokio::fs::File::create(&temp_path).await?;
        file.write_all(content).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&temp_path, path).await
    }
    .await;

    if result.is_err() {
        let _ = tokio::fs::remove_file(&temp_path).await;
    }
    Ok(result?)
}

fn daemon_config_response(
    path: &std::path::Path,
    content: String,
) -> Result<DaemonConfigResponse, BoxError> {
    let config = config::Config::from_contents(&content)?;
    let config = serde_json::to_value(config)?;
    Ok(DaemonConfigResponse {
        path: path.to_string_lossy().to_string(),
        content,
        config,
        models: None,
        models_error: None,
    })
}

fn normalize_config_file_content(mut content: String) -> String {
    content = content.replace("\r\n", "\n").replace('\r', "\n");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content
}

async fn daemon_config_needs_backup(path: &Path, next_content: &[u8]) -> Result<bool, BoxError> {
    match tokio::fs::read(path).await {
        Ok(existing) => Ok(existing != next_content),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

async fn backup_daemon_config(path: &Path) -> Result<PathBuf, BoxError> {
    let backup_path = unique_daemon_config_backup_path(path).await?;
    tokio::fs::copy(path, &backup_path).await?;
    Ok(backup_path)
}

async fn unique_daemon_config_backup_path(path: &Path) -> Result<PathBuf, BoxError> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(config::CONFIG_FILE_NAME);
    let backup_path = path.with_file_name(format!("{file_name}.bak"));
    match tokio::fs::metadata(&backup_path).await {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(backup_path),
        Err(err) => return Err(err.into()),
        Ok(_) => {}
    }

    let stamp = unix_ms();
    for attempt in 1..=1000 {
        let backup_path = path.with_file_name(format!("{file_name}.{stamp}.{attempt}.bak"));
        match tokio::fs::metadata(&backup_path).await {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(backup_path),
            Err(err) => return Err(err.into()),
            Ok(_) => {}
        }
    }

    Err(format!("could not allocate backup path for {}", path.display()).into())
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

#[cfg(test)]
mod tests {
    use super::*;
    use anda_engine::model::ModelConfig;

    fn test_model_config(model: &str, labels: &[&str]) -> ModelConfig {
        ModelConfig {
            family: "openai".to_string(),
            model: model.to_string(),
            api_base: "http://127.0.0.1:1/v1".to_string(),
            api_key: "test-key".to_string(),
            labels: labels.iter().map(|label| label.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn normalize_config_file_content_uses_lf_and_final_newline() {
        assert_eq!(
            normalize_config_file_content("addr: 127.0.0.1:8042\r\nlog_level: warn".to_string()),
            "addr: 127.0.0.1:8042\nlog_level: warn\n"
        );
    }

    #[test]
    fn apply_models_update_switches_active_and_registers_new_labels() {
        let http_client = crate::util::http_client::new_reqwest_client();
        let target = Models::from_configs(
            &[test_model_config("old-model", &["old", "memory"])],
            http_client.clone(),
        );
        let next = Models::from_configs(&[test_model_config("new-model", &["fast"])], http_client);
        let active = next.get("new-model").unwrap();
        next.set_model(active);
        target.replace(&next);

        assert_eq!(target.get_model().unwrap().model_name(), "new-model");
        assert_eq!(target.get("fast").unwrap().model_name(), "new-model");
        assert_eq!(
            serde_json::to_value(daemon_models_response(&next)).unwrap(),
            json!({
                "active_model": "new-model",
                "model_names": ["new-model"]
            })
        );
    }

    #[test]
    fn brain_model_from_models_prefers_brain_or_memory_labels() {
        let http_client = crate::util::http_client::new_reqwest_client();
        let models = Models::from_configs(
            &[
                test_model_config("active-model", &[]),
                test_model_config("memory-model", &["memory"]),
            ],
            http_client,
        );
        let active = models.get("active-model").unwrap();
        models.set_model(active);

        assert_eq!(
            brain_model_from_models(&models).unwrap().model_name(),
            "memory-model"
        );
    }

    #[tokio::test]
    async fn daemon_config_atomic_write_replaces_content_without_leftover_temp_files() {
        let home = tempfile::tempdir().unwrap();
        let config_path = home.path().join(config::CONFIG_FILE_NAME);

        write_daemon_config_atomically(&config_path, b"addr: 127.0.0.1:8042\n")
            .await
            .unwrap();
        write_daemon_config_atomically(&config_path, b"addr: 127.0.0.1:9000\n")
            .await
            .unwrap();

        assert_eq!(
            tokio::fs::read(&config_path).await.unwrap(),
            b"addr: 127.0.0.1:9000\n"
        );

        let mut entries = tokio::fs::read_dir(home.path()).await.unwrap();
        let mut names = Vec::new();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
        assert_eq!(names, vec![config::CONFIG_FILE_NAME.to_string()]);
    }

    #[tokio::test]
    async fn daemon_config_backup_copies_existing_file_when_content_changes() {
        let home = tempfile::tempdir().unwrap();
        let config_path = home.path().join(config::CONFIG_FILE_NAME);
        let existing = b"addr: 127.0.0.1:8042\n";
        tokio::fs::write(&config_path, existing).await.unwrap();

        assert!(
            !daemon_config_needs_backup(&config_path, existing)
                .await
                .unwrap()
        );
        assert!(
            daemon_config_needs_backup(&config_path, b"addr: 127.0.0.1:9000\n")
                .await
                .unwrap()
        );

        let backup_path = backup_daemon_config(&config_path).await.unwrap();
        assert_eq!(tokio::fs::read(&backup_path).await.unwrap(), existing);

        let second_backup_path = backup_daemon_config(&config_path).await.unwrap();
        assert_ne!(backup_path, second_backup_path);
        assert_eq!(
            tokio::fs::read(&second_backup_path).await.unwrap(),
            existing
        );
    }

    const VALID_CONFIG_YAML: &str = r#"
model:
  active: gpt-test
  providers:
    - family: openai
      model: gpt-test
      api_base: http://127.0.0.1:1/v1
      api_key: test-key
      labels: ["memory"]
"#;

    fn runtime_models_at(config_path: PathBuf) -> RuntimeModels {
        let http_client = crate::util::http_client::new_reqwest_client();
        let models = Arc::new(Models::from_configs(
            &[test_model_config("gpt-test", &["memory"])],
            http_client.clone(),
        ));
        let active = models.get("gpt-test").unwrap();
        models.set_model(active);
        let brain_models = Arc::new(Models::from_configs(
            &[test_model_config("gpt-test", &["memory"])],
            http_client.clone(),
        ));
        RuntimeModels::new(models, brain_models, config_path, http_client)
    }

    #[tokio::test]
    async fn runtime_models_tracks_active_model() {
        let home = tempfile::tempdir().unwrap();
        let runtime = runtime_models_at(home.path().join(config::CONFIG_FILE_NAME));

        let current = runtime.current().await;
        assert_eq!(current.active_model.as_deref(), Some("gpt-test"));

        let updated = runtime.set_active_model("brand-new".to_string()).await;
        assert_eq!(updated.active_model.as_deref(), Some("brand-new"));
        assert!(updated.model_names.iter().any(|name| name == "brand-new"));
    }

    #[tokio::test]
    async fn runtime_models_reload_from_config_success_and_failure() {
        let home = tempfile::tempdir().unwrap();
        let config_path = home.path().join(config::CONFIG_FILE_NAME);
        tokio::fs::write(&config_path, VALID_CONFIG_YAML)
            .await
            .unwrap();

        let runtime = runtime_models_at(config_path.clone());
        let reloaded = runtime.reload_from_config().await.unwrap();
        assert_eq!(reloaded.active_model.as_deref(), Some("gpt-test"));

        // An empty model section yields setup issues, surfacing an error.
        tokio::fs::write(&config_path, "addr: 127.0.0.1:8042\n")
            .await
            .unwrap();
        assert!(runtime.reload_from_config().await.is_err());
    }

    #[test]
    fn model_setup_issues_filters_model_prefixed_issues() {
        let config = config::Config::from_contents("addr: 127.0.0.1:8042\n").unwrap();
        let issues = model_setup_issues(&config);
        assert!(issues.iter().all(|issue| issue.starts_with("model.")));
        assert!(!issues.is_empty());

        let ok = config::Config::from_contents(VALID_CONFIG_YAML).unwrap();
        assert!(model_setup_issues(&ok).is_empty());
    }

    #[test]
    fn daemon_config_response_parses_valid_and_rejects_invalid() {
        let response = daemon_config_response(
            std::path::Path::new("/tmp/config.yaml"),
            VALID_CONFIG_YAML.to_string(),
        )
        .unwrap();
        assert_eq!(response.path, "/tmp/config.yaml");
        assert!(response.config.is_object());

        assert!(
            daemon_config_response(std::path::Path::new("/tmp/x"), "\tnot: [valid".to_string())
                .is_err()
        );
    }

    #[tokio::test]
    async fn get_version_reports_app_metadata() {
        use axum::response::IntoResponse;
        let response = get_version().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    use crate::auto_update::AutoUpdater;
    use crate::util::key::{Claims, Ed25519Key, iana};
    use anda_db::storage::StorageConfig;
    use axum::extract::State;
    use ed25519_dalek::VerifyingKey;

    async fn route_test_db() -> Arc<AndaDB> {
        let object_store: Arc<dyn object_store::ObjectStore> =
            Arc::new(object_store::memory::InMemory::new());
        Arc::new(
            AndaDB::connect(
                object_store,
                anda_db::database::DBConfig {
                    name: "route_test".to_string(),
                    description: "route test".to_string(),
                    storage: StorageConfig {
                        cache_max_capacity: 1024,
                        compress_level: 1,
                        object_chunk_size: 256 * 1024,
                        bucket_overload_size: 256 * 1024,
                        max_small_object_size: 1024 * 1024,
                    },
                    lock: None,
                },
            )
            .await
            .unwrap(),
        )
    }

    fn minimal_app(pubkeys: Vec<VerifyingKey>) -> AppState {
        AppState {
            engines: Arc::new(BTreeMap::new()),
            default_engine: Principal::management_canister(),
            start_time_ms: 0,
            extra_info: Arc::new(BTreeMap::new()),
            ed25519_pubkeys: Arc::new(pubkeys),
        }
    }

    fn authed_headers(key: &Ed25519Key) -> HeaderMap {
        let mut claims = Claims::default();
        claims.extra.insert(iana::CWTClaimScope, "*");
        let token = key.sign_cwt(claims).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        headers
    }

    fn dead_proxy_http() -> reqwest::Client {
        reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:1").unwrap())
            .build()
            .unwrap()
    }

    #[test]
    fn verify_authenticated_request_rejects_anonymous() {
        let app = minimal_app(vec![]);
        let err = verify_authenticated_request(&app, &HeaderMap::new());
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn auto_update_routes_require_auth_and_return_state() {
        let db = route_test_db().await;
        let key = Ed25519Key::new([5u8; 32]);
        let app = minimal_app(vec![key.pubkey().into()]);
        let auto_updater = Arc::new(AutoUpdater::new(
            db,
            std::env::temp_dir(),
            dead_proxy_http(),
        ));
        let state = AutoUpdateRouteState { app, auto_updater };

        // Without a token, the status route is unauthorized.
        let resp = auto_update_status(State(state.clone()), HeaderMap::new())
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // With a valid bearer token, the status route returns the persisted state.
        let resp = auto_update_status(State(state.clone()), authed_headers(&key))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::OK);

        // The check route runs a (failing, dead-proxy) check and still responds 200.
        let resp = auto_update_check(State(state.clone()), authed_headers(&key))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::OK);

        // Install with no downloaded update is a bad request.
        let resp = auto_update_install_and_restart(State(state), authed_headers(&key))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    async fn build_route_bot(db: Arc<AndaDB>, home: PathBuf) -> Arc<AndaBot> {
        let http = dead_proxy_http();
        let brain_client = crate::brain::Client::new(
            "http://127.0.0.1:1/v1/anda_bot".to_string(),
            Some("t".to_string()),
        )
        .with_http_client(http);
        let conversations = Conversations::connect(db.clone(), "bot".to_string())
            .await
            .unwrap();
        let conversations_tool = Arc::new(ConversationsTool::new(
            conversations,
            home.to_string_lossy().to_string(),
        ));
        let resource_store = Arc::new(ResourceStore::connect(db.clone()).await.unwrap());
        let skills = SkillLibrary::for_test(home.clone());
        let bridge = Arc::new(BrowserBridge::new());
        Arc::new(AndaBot::new(
            brain_client,
            Arc::new(anda_engine::model::Models::default()),
            home,
            conversations_tool,
            resource_store,
            vec![],
            vec![],
            skills,
            Arc::new(ChromeBrowserTool::tabs(bridge)),
            None,
            None,
            vec![],
        ))
    }

    #[tokio::test]
    async fn daemon_control_routes_serve_config_and_status() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join(config::CONFIG_FILE_NAME);
        tokio::fs::write(&config_path, VALID_CONFIG_YAML)
            .await
            .unwrap();
        let db = route_test_db().await;
        let key = Ed25519Key::new([6u8; 32]);
        let app = minimal_app(vec![key.pubkey().into()]);
        let bot = build_route_bot(db, dir.path().to_path_buf()).await;
        let runtime_models = runtime_models_at(config_path.clone());
        let state = DaemonControlRouteState {
            app,
            bot,
            cancel_token: CancellationToken::new(),
            runtime_models,
            config_write_lock: Arc::new(Mutex::new(())),
        };

        // get_status hits the (dead-proxy) brain and surfaces an error.
        let resp = get_status(State(state.clone())).await.into_response();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

        // Reading the config requires auth and returns the parsed config.
        let resp = get_daemon_config(State(state.clone()), HeaderMap::new())
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let resp = get_daemon_config(State(state.clone()), authed_headers(&key))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::OK);

        // Updating the config writes it and reloads the models.
        let resp = update_daemon_config(
            State(state.clone()),
            authed_headers(&key),
            AxumJson(DaemonConfigUpdateRequest {
                content: VALID_CONFIG_YAML.to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);

        // Reloading models from the on-disk config succeeds.
        let resp = reload_daemon_models(State(state.clone()), authed_headers(&key))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::OK);

        // Shutdown cancels the token.
        assert!(!state.cancel_token.is_cancelled());
        let resp = daemon_shutdown(State(state.clone()), authed_headers(&key))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(state.cancel_token.is_cancelled());
    }
}
