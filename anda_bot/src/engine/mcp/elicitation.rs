//! [`McpElicitations`]: the questions an MCP server asks the user while one
//! of its tools runs (MCP elicitation).
//!
//! A form becomes one choice card per field and a request to open a page one
//! card with the link; the answers go back to the server. A question is asked
//! in the conversation whose call the server is serving: [`McpGate`] notes
//! each call while it runs. A question nobody can answer is cancelled: no
//! call of that server is running, or more than one is, or the run is one
//! nobody watches (an automation, a scheduled job, an IM chat). So is one the
//! user answers in chat instead.
//!
//! [`McpGate`]: super::McpGate

use anda_core::{BoxError, CancellationToken};
use anda_engine::{context::BaseCtx, extension::mcp::McpElicitationHandler};
use async_trait::async_trait;
use parking_lot::Mutex;
use rmcp::model::{
    ElicitRequestParams, ElicitResult, ElicitationAction, ElicitationCapability,
    FormElicitationCapability, UrlElicitationCapability,
};
use serde_json::{Map, Value, json};
use std::{
    collections::HashMap,
    future::Future,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use crate::engine::{
    ActionSession, McpInputAnswer, McpInputCard, UserChoiceInput, UserChoiceOption, approval_detail,
};

/// Fields one form may ask for.
const MAX_FIELDS: usize = 12;
/// Answers to a field that fail its checks before the form is cancelled.
const MAX_ATTEMPTS: usize = 3;
/// Options shown as rows of their own; a field with more takes typed text.
const MAX_OPTION_ROWS: usize = 8;
/// How much of the server's message and of a field's description a card shows.
const MESSAGE_CHARS: usize = 1_000;
const DESCRIPTION_CHARS: usize = 300;
const NOTE: &str = "This answer goes to the MCP server. Never enter passwords, keys or payment \
                    details here.";

tokio::task_local! {
    /// The server call the current task makes, and who it is for.
    static CALL: (String, BaseCtx, Duration);
}

/// Routes servers' questions to the user each call runs for, and asks them.
#[derive(Default)]
pub(crate) struct McpElicitations {
    calls: Mutex<HashMap<u64, (String, BaseCtx, Duration)>>,
    next: AtomicU64,
}

impl McpElicitations {
    /// Runs `call`, a request to server `server_id` made for `ctx`, so that
    /// the server's questions during it reach that user. `timeout` is how
    /// long the server waits for an answer.
    pub(crate) async fn during<T>(
        &self,
        server_id: &str,
        ctx: &BaseCtx,
        timeout: Duration,
        call: impl Future<Output = T>,
    ) -> T {
        struct Done<'a>(&'a McpElicitations, u64);
        impl Drop for Done<'_> {
            fn drop(&mut self) {
                self.0.calls.lock().remove(&self.1);
            }
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let call_of = (server_id.to_string(), ctx.clone(), timeout);
        self.calls.lock().insert(id, call_of.clone());
        let _done = Done(self, id);
        CALL.scope(call_of, call).await
    }

    /// Who a question from `server_id` is for: the call the asking task makes
    /// (a question within the call's own rounds), else the only call of that
    /// server that runs (one the server sends on its own).
    fn asker(&self, server_id: &str) -> Option<(BaseCtx, Duration)> {
        if let Ok(Some(asker)) =
            CALL.try_with(|(id, ctx, timeout)| (id == server_id).then(|| (ctx.clone(), *timeout)))
        {
            return Some(asker);
        }
        let calls = self.calls.lock();
        let mut calls = calls.values().filter(|(id, _, _)| id == server_id);
        let (_, ctx, timeout) = calls.next()?;
        calls.next().is_none().then(|| (ctx.clone(), *timeout))
    }
}

#[async_trait]
impl McpElicitationHandler for McpElicitations {
    fn capabilities(&self) -> ElicitationCapability {
        ElicitationCapability::new()
            .with_form(FormElicitationCapability::default())
            .with_url(UrlElicitationCapability::new())
    }

