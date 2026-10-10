//! [`McpGate`]: the agent's one way to the MCP servers' tools.
//!
//! It wraps the engine's provider, so every call passes it, whether the model
//! or a subagent made it; the engine's tool hooks see neither. Before a call
//! it decides, from the server's approval policy, the session's approval mode
//! and whether the tool's definition is the reviewed one, to run it, to ask
//! the user, or to refuse it with a reason the model can act on.
//!
//! It also answers to `mcp__<server>__<tool>`, the name other agents give an
//! MCP tool, so a skill written for them reaches the same tool.

use anda_core::{
    BoxError, BoxFut, FunctionDefinition, Json, Resource, ToolGroup, ToolInput, ToolOutput,
    ToolProvider, validate_function_name,
};
use anda_engine::{
    context::BaseCtx,
    extension::mcp::{McpToolProvider, McpToolRoute},
};
use serde_json::json;
use std::sync::Arc;

use super::{
    McpChange, McpElicitations, McpManager, redact::redact_json, review::McpReview,
    state::McpSource,
};
use crate::{
    config::McpApproval,
    engine::{
        ApprovalMode, McpApprovalCard, approval_detail, approval_scope, request_mcp_tool_approval,
    },
};

const ALIAS_PREFIX: &str = "mcp__";
/// How much of the server's description and of the call's arguments an
/// approval card shows.
const CARD_DESCRIPTION_CHARS: usize = 600;
const CARD_ARGUMENTS_BYTES: usize = 4096;

pub(crate) struct McpGate {
    provider: Arc<McpToolProvider>,
    manager: McpManager,
    elicitations: Arc<McpElicitations>,
}

impl McpGate {
    pub fn new(
        provider: Arc<McpToolProvider>,
        manager: McpManager,
        elicitations: Arc<McpElicitations>,
    ) -> Self {
        Self {
            provider,
            manager,
            elicitations,
        }
    }

    /// The tool a lowercase name calls: an Anda name, or an alias.
    fn route(&self, name: &str) -> Option<McpToolRoute> {
        let alias = parse_alias(name);
        self.provider
            .routes()
            .into_iter()
            .find(|route| match alias {
                Some((server, tool)) => {
                    route.server_id.eq_ignore_ascii_case(server)
                        && route.remote_name.eq_ignore_ascii_case(tool)
                }
                None => route.name.eq_ignore_ascii_case(name),
            })
    }

    /// Decides whether `route` may run now, asking the user when it must.
    /// `Some` is the refusal the model gets instead of a result; an `Err` is
    /// a denied or unanswered approval.
    async fn check(
        &self,
        ctx: &BaseCtx,
        route: &McpToolRoute,
        args: &Json,
    ) -> Result<Option<ToolOutput<Json>>, BoxError> {
        let scope = approval_scope(ctx);
        let policy = self
            .manager
            .tool_policy(&route.server_id, &route.remote_name);
        if scope.external_user && !policy.allow_external_users {
            return Ok(Some(refusal(
                route,
                "external_user",
                format!(
                    "MCP server {} is not available to external users, so {} was not called.",
                    route.server_id, route.remote_name
                ),
                format!(
                    "The owner can make it available with `anda mcp external-users {} on`.",
                    route.server_id
                ),
            )));
        }

        let (review, digest) = self.manager.review(route).await;
        let reviewed = review == McpReview::Trusted;
        if !must_ask(policy.approval, scope.mode, reviewed, read_only(route)) {
            return Ok(None);
        }
        if let Some(reason) = scope.unanswerable {
            return Ok(Some(refusal(
                route,
                "approval_required",
                format!(
                    "Calling {} on MCP server {} needs the user's approval, which nobody can give in this {reason}; it was not called.",
                    route.remote_name, route.server_id
                ),
                allow_hint(route),
            )));
        }

        let (title, card) = approval_card(route, policy.approval, review, &digest, args);
        let always = request_mcp_tool_approval(ctx, &route.name, title, card).await?;
        // The card showed the tool as new or changed: approving it accepts
        // the definition, so it is not asked about as new again.
        if !reviewed {
            self.manager.accept_definition(route).await;
        }
        // "Always allow" is the owner's answer on the card, written to the
        // server's entry like any other policy. The call goes ahead either
        // way: it was approved.
        if always {
            let change = McpChange::SetApproval {
                id: route.server_id.clone(),
                tool: Some(route.remote_name.clone()),
                approval: Some(McpApproval::Allow),
            };
            if let Err(err) = self.manager.apply(change, None, McpSource::Manual).await {
                log::warn!(
                    "MCP tool {} of {} was approved, but always allowing it failed: {err}",
                    route.remote_name,
                    route.server_id
                );
            }
        }
        Ok(None)
    }
}