    async fn elicit(
        &self,
        server_id: &str,
        request: ElicitRequestParams,
        _cancellation: CancellationToken,
    ) -> Result<ElicitResult, BoxError> {
        let Some((ctx, timeout)) = self.asker(server_id) else {
            log::info!("MCP server {server_id} asked for input outside a call it serves");
            return Ok(ElicitResult::new(ElicitationAction::Cancel));
        };
        let Some(session) = ctx.get_state::<ActionSession>() else {
            return Ok(ElicitResult::new(ElicitationAction::Cancel));
        };
        let asking = Asking {
            server_id,
            session: &session,
            ctx: &ctx,
            deadline: Instant::now() + timeout,
        };
        let result = match request {
            ElicitRequestParams::FormElicitationParams {
                message,
                requested_schema,
                ..
            } => {
                asking
                    .form(&message, &serde_json::to_value(requested_schema)?)
                    .await
            }
            ElicitRequestParams::UrlElicitationParams { message, url, .. } => {
                asking.page(&message, &url).await
            }
            _ => ElicitResult::new(ElicitationAction::Cancel),
        };
        Ok(result)
    }
}

/// One question being asked.
struct Asking<'a> {
    server_id: &'a str,
    session: &'a ActionSession,
    ctx: &'a BaseCtx,
    deadline: Instant,
}

impl Asking<'_> {
    /// Asks for a form's fields one card at a time.
    async fn form(&self, message: &str, schema: &Value) -> ElicitResult {
        let fields = match form_fields(schema) {
            Ok(fields) if fields.len() <= MAX_FIELDS => fields,
            Ok(_) => return self.cancel(&format!("a form of more than {MAX_FIELDS} fields")),
            Err(reason) => return self.cancel(&reason),
        };
        let mut content = Map::new();
        for (index, field) in fields.iter().enumerate() {
            let step = (fields.len() > 1).then_some((index + 1, fields.len()));
            let mut problem = None;
            let mut attempts = 0;
            let value = loop {
                let card = field_card(self.server_id, message, field, step, problem.as_deref());
                let (id, text) = match self.ask(card).await {
                    Some(answer) => answer,
                    None => return ElicitResult::new(ElicitationAction::Cancel),
                };
                match id.as_str() {
                    DECLINE => return ElicitResult::new(ElicitationAction::Decline),
                    SKIP => break None,
                    _ => {}
                }
                match field.answer(&id, text.as_deref()) {
                    Ok(value) => break value,
                    Err(reason) => {
                        attempts += 1;
                        if attempts >= MAX_ATTEMPTS {
                            return self.cancel(&format!("{}: {reason}", field.name));
                        }
                        problem = Some(reason);
                    }
                }
            };
            if let Some(value) = value {
                content.insert(field.name.clone(), value);
            }
        }
        ElicitResult::new(ElicitationAction::Accept).with_content(Value::Object(content))
    }

    /// Asks to open a page; the client opens it when the user agrees.
    async fn page(&self, message: &str, url: &str) -> ElicitResult {
        let Some(host) = reqwest::Url::parse(url)
            .ok()
            .filter(|url| matches!(url.scheme(), "https" | "http"))
            .and_then(|url| url.host_str().map(str::to_string))
        else {
            return self.cancel("a link that is not http(s)");
        };
        let card = McpInputCard {
            tool: format!("mcp:{}", self.server_id),
            tool_label: "MCP server request".to_string(),
            title: format!("{} asks you to open a page", self.server_id),
            message: Some(clip(message, MESSAGE_CHARS)),
            details: vec![
                approval_detail("MCP server", self.server_id, "text"),
                approval_detail("Site", &host, "text"),
                approval_detail("Link", url, "url"),
            ],
            choices: vec![
                UserChoiceOption {
                    url: Some(url.to_string()),
                    ..option(OPEN, "Open the page", Some(&format!("Opens {host}")))
                },
                option(DECLINE, "Decline", Some("Tell the server you won't")),
            ],
            metadata: json!({"server_id": self.server_id, "mode": "url", "url": url}),
            timeout: Duration::ZERO,
        };
        match self.ask(card).await {
            Some((id, _)) if id == OPEN => ElicitResult::new(ElicitationAction::Accept),
            Some((id, _)) if id == DECLINE => ElicitResult::new(ElicitationAction::Decline),
            _ => ElicitResult::new(ElicitationAction::Cancel),
        }
    }

    /// Shows one card until the server's deadline; `None` when nobody
    /// answered it with an option.
    async fn ask(&self, mut card: McpInputCard) -> Option<(String, Option<String>)> {
        card.timeout = self.deadline.saturating_duration_since(Instant::now());
        if card.timeout.is_zero() {
            return None;
        }
        match self.session.request_mcp_input(self.ctx, card).await {
            Ok(McpInputAnswer::Choice { id, text }) => Some((id, text)),
            Ok(McpInputAnswer::InChat) => None,
            Ok(McpInputAnswer::Unanswered(reason)) => {
                log::info!("MCP server {} asked for input: {reason}", self.server_id);
                None
            }
            Err(err) => {
                log::warn!("MCP server {} asked for input: {err}", self.server_id);
                None
            }
        }
    }

    fn cancel(&self, reason: &str) -> ElicitResult {
        log::warn!(
            "MCP server {} asked for input Anda cannot ask: {reason}",
            self.server_id
        );
        ElicitResult::new(ElicitationAction::Cancel)
    }
}

const DECLINE: &str = "decline";
const SKIP: &str = "skip";
const SUBMIT: &str = "submit";
const OPEN: &str = "open";

/// One field of a form.
#[derive(Debug, PartialEq)]
struct Field {
    name: String,
    title: String,
    description: Option<String>,
    required: bool,
    kind: Kind,
}

#[derive(Debug, PartialEq)]
enum Kind {
    Text {
        format: Option<String>,
        min_length: Option<u64>,
        max_length: Option<u64>,
    },
    Number {
        integer: bool,
        minimum: Option<f64>,
        maximum: Option<f64>,
    },
    Boolean,
    /// One of the options: `(value, label)`.
    Choice(Vec<(String, String)>),
    /// Some of the options.
    Many {
        options: Vec<(String, String)>,
        min_items: Option<u64>,
        max_items: Option<u64>,
    },
}

/// The fields of a form's schema, in its order. Elicitation schemas are flat
/// objects of strings, numbers, booleans and enums.
fn form_fields(schema: &Value) -> Result<Vec<Field>, String> {
    let properties = match schema.get("properties") {
        None => return Ok(Vec::new()),
        Some(Value::Object(properties)) => properties,
        Some(_) => return Err("a form whose properties are not an object".to_string()),
    };
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    properties
        .iter()
        .map(|(name, property)| {
            let text = |key: &str| property.get(key).and_then(Value::as_str);
            let number = |key: &str| property.get(key).and_then(Value::as_f64);
            let count = |key: &str| property.get(key).and_then(Value::as_u64);
            let kind = if let Some(options) = options_of(property) {
                Kind::Choice(options)
            } else {
                match text("type") {
                    Some("string") => Kind::Text {
                        format: text("format").map(str::to_string),
                        min_length: count("minLength"),
                        max_length: count("maxLength"),
                    },
                    Some(kind @ ("number" | "integer")) => Kind::Number {
                        integer: kind == "integer",
                        minimum: number("minimum"),
                        maximum: number("maximum"),
                    },
                    Some("boolean") => Kind::Boolean,
                    Some("array") => match property.get("items").and_then(options_of) {
                        Some(options) => Kind::Many {
                            options,
                            min_items: count("minItems"),
                            max_items: count("maxItems"),
                        },
                        None => return Err(format!("field {name}: a list without options")),
                    },
                    other => return Err(format!("field {name}: an unsupported type {other:?}")),
                }
            };
            Ok(Field {
                name: name.clone(),
                title: text("title")
                    .map(str::trim)
                    .filter(|title| !title.is_empty())
                    .unwrap_or(name)
                    .to_string(),
                description: text("description").map(|text| clip(text, DESCRIPTION_CHARS)),
                required: required.contains(&name.as_str()),
                kind,
            })
        })
        .collect()
}