impl ToolProvider<BaseCtx> for McpGate {
    fn name(&self) -> String {
        self.provider.name()
    }

    fn definitions(&self, names: Option<&[String]>) -> Vec<FunctionDefinition> {
        let Some(names) = names else {
            return self.provider.definitions(None);
        };
        let (aliases, names): (Vec<String>, Vec<String>) = names
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .partition(|name| parse_alias(name).is_some());
        let mut definitions = self.provider.definitions(Some(&names));
        if aliases.is_empty() {
            return definitions;
        }
        let routes = self.provider.routes();
        for alias in aliases {
            let Some((server, tool)) = parse_alias(&alias) else {
                continue;
            };
            if let Some(route) = routes.iter().find(|route| {
                route.server_id.eq_ignore_ascii_case(server)
                    && route.remote_name.eq_ignore_ascii_case(tool)
            }) {
                let mut definition = route.definition.clone();
                // Named as asked: a subagent may call only the names its
                // skill lists, and this is the one it lists.
                if validate_function_name(&alias).is_ok() {
                    definition.name = alias;
                }
                definitions.push(definition);
            }
        }
        definitions
    }

    fn groups(&self) -> Vec<ToolGroup> {
        self.provider.groups()
    }

    fn contains_lowercase(&self, name: &str) -> bool {
        self.provider.contains_lowercase(name)
            || (parse_alias(name).is_some() && self.route(name).is_some())
    }

    fn supported_resource_tags(&self, name: &str) -> Vec<String> {
        self.provider.supported_resource_tags(name)
    }

    fn select_resources(&self, name: &str, resources: &mut Vec<Resource>) -> Vec<Resource> {
        self.provider.select_resources(name, resources)
    }

    fn init(&self, ctx: BaseCtx) -> BoxFut<'_, Result<(), BoxError>> {
        self.provider.init(ctx)
    }

    fn refresh(&self) -> BoxFut<'_, Result<(), BoxError>> {
        self.provider.refresh()
    }

    fn call(
        &self,
        ctx: BaseCtx,
        mut input: ToolInput<Json>,
    ) -> BoxFut<'_, Result<ToolOutput<Json>, BoxError>> {
        Box::pin(async move {
            input.name.make_ascii_lowercase();
            let route = self
                .route(&input.name)
                .ok_or_else(|| format!("MCP tool {} not found", input.name))?;
            if let Some(refusal) = self.check(&ctx, &route, &input.args).await? {
                return Ok(refusal);
            }
            input.name = route.name.clone();
            // The server may ask the user for input while the call runs.
            let result = self
                .elicitations
                .during(
                    &route.server_id,
                    &ctx,
                    self.manager.elicitation_timeout(&route.server_id),
                    self.provider.call(ctx.clone(), input),
                )
                .await;
            let failed = result
                .as_ref()
                .map_or(true, |output| output.is_error == Some(true));
            self.manager.record_call(&route.server_id, failed);
            result
        })
    }
}

/// Whether a call must be approved first. The most specific setting wins: a
/// tool set to `ask` asks even with full access, and one set to `allow` does
/// not ask even when every call should, but only while its definition is the
/// reviewed one. `auto` follows the mode, letting a read-only tool whose
/// definition was reviewed run unasked. The server's hints alone grant
/// nothing: a changed definition is no longer trusted to tell the truth.
pub(super) fn must_ask(
    approval: McpApproval,
    mode: ApprovalMode,
    reviewed: bool,
    read_only: bool,
) -> bool {
    match approval {
        McpApproval::Ask => true,
        McpApproval::Allow => !reviewed && mode != ApprovalMode::FullAccess,
        McpApproval::Auto => match mode {
            ApprovalMode::FullAccess => false,
            ApprovalMode::RequestApproval => true,
            ApprovalMode::OnRisk | ApprovalMode::Custom => !(reviewed && read_only),
        },
    }
}