/// The options of an enum: `enum` (with `enumNames` as labels), or `oneOf` /
/// `anyOf` of `{const, title}`.
fn options_of(schema: &Value) -> Option<Vec<(String, String)>> {
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        let names = schema.get("enumNames").and_then(Value::as_array);
        return values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let value = value.as_str()?.to_string();
                let label = names
                    .and_then(|names| names.get(index))
                    .and_then(Value::as_str)
                    .unwrap_or(&value)
                    .to_string();
                Some((value, label))
            })
            .collect();
    }
    let titled = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))?
        .as_array()?;
    titled
        .iter()
        .map(|option| {
            let value = option.get("const")?.as_str()?.to_string();
            let label = option
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(&value)
                .to_string();
            Some((value, label))
        })
        .collect()
}

impl Field {
    /// The value an answer gives the field, `None` to leave it out, or what
    /// is wrong with it.
    fn answer(&self, choice: &str, text: Option<&str>) -> Result<Option<Value>, String> {
        let text = text.map(str::trim).unwrap_or_default();
        match &self.kind {
            Kind::Boolean => Ok(Some(Value::Bool(choice == "true"))),
            Kind::Choice(options) if options.len() <= MAX_OPTION_ROWS => {
                let index: usize = choice
                    .strip_prefix('o')
                    .and_then(|index| index.parse().ok())
                    .ok_or("an unknown option")?;
                let (value, _) = options.get(index).ok_or("an unknown option")?;
                Ok(Some(Value::String(value.clone())))
            }
            _ if text.is_empty() => {
                if self.required {
                    Err("an answer is required".to_string())
                } else {
                    Ok(None)
                }
            }
            Kind::Choice(options) => pick(options, text).map(|value| Some(Value::String(value))),
            Kind::Many {
                options,
                min_items,
                max_items,
            } => {
                let picked = text
                    .split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(|item| pick(options, item).map(Value::String))
                    .collect::<Result<Vec<_>, _>>()?;
                let count = picked.len() as u64;
                if min_items.is_some_and(|min| count < min)
                    || max_items.is_some_and(|max| count > max)
                {
                    return Err(format!(
                        "pick {} options",
                        range_text(min_items.map(|v| v as f64), max_items.map(|v| v as f64))
                    ));
                }
                Ok(Some(Value::Array(picked)))
            }
            Kind::Number {
                integer,
                minimum,
                maximum,
            } => {
                let number: f64 = text.parse().map_err(|_| "a number is expected")?;
                if *integer && number.fract() != 0.0 {
                    return Err("a whole number is expected".to_string());
                }
                if minimum.is_some_and(|min| number < min)
                    || maximum.is_some_and(|max| number > max)
                {
                    return Err(format!("a number {}", range_text(*minimum, *maximum)));
                }
                Ok(Some(if *integer {
                    json!(number as i64)
                } else {
                    json!(number)
                }))
            }
            Kind::Text {
                format,
                min_length,
                max_length,
            } => {
                let length = text.chars().count() as u64;
                if min_length.is_some_and(|min| length < min)
                    || max_length.is_some_and(|max| length > max)
                {
                    return Err(format!(
                        "{} characters",
                        range_text(min_length.map(|v| v as f64), max_length.map(|v| v as f64))
                    ));
                }
                let valid = match format.as_deref() {
                    Some("email") => text.split_once('@').is_some_and(|(user, domain)| {
                        !user.is_empty() && domain.contains('.') && !text.contains(' ')
                    }),
                    Some("uri") => reqwest::Url::parse(text).is_ok(),
                    Some("date") => chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok(),
                    Some("date-time") => chrono::DateTime::parse_from_rfc3339(text).is_ok(),
                    _ => true,
                };
                if !valid {
                    return Err(format!(
                        "a valid {}",
                        format_hint(format.as_deref()).unwrap_or("value")
                    ));
                }
                Ok(Some(Value::String(text.to_string())))
            }
        }
    }
}

/// The option `text` names, by value or label.
fn pick(options: &[(String, String)], text: &str) -> Result<String, String> {
    options
        .iter()
        .find(|(value, label)| value == text || label.eq_ignore_ascii_case(text))
        .map(|(value, _)| value.clone())
        .ok_or_else(|| {
            let names: Vec<&str> = options.iter().map(|(value, _)| value.as_str()).collect();
            format!("one of {}", names.join(", "))
        })
}

fn range_text(min: Option<f64>, max: Option<f64>) -> String {
    match (min, max) {
        (Some(min), Some(max)) => format!("from {min} to {max}"),
        (Some(min), None) => format!("of at least {min}"),
        (None, Some(max)) => format!("of at most {max}"),
        (None, None) => String::new(),
    }
}

fn format_hint(format: Option<&str>) -> Option<&'static str> {
    match format? {
        "email" => Some("email address"),
        "uri" => Some("URL"),
        "date" => Some("date (YYYY-MM-DD)"),
        "date-time" => Some("date and time (RFC 3339)"),
        _ => None,
    }
}

/// The card that asks for one field.
fn field_card(
    server_id: &str,
    message: &str,
    field: &Field,
    step: Option<(usize, usize)>,
    problem: Option<&str>,
) -> McpInputCard {
    let mut title = format!("{server_id}: {}", field.title);
    if let Some((index, count)) = step {
        title.push_str(&format!(" ({index}/{count})"));
    }
    let mut text = clip(message, MESSAGE_CHARS);
    if let Some(description) = &field.description {
        text.push_str("\n\n");
        text.push_str(description);
    }
    if let Some(problem) = problem {
        text.push_str(&format!("\n\nThat answer did not fit: {problem}."));
    }
    let typed = |placeholder: String| UserChoiceOption {
        input: Some(UserChoiceInput {
            placeholder: Some(placeholder),
            required: field.required,
            multiline: false,
        }),
        ..option(SUBMIT, "Send", None)
    };
    let mut choices = match &field.kind {
        Kind::Boolean => vec![option("true", "Yes", None), option("false", "No", None)],
        Kind::Choice(options) if options.len() <= MAX_OPTION_ROWS => options
            .iter()
            .enumerate()
            .map(|(index, (value, label))| {
                let detail = (value != label).then_some(value.as_str());
                option(&format!("o{index}"), label, detail)
            })
            .collect(),
        Kind::Choice(options) => vec![typed(format!("One of: {}", option_names(options)))],
        Kind::Many { options, .. } => vec![typed(format!(
            "Comma-separated, from: {}",
            option_names(options)
        ))],
        Kind::Number {
            integer,
            minimum,
            maximum,
        } => vec![typed(
            format!(
                "{} {}",
                if *integer {
                    "A whole number"
                } else {
                    "A number"
                },
                range_text(*minimum, *maximum)
            )
            .trim()
            .to_string(),
        )],
        Kind::Text { format, .. } => vec![typed(
            format_hint(format.as_deref())
                .map(|hint| format!("A {hint}"))
                .unwrap_or_else(|| field.title.clone()),
        )],
    };
    if !field.required && !matches!(field.kind, Kind::Text { .. } | Kind::Number { .. }) {
        choices.push(option(SKIP, "Skip", Some("Leave it out")));
    }
    choices.push(option(
        DECLINE,
        "Decline",
        Some("Tell the server you won't answer"),
    ));
    McpInputCard {
        tool: format!("mcp:{server_id}"),
        tool_label: "MCP server request".to_string(),
        title,
        message: Some(text),
        details: vec![
            approval_detail("MCP server", server_id, "text"),
            approval_detail("Note", NOTE, "text"),
        ],
        choices,
        metadata: json!({"server_id": server_id, "mode": "form", "field": field.name}),
        timeout: Duration::ZERO,
    }
}