fn read_only(route: &McpToolRoute) -> bool {
    route.tool.annotations.as_ref().is_some_and(|hints| {
        hints.read_only_hint == Some(true) && hints.destructive_hint != Some(true)
    })
}

/// `mcp__<server>__<tool>`, split.
fn parse_alias(name: &str) -> Option<(&str, &str)> {
    let (server, tool) = name.strip_prefix(ALIAS_PREFIX)?.split_once("__")?;
    (!server.is_empty() && !tool.is_empty()).then_some((server, tool))
}

/// A call the gate did not make, as a tool error the model can act on.
fn refusal(route: &McpToolRoute, code: &str, message: String, hint: String) -> ToolOutput<Json> {
    ToolOutput {
        output: json!({
            "error": {
                "code": code,
                "server_id": route.server_id,
                "tool": route.remote_name,
                "message": message,
                "hint": hint,
            }
        }),
        is_error: Some(true),
        ..Default::default()
    }
}

fn allow_hint(route: &McpToolRoute) -> String {
    format!(
        "The owner can let the agent call it without asking: `anda mcp approval {id} allow --tool {tool}`, or `anda mcp approval {id} allow` for every tool of the server.",
        id = route.server_id,
        tool = route.remote_name
    )
}

fn approval_card(
    route: &McpToolRoute,
    approval: McpApproval,
    review: McpReview,
    digest: &str,
    args: &Json,
) -> (String, McpApprovalCard) {
    let tool = &route.tool;
    let hints = tool.annotations.as_ref();
    let tool_label = tool
        .title
        .clone()
        .or_else(|| hints.and_then(|hints| hints.title.clone()))
        .unwrap_or_else(|| route.remote_name.clone());
    let title = format!("Run MCP tool: {} · {tool_label}", route.server_id);

    let mut message = format!(
        "The agent wants to call a tool of the MCP server {}. The tool's name, description and hints come from the server and are not verified.",
        route.server_id
    );
    match review {
        McpReview::Trusted => {}
        McpReview::New => message.push_str(
            " The tool is new since the server was reviewed; approving accepts its definition.",
        ),
        McpReview::Changed => message.push_str(
            " Its definition changed since it was reviewed; approving accepts the new one.",
        ),
    }

    let mut details = vec![
        approval_detail("Server", &route.server_id, "text"),
        approval_detail("Tool", &route.remote_name, "text"),
    ];
    if let Some(description) = tool.description.as_deref().map(str::trim)
        && !description.is_empty()
    {
        let mut shown: String = description.chars().take(CARD_DESCRIPTION_CHARS).collect();
        if shown.len() < description.len() {
            shown.push('…');
        }
        details.push(approval_detail("Description", shown, "text"));
    }
    let hint_labels: Vec<&str> = hints
        .map(|hints| {
            [
                (hints.read_only_hint, "read-only", "writes"),
                (hints.destructive_hint, "destructive", "non-destructive"),
                (hints.idempotent_hint, "idempotent", "not idempotent"),
                (hints.open_world_hint, "open world", "closed world"),
            ]
            .into_iter()
            .filter_map(|(hint, yes, no)| hint.map(|hint| if hint { yes } else { no }))
            .collect()
        })
        .unwrap_or_default();
    if !hint_labels.is_empty() {
        details.push(approval_detail("Server hints", &hint_labels, "list"));
    }
    match review {
        McpReview::Trusted => {}
        McpReview::New => details.push(approval_detail("Review", "new tool", "text")),
        McpReview::Changed => details.push(approval_detail(
            "Review",
            format!(
                "definition changed; see `anda mcp diff {} {}`",
                route.server_id, route.remote_name
            ),
            "text",
        )),
    }
    let mut arguments =
        serde_json::to_string_pretty(&redact_json(args)).unwrap_or_else(|_| "{}".to_string());
    if arguments.len() > CARD_ARGUMENTS_BYTES {
        arguments.truncate(arguments.floor_char_boundary(CARD_ARGUMENTS_BYTES));
        arguments.push_str("\n…");
    }
    details.push(approval_detail("Arguments", arguments, "code"));

    let card = McpApprovalCard {
        message,
        summary: format!("{} · {}", route.server_id, route.remote_name),
        details,
        metadata: json!({
            "server_id": route.server_id,
            "tool": route.remote_name,
            "digest": digest,
            "approval": approval.as_str(),
            "review": review,
        }),
    };
    (title, card)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::McpSettings,
        engine::{
            ActionEvent, ActionResponseArgs, ActionRuntime, ActionSession, SessionRequestMeta,
            action_id_from_message,
            mcp::{
                McpChange,
                state::McpSource,
                test_server::{self, Catalog},
            },
        },
    };
    use anda_core::{RequestMeta, StateFeatures};
    use parking_lot::{Mutex, RwLock};
    use serde_json::Value;

    #[test]
    fn explicit_policies_win_over_the_mode() {
        use ApprovalMode::*;
        use McpApproval::*;
        // Whether to ask, by mode: FullAccess, OnRisk, Custom, RequestApproval.
        let table: [(McpApproval, bool, bool, [bool; 4]); 6] = [
            // An explicit allow runs everywhere while the tool is unchanged.
            (Allow, true, false, [false, false, false, false]),
            // ... and asks once it changed, unless the session has full access.
            (Allow, false, false, [false, true, true, true]),
            // An explicit ask asks even with full access.
            (Ask, true, true, [true, true, true, true]),
            // `auto` runs a reviewed read-only tool unless every call asks.
            (Auto, true, true, [false, false, false, true]),
            // ... and asks for the rest outside full access.
            (Auto, true, false, [false, true, true, true]),
            (Auto, false, true, [false, true, true, true]),
        ];
        for (approval, reviewed, read_only, expected) in table {
            let got = [FullAccess, OnRisk, Custom, RequestApproval]
                .map(|mode| must_ask(approval, mode, reviewed, read_only));
            assert_eq!(
                got, expected,
                "{approval:?} reviewed={reviewed} read_only={read_only}"
            );
        }
    }

    fn write_tool(name: &str) -> Value {
        json!({
            "name": name,
            "description": format!("Changes things with {name}."),
            "inputSchema": { "type": "object" }
        })
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        gate: McpGate,
        manager: McpManager,
        catalog: Catalog,
    }

    /// A gate over one server, `mock`, written to mcp.json as `entry` and
    /// connected without the manager seeing it, as engine startup does.
    async fn fixture(entry: Value, tools: Vec<Value>) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let catalog: Catalog = Arc::new(RwLock::new(tools));
        let mut entry = entry;
        entry["url"] = json!(test_server::serve(catalog.clone()).await);
        tokio::fs::write(
            McpSettings::file_path(dir.path()),
            json!({ "mcpServers": { "mock": entry } }).to_string(),
        )
        .await
        .unwrap();
        let manager = McpManager::for_test(dir.path()).await;
        manager.provider().refresh_server("mock").await.unwrap();
        Fixture {
            _dir: dir,
            gate: McpGate::new(
                manager.provider().clone(),
                manager.clone(),
                Arc::new(McpElicitations::default()),
            ),
            manager,
            catalog,
        }
    }

    impl Fixture {
        async fn call(&self, ctx: BaseCtx, name: &str) -> Result<ToolOutput<Json>, BoxError> {
            self.gate
                .call(ctx, ToolInput::new(name.to_string(), json!({ "q": "x" })))
                .await
        }

        /// Serves the catalog as it is now.
        async fn republish(&self) {
            let receipt = self.manager.reconnect(Some("mock")).await.unwrap();
            assert!(receipt.failed.is_empty(), "{receipt:?}");
        }
    }

    fn ctx(meta: &[(&str, Value)]) -> BaseCtx {
        let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx().base;
        let extra = meta
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect();
        ctx.set_state(SessionRequestMeta::new(RequestMeta {
            extra,
            ..Default::default()
        }));
        ctx
    }

    /// `ctx` with a user who answers every approval card with `approve`, and
    /// the cards they were shown.
    fn answering(ctx: BaseCtx, approve: bool) -> (BaseCtx, Arc<Mutex<Vec<Value>>>) {
        answering_with(ctx, approve, None)
    }

    fn answering_with(
        ctx: BaseCtx,
        approve: bool,
        remember: Option<bool>,
    ) -> (BaseCtx, Arc<Mutex<Vec<Value>>>) {
        let caller = ctx.caller().to_text();
        let runtime = Arc::new(ActionRuntime::new());
        let (event_sender, mut event_rx) = tokio::sync::mpsc::channel(4);
        ctx.set_state(ActionSession::new(
            runtime.clone(),
            event_sender,
            caller.clone(),
            "session_test".to_string(),
            Arc::new(std::sync::atomic::AtomicU64::new(1)),
            Arc::new(anda_engine::model::Models::default()),
            std::env::temp_dir(),
        ));
        let cards = Arc::new(Mutex::new(Vec::new()));
        let shown = cards.clone();
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if let ActionEvent::Add(message) = event
                    && let Some(action_id) = action_id_from_message(&message)
                {
                    let payload =
                        serde_json::to_value(&message).unwrap()["content"][0]["payload"].clone();
                    shown.lock().push(payload);
                    let _ = runtime
                        .respond(
                            &caller,
                            0,
                            ActionResponseArgs {
                                action_id,
                                approve: Some(approve),
                                choice_id: None,
                                choice_text: None,
                                remember,
                            },
                        )
                        .await;
                }
            }
        });
        (ctx, cards)
    }

    fn ran(result: &Result<ToolOutput<Json>, BoxError>) -> bool {
        matches!(result, Ok(output) if output.is_error != Some(true))
    }

    fn refused(result: &Result<ToolOutput<Json>, BoxError>) -> Option<String> {
        let output = result.as_ref().ok()?;
        (output.is_error == Some(true)).then(|| {
            output.output["error"]["code"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
    }

    #[tokio::test]
    async fn reviewed_read_only_tools_run_and_the_rest_ask() {
        let mock = fixture(
            json!({ "type": "http" }),
            vec![test_server::read_only_tool("search"), write_tool("delete")],
        )
        .await;
        let on_risk = || ctx(&[]);

        // The first call pins the catalog the server came with.
        assert!(ran(&mock.call(on_risk(), "mcp_mock_search").await));
        let diff = mock.manager.tool_diff("mock", "search").unwrap();
        assert_eq!(diff.review, McpReview::Trusted);
        assert!(diff.reviewed_at.is_some());

        // Nobody can answer here, so the model gets a reason and a way out.
        let result = mock.call(on_risk(), "mcp_mock_delete").await;
        assert_eq!(refused(&result).as_deref(), Some("approval_required"));
        let error = &result.unwrap().output["error"];
        assert!(
            error["hint"]
                .as_str()
                .unwrap()
                .contains("anda mcp approval mock allow --tool delete"),
            "{error}"
        );

        let full_access = ctx(&[("approval_mode", json!("full_access"))]);
        assert!(ran(&mock.call(full_access, "mcp_mock_delete").await));

        let (approving, cards) = answering(on_risk(), true);
        assert!(ran(&mock.call(approving, "mcp_mock_delete").await));
        let card = cards.lock().pop().unwrap();
        assert_eq!(card["title"], "Run MCP tool: mock · delete");
        assert_eq!(card["metadata"]["review"], "trusted");
        assert_eq!(card["metadata"]["approval"], "auto");
        assert!(
            card["details"].to_string().contains("\\\"q\\\": \\\"x\\\""),
            "{card}"
        );

        let (denying, _) = answering(on_risk(), false);
        let err = mock.call(denying, "mcp_mock_delete").await.unwrap_err();
        assert!(err.to_string().contains("denied"), "{err}");

        // Every call made counted; refused and denied ones did not.
        let usage = mock.manager.server("mock").await.unwrap().server.usage;
        assert_eq!((usage.calls, usage.errors), (3, 0));
        assert!(usage.last_used_at.is_some());
    }

    #[tokio::test]
    async fn always_allow_on_the_card_stops_asking_for_that_tool() {
        let mock = fixture(json!({}), vec![write_tool("delete"), write_tool("create")]).await;
        let (always, cards) = answering_with(ctx(&[]), true, Some(true));
        assert!(ran(&mock.call(always, "mcp_mock_delete").await));
        let card = cards.lock().pop().unwrap();
        assert_eq!(card["approval"]["remember_label"], "Always allow");
        assert_eq!(
            mock.manager.tool_policy("mock", "delete").approval,
            McpApproval::Allow
        );
        let entry = &crate::engine::mcp::config_store::McpConfigFile::read(
            &McpSettings::file_path(mock._dir.path()),
        )
        .await
        .unwrap()
        .root()["mcpServers"]["mock"];
        assert_eq!(entry["approval"], json!({ "tools": { "delete": "allow" } }));

        // Nobody is asked about it again; the other tool still asks.
        assert!(ran(&mock.call(ctx(&[]), "mcp_mock_delete").await));
        let result = mock.call(ctx(&[]), "mcp_mock_create").await;
        assert_eq!(refused(&result).as_deref(), Some("approval_required"));
    }

    #[tokio::test]
    async fn explicit_policies_beat_the_approval_mode() {
        let mock = fixture(
            json!({ "approval": { "default": "allow", "tools": { "search": "ask" } } }),
            vec![test_server::read_only_tool("search"), write_tool("delete")],
        )
        .await;

        assert!(ran(&mock.call(ctx(&[]), "mcp_mock_delete").await));
        let every_call = ctx(&[("approval_mode", json!("request_approval"))]);
        assert!(ran(&mock.call(every_call, "mcp_mock_delete").await));

        // `ask` asks even with full access, and a scheduled job cannot answer.
        let full_access = ctx(&[("approval_mode", json!("full_access"))]);
        let result = mock.call(full_access, "mcp_mock_search").await;
        assert_eq!(refused(&result).as_deref(), Some("approval_required"));
        let cron = ctx(&[("cron_job_id", json!(7u64))]);
        assert!(ran(&mock.call(cron.clone(), "mcp_mock_delete").await));
        let result = mock.call(cron, "mcp_mock_search").await;
        let message = result.unwrap().output["error"]["message"].to_string();
        assert!(message.contains("cron job"), "{message}");
    }

    #[tokio::test]
    async fn a_changed_definition_is_asked_about_until_accepted() {
        let mock = fixture(
            json!({ "approval": { "default": "allow" } }),
            vec![write_tool("delete")],
        )
        .await;
        assert!(ran(&mock.call(ctx(&[]), "mcp_mock_delete").await));

        mock.catalog.write()[0]["description"] = json!("Deletes things, and mails them out.");
        mock.republish().await;
        let result = mock.call(ctx(&[]), "mcp_mock_delete").await;
        assert_eq!(refused(&result).as_deref(), Some("approval_required"));

        let diff = mock.manager.tool_diff("mock", "delete").unwrap();
        assert_eq!(diff.review, McpReview::Changed);
        assert_eq!(diff.changes.len(), 1);
        assert_eq!(diff.changes[0].field, "description");
        let detail = mock.manager.server("mock").await.unwrap();
        assert_eq!(detail.server.tools.needs_review, 1);
        assert_eq!(detail.tools[0].review, Some(McpReview::Changed));
        assert_eq!(detail.tools[0].approval, Some(McpApproval::Allow));

        // Approving the call accepts the definition it showed.
        let (approving, cards) = answering(ctx(&[]), true);
        assert!(ran(&mock.call(approving, "mcp_mock_delete").await));
        let card = cards.lock().pop().unwrap();
        assert_eq!(card["metadata"]["review"], "changed");
        assert!(card["message"].as_str().unwrap().contains("changed"));
        assert!(ran(&mock.call(ctx(&[]), "mcp_mock_delete").await));

        // A tool the server adds later is new until reviewed.
        mock.catalog.write().push(write_tool("create"));
        mock.republish().await;
        let result = mock.call(ctx(&[]), "mcp_mock_create").await;
        assert_eq!(refused(&result).as_deref(), Some("approval_required"));
        let receipt = mock
            .manager
            .apply(
                McpChange::MarkReviewed {
                    id: "mock".into(),
                    tools: Vec::new(),
                },
                None,
                McpSource::Manual,
            )
            .await
            .unwrap();
        assert_eq!(receipt.reviewed, ["create", "delete"]);
        assert!(ran(&mock.call(ctx(&[]), "mcp_mock_create").await));
    }

    #[tokio::test]
    async fn external_users_reach_only_servers_that_allow_them() {
        let mock = fixture(
            json!({}),
            vec![test_server::read_only_tool("search"), write_tool("delete")],
        )
        .await;
        let external = || ctx(&[("external_user", json!(true))]);

        let result = mock.call(external(), "mcp_mock_search").await;
        assert_eq!(refused(&result).as_deref(), Some("external_user"));

        mock.manager
            .apply(
                McpChange::SetExternalUsers {
                    id: "mock".into(),
                    allowed: true,
                },
                None,
                McpSource::Manual,
            )
            .await
            .unwrap();
        assert!(ran(&mock.call(external(), "mcp_mock_search").await));
        // An external user cannot approve for the owner, so nothing is shown.
        let (asking, cards) = answering(external(), true);
        let result = mock.call(asking, "mcp_mock_delete").await;
        assert_eq!(refused(&result).as_deref(), Some("approval_required"));
        assert!(cards.lock().is_empty());
    }

    #[tokio::test]
    async fn event_automation_runs_use_only_read_only_and_allowed_tools() {
        let mock = fixture(
            json!({}),
            vec![test_server::read_only_tool("search"), write_tool("delete")],
        )
        .await;
        let automation = || ctx(&[("mcp_trigger_id", json!(5u64))]);
        // The events are untrusted, so the run gets no full access: the
        // write tool needs approval, which nobody can give.
        assert!(ran(&mock.call(automation(), "mcp_mock_search").await));
        let (asking, cards) = answering(automation(), true);
        let result = mock.call(asking, "mcp_mock_delete").await;
        assert_eq!(refused(&result).as_deref(), Some("approval_required"));
        assert!(cards.lock().is_empty());

        mock.manager
            .apply(
                McpChange::SetApproval {
                    id: "mock".into(),
                    tool: Some("delete".into()),
                    approval: Some(McpApproval::Allow),
                },
                None,
                McpSource::Manual,
            )
            .await
            .unwrap();
        assert!(ran(&mock.call(automation(), "mcp_mock_delete").await));
    }

    #[tokio::test]
    async fn other_agents_tool_names_reach_the_tool() {
        let mock = fixture(
            json!({}),
            vec![
                test_server::read_only_tool("search"),
                write_tool("delete"),
                test_server::read_only_tool("Search.V2"),
            ],
        )
        .await;
        let names: Vec<String> = [
            "mcp_mock_search",
            "mcp__mock__delete",
            "mcp__MOCK__search",
            "mcp__mock__search.v2",
            "mcp__mock__*",
            "mcp__other__search",
        ]
        .map(str::to_string)
        .into();
        let mut defined: Vec<String> = mock
            .gate
            .definitions(Some(&names))
            .into_iter()
            .map(|definition| definition.name)
            .collect();
        defined.sort();
        // An alias that is no valid tool name is offered by the Anda name.
        assert_eq!(
            defined,
            [
                "mcp__mock__delete",
                "mcp__mock__search",
                "mcp_mock_search",
                "mcp_mock_search_v2"
            ]
        );
        assert!(mock.gate.contains_lowercase("mcp__mock__delete"));
        assert!(!mock.gate.contains_lowercase("mcp__mock__*"));
        assert!(!mock.gate.contains_lowercase("mcp__other__search"));

        let full_access = ctx(&[("approval_mode", json!("full_access"))]);
        let output = mock.call(full_access, "mcp__mock__delete").await.unwrap();
        assert!(
            output.output.to_string().contains("called delete"),
            "{:?}",
            output.output
        );
        let err = mock.call(ctx(&[]), "mcp__mock__*").await.unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
    }
}