fn option_names(options: &[(String, String)]) -> String {
    clip(
        &options
            .iter()
            .map(|(value, _)| value.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        DESCRIPTION_CHARS,
    )
}

fn option(id: &str, label: &str, description: Option<&str>) -> UserChoiceOption {
    UserChoiceOption {
        id: id.to_string(),
        label: label.to_string(),
        value: None,
        description: description.map(str::to_string),
        input: None,
        url: None,
    }
}

fn clip(text: &str, max_chars: usize) -> String {
    let text = text.trim();
    match text.char_indices().nth(max_chars) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{
        ActionEvent, ActionResponseArgs, ActionRuntime, action_id_from_message,
        agent::SessionRequestMeta,
    };
    use crate::util::request_meta::keys;
    use anda_core::{RequestMeta, StateFeatures};
    use anda_engine::extension::mcp::McpElicitationHandler;
    use std::sync::Arc;

    type Cards = Arc<Mutex<Vec<Value>>>;

    fn ctx(meta: &[(&str, Value)]) -> BaseCtx {
        let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx().base;
        ctx.set_state(SessionRequestMeta::new(RequestMeta {
            extra: meta
                .iter()
                .map(|(key, value)| (key.to_string(), value.clone()))
                .collect(),
            ..Default::default()
        }));
        ctx
    }

    /// `ctx` with a user who answers the cards in turn with `answers`
    /// (choice id, typed text), and the cards they were shown. Past the
    /// answers, cards wait.
    fn answering(
        ctx: BaseCtx,
        answers: Vec<(&'static str, Option<&'static str>)>,
    ) -> (BaseCtx, Cards) {
        let caller = ctx.caller().to_text();
        let runtime = Arc::new(ActionRuntime::new());
        let (event_sender, mut event_rx) = tokio::sync::mpsc::channel(8);
        ctx.set_state(ActionSession::new(
            runtime.clone(),
            event_sender,
            caller.clone(),
            "session_test".to_string(),
            Arc::new(AtomicU64::new(1)),
            Arc::new(anda_engine::model::Models::default()),
            std::env::temp_dir(),
        ));
        let cards = Cards::default();
        let shown = cards.clone();
        tokio::spawn(async move {
            let mut answers = answers.into_iter();
            while let Some(event) = event_rx.recv().await {
                match event {
                    ActionEvent::Add(message) => {
                        let action_id = action_id_from_message(&message).unwrap();
                        let payload =
                            serde_json::to_value(&message).unwrap()["content"][0]["payload"]
                                .clone();
                        shown.lock().push(payload);
                        if let Some((choice, text)) = answers.next() {
                            runtime
                                .respond(
                                    &caller,
                                    0,
                                    ActionResponseArgs {
                                        action_id,
                                        approve: None,
                                        choice_id: Some(choice.to_string()),
                                        choice_text: text.map(str::to_string),
                                        remember: None,
                                    },
                                )
                                .await
                                .unwrap();
                        }
                    }
                    ActionEvent::Resolve {
                        action_id, status, ..
                    } => shown
                        .lock()
                        .push(json!({"resolved": action_id, "status": status})),
                }
            }
        });
        (ctx, cards)
    }

    fn form(schema: Value) -> ElicitRequestParams {
        serde_json::from_value(json!({
            "mode": "form", "message": "Where should the issue go?", "requestedSchema": schema
        }))
        .unwrap()
    }

    /// Asks `request` of server `mock` during a call made for `ctx`.
    async fn ask(ctx: &BaseCtx, request: ElicitRequestParams, timeout: Duration) -> ElicitResult {
        let elicitations = McpElicitations::default();
        elicitations
            .during(
                "mock",
                ctx,
                timeout,
                elicitations.elicit("mock", request, CancellationToken::new()),
            )
            .await
            .unwrap()
    }

    fn issue_form() -> Value {
        json!({
            "type": "object",
            "properties": {
                "repo": {"type": "string", "title": "Repository", "description": "owner/name"},
                "count": {"type": "integer", "minimum": 1, "maximum": 5},
                "private": {"type": "boolean"},
                "kind": {"type": "string", "enum": ["bug", "feature"], "enumNames": ["Bug", "Feature"]},
                "labels": {"type": "array", "items": {"anyOf": [
                    {"const": "p0", "title": "Urgent"}, {"const": "docs", "title": "Docs"}
                ]}}
            },
            "required": ["repo", "count"]
        })
    }

    #[test]
    fn forms_are_flat_fields_in_schema_order() {
        let fields = form_fields(&issue_form()).unwrap();
        let names: Vec<_> = fields.iter().map(|field| field.name.as_str()).collect();
        assert_eq!(names, ["repo", "count", "private", "kind", "labels"]);
        assert_eq!(fields[0].title, "Repository");
        assert!(fields[0].required && !fields[2].required);
        assert_eq!(
            fields[3].kind,
            Kind::Choice(vec![
                ("bug".into(), "Bug".into()),
                ("feature".into(), "Feature".into())
            ])
        );
        assert!(matches!(&fields[4].kind, Kind::Many { options, .. } if options[0].1 == "Urgent"));
        assert!(form_fields(&json!({"properties": {"x": {"type": "object"}}})).is_err());
        assert_eq!(form_fields(&json!({"type": "object"})).unwrap(), []);

        // Answers are checked against the field.
        let count = &fields[1];
        assert_eq!(count.answer(SUBMIT, Some(" 3 ")), Ok(Some(json!(3))));
        assert!(
            count
                .answer(SUBMIT, Some("9"))
                .unwrap_err()
                .contains("from 1 to 5")
        );
        assert!(count.answer(SUBMIT, Some("2.5")).is_err());
        assert!(
            count
                .answer(SUBMIT, Some(""))
                .unwrap_err()
                .contains("required")
        );
        assert_eq!(fields[3].answer("o1", None), Ok(Some(json!("feature"))));
        assert_eq!(
            fields[4].answer(SUBMIT, Some("p0, Docs")),
            Ok(Some(json!(["p0", "docs"])))
        );
        assert!(fields[4].answer(SUBMIT, Some("p1")).is_err());
        let email = Field {
            name: "email".into(),
            title: "Email".into(),
            description: None,
            required: false,
            kind: Kind::Text {
                format: Some("email".into()),
                min_length: None,
                max_length: None,
            },
        };
        assert!(email.answer(SUBMIT, Some("nobody")).is_err());
        assert_eq!(email.answer(SUBMIT, Some("")), Ok(None));
        assert_eq!(
            email.answer(SUBMIT, Some("a@b.dev")),
            Ok(Some(json!("a@b.dev")))
        );
    }

    #[tokio::test]
    async fn a_form_is_asked_one_field_at_a_time() {
        let (ctx, cards) = answering(
            ctx(&[]),
            vec![
                (SUBMIT, Some("ldclabs/anda")),
                (SUBMIT, Some("9")),
                (SUBMIT, Some("3")),
                ("true", None),
                ("o1", None),
                (SKIP, None),
            ],
        );
        let result = ask(&ctx, form(issue_form()), Duration::from_secs(60)).await;
        assert_eq!(result.action, ElicitationAction::Accept);
        assert_eq!(
            result.content,
            Some(json!({"repo": "ldclabs/anda", "count": 3, "private": true, "kind": "feature"}))
        );
        let cards = cards.lock().clone();
        let shown: Vec<&Value> = cards
            .iter()
            .filter(|card| card["title"].is_string())
            .collect();
        assert_eq!(shown.len(), 6);
        assert_eq!(shown[0]["title"], "mock: Repository (1/5)");
        assert!(shown[0]["message"].as_str().unwrap().contains("owner/name"));
        assert_eq!(shown[0]["tool"]["name"], "mcp:mock");
        assert_eq!(shown[0]["choices"][0]["input"]["required"], true);
        assert_eq!(shown[0]["choices"][1]["id"], DECLINE);
        // The rejected answer is asked again, saying why.
        assert!(
            shown[2]["message"]
                .as_str()
                .unwrap()
                .contains("from 1 to 5")
        );
        assert_eq!(shown[4]["choices"][1]["label"], "Feature");
        assert!(
            shown[3]["details"]
                .to_string()
                .contains("Never enter passwords")
        );
    }

    #[tokio::test]
    async fn declining_answering_in_chat_or_nobody_to_ask_ends_the_form() {
        let (ctx_decline, _) = answering(ctx(&[]), vec![(DECLINE, None)]);
        let result = ask(&ctx_decline, form(issue_form()), Duration::from_secs(60)).await;
        assert_eq!(result.action, ElicitationAction::Decline);

        // Unanswered by the server's deadline: the card closes, the form is
        // cancelled.
        let (ctx_silent, cards) = answering(ctx(&[]), vec![]);
        let result = ask(&ctx_silent, form(issue_form()), Duration::from_millis(100)).await;
        assert_eq!(result.action, ElicitationAction::Cancel);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(cards.lock().iter().any(|card| card["status"] == "expired"));

        // A card whose call goes away (the server gave up, the task stopped)
        // closes rather than waiting for an answer nobody takes.
        let (ctx_dropped, cards) = answering(ctx(&[]), vec![]);
        let asked = ask(&ctx_dropped, form(issue_form()), Duration::from_secs(60));
        assert!(
            tokio::time::timeout(Duration::from_millis(100), asked)
                .await
                .is_err()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
        let cards = cards.lock().clone();
        assert_eq!(cards.len(), 2, "{cards:?}");
        assert_eq!(cards[1]["status"], "expired");

        // An IM chat shows no cards.
        let (ctx_im, cards) = answering(
            ctx(&[(keys::REPLY_TARGET, json!("chat-1"))]),
            vec![(SUBMIT, Some("x"))],
        );
        let result = ask(&ctx_im, form(issue_form()), Duration::from_secs(60)).await;
        assert_eq!(result.action, ElicitationAction::Cancel);
        assert!(cards.lock().is_empty());

        // Outside any call of the server, and when two calls could be the
        // one asking, nobody is asked.
        let elicitations = McpElicitations::default();
        let outside = elicitations
            .elicit("mock", form(issue_form()), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(outside.action, ElicitationAction::Cancel);
        let (first, _) = answering(ctx(&[]), vec![]);
        let (second, _) = answering(ctx(&[]), vec![]);
        let hold = || tokio::time::sleep(Duration::from_millis(200));
        let ambiguous = async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            elicitations
                .elicit("mock", form(issue_form()), CancellationToken::new())
                .await
                .unwrap()
        };
        let (_, _, ambiguous) = tokio::join!(
            elicitations.during("mock", &first, Duration::from_secs(5), hold()),
            elicitations.during("mock", &second, Duration::from_secs(5), hold()),
            ambiguous
        );
        assert_eq!(ambiguous.action, ElicitationAction::Cancel);
        // With one call of the server running, a question the server sends
        // on its own, outside the call's task, reaches that call's user.
        let (only, cards) = answering(ctx(&[]), vec![(DECLINE, None)]);
        let single = async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            elicitations
                .elicit("mock", form(issue_form()), CancellationToken::new())
                .await
                .unwrap()
        };
        let (_, single) = tokio::join!(
            elicitations.during("mock", &only, Duration::from_secs(5), hold()),
            single
        );
        assert_eq!(single.action, ElicitationAction::Decline);
        assert_eq!(cards.lock()[0]["title"], "mock: Repository (1/5)");
        assert!(elicitations.asker("mock").is_none());
    }

    #[tokio::test]
    async fn a_page_opens_from_its_card() {
        let request = |url: &str| -> ElicitRequestParams {
            serde_json::from_value(json!({
                "mode": "url", "message": "Connect your account", "url": url,
                "elicitationId": "e1"
            }))
            .unwrap()
        };
        let (ctx_open, cards) = answering(ctx(&[]), vec![(OPEN, None)]);
        let result = ask(
            &ctx_open,
            request("https://example.com/connect?x=1"),
            Duration::from_secs(60),
        )
        .await;
        assert_eq!(result.action, ElicitationAction::Accept);
        let card = cards.lock()[0].clone();
        assert_eq!(card["choices"][0]["url"], "https://example.com/connect?x=1");
        assert_eq!(card["choices"][1]["url"], Value::Null);
        assert!(card["details"].to_string().contains("example.com"));

        let (ctx_script, cards) = answering(ctx(&[]), vec![(OPEN, None)]);
        let result = ask(
            &ctx_script,
            request("javascript:alert(1)"),
            Duration::from_secs(60),
        )
        .await;
        assert_eq!(result.action, ElicitationAction::Cancel);
        assert!(cards.lock().is_empty());
    }
}
