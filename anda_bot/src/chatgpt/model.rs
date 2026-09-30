//! Stateless Responses adapter for ChatGPT plan usage. Reuses Engine wire types
//! and output conversion without changing the API-key adapters' contract.
use anda_core::{AgentOutput, BoxError, BoxPinFut, CompletionRequest, ContentPart, Message};
use anda_engine::model::{CompletionFeaturesDyn, Model, ModelError, openai::types as wire};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

use super::{ChatGptService, ProviderError};
const NAMESPACE: &str = "anda";

pub fn completion_model(
    service: Arc<ChatGptService>,
    profile: String,
    config: &crate::config::ModelProvider,
) -> Model {
    let mut model = Model::new(Arc::new(ChatGPTCompleter {
        service,
        profile,
        slug: config.model.clone(),
        id: config.selection_id(),
        effort: config.effort,
    }))
    .with_labels(config.labels.clone());
    model.context_window = config.context_window;
    // Plan requests cannot enforce max_output_tokens on the server.
    model
}
struct ChatGPTCompleter {
    service: Arc<ChatGptService>,
    profile: String,
    slug: String,
    id: String,
    effort: Option<anda_engine::model::ModelEffort>,
}
impl CompletionFeaturesDyn for ChatGPTCompleter {
    fn model_name(&self) -> String {
        self.id.clone()
    }
    fn completion(
        &self,
        mut request: CompletionRequest,
    ) -> BoxPinFut<Result<AgentOutput, BoxError>> {
        let (service, profile, slug, id) = (
            self.service.clone(),
            self.profile.clone(),
            self.slug.clone(),
            self.id.clone(),
        );
        if request.effort.is_none() {
            request.effort = self.effort;
        }
        Box::pin(async move {
            let (body, history, chat) = build_request(&slug, request)?;
            let (mut token, cancel) = service.access(&profile, false).await?;
            let mut auth_retried = false;
            let mut retries = 0;
            loop {
                let response = tokio::select! {
                    _=cancel.cancelled()=>return Err("ChatGPT account was signed out".into()),
                    response=service.http.post(format!("{}/responses",service.endpoints.api)).bearer_auth(token.as_str())
                        .header("Accept","text/event-stream").header("Accept-Encoding","identity").json(&body).send()=>response.map_err(|e|e.without_url())?,
                };
                if response.status().is_success() {
                    let mut response = read_stream(response, &cancel).await?;
                    // Namespace is not part of Anda tool names. Reject other namespaces
                    // instead of routing a provider-selected tool outside this request.
                    for item in &response.output {
                        if item.get("type").and_then(Value::as_str) == Some("function_call")
                            && item
                                .get("namespace")
                                .and_then(Value::as_str)
                                .is_some_and(|v| v != NAMESPACE)
                        {
                            return Err("ChatGPT returned an unexpected tool namespace".into());
                        }
                    }
                    response.parse_output();
                    let item_types: Vec<_> = response
                        .output
                        .iter()
                        .map(|v| {
                            v.get("type")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown")
                                .to_string()
                        })
                        .collect();
                    let parsed_count = response.parsed_output.len();
                    let mut output = response.try_into(history, chat)?;
                    if output.failed_reason.is_none()
                        && output.content.is_empty()
                        && output.tool_calls.is_empty()
                    {
                        return Err(format!("ChatGPT completed without supported output (item types: {item_types:?}, parsed items: {parsed_count})").into());
                    }
                    output.model = Some(id);
                    return Ok(output);
                }
                let status = response.status();
                let request_id = response
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(1)
                    .min(30);
                let error = super::response_error(
                    status.as_u16(),
                    request_id,
                    &super::read_bytes(response, 1024 * 1024).await?,
                );
                if status == reqwest::StatusCode::UNAUTHORIZED && !auth_retried {
                    auth_retried = true;
                    token = service.access(&profile, true).await?.0;
                    continue;
                }
                let retryable = status.is_server_error()
                    || (status == reqwest::StatusCode::TOO_MANY_REQUESTS
                        && error.code != "subscription_sharing_usage_limit_exceeded");
                if retryable && retries < 2 {
                    retries += 1;
                    tokio::select! {_=cancel.cancelled()=>return Err("ChatGPT account was signed out".into()),_=tokio::time::sleep(Duration::from_secs(retry_after*retries))=>{}}
                    continue;
                }
                return Err(model_error(error, retryable));
            }
        })
    }
}
fn model_error(error: ProviderError, retryable: bool) -> BoxError {
    let status =
        reqwest::StatusCode::from_u16(error.status).unwrap_or(reqwest::StatusCode::BAD_GATEWAY);
    Box::new(
        ModelError::new(error.to_string())
            .with_status(status)
            .with_retryable(retryable)
            .with_source(Box::new(error)),
    )
}

fn normalize_item(mut item: wire::MessageItem) -> Result<Option<wire::MessageItem>, BoxError> {
    match &mut item {
        wire::MessageItem::Message { role, content, .. } => {
            if role == "system" {
                *role = "developer".into();
            }
            let content = serde_json::to_value(content)?;
            if content.as_array().is_some_and(|items| {
                items.iter().any(|v| {
                    matches!(
                        v.get("type").and_then(Value::as_str),
                        Some("input_audio" | "input_video")
                    ) || v.get("file_id").is_some_and(|v| !v.is_null())
                })
            }) {
                return Err(
                    "ChatGPT plan does not support audio, video, or uploaded file IDs".into(),
                );
            }
        }
        wire::MessageItem::FunctionCall {
            namespace, call_id, ..
        } => {
            if namespace.as_deref().is_some_and(|v| v != NAMESPACE) {
                return Err("unsupported tool namespace in history".into());
            }
            *namespace = Some(NAMESPACE.into());
            if call_id.is_empty() {
                return Err("tool call history is missing its call ID".into());
            }
        }
        wire::MessageItem::FunctionCallOutput { call_id, .. } => {
            if call_id.is_empty() {
                return Err("tool output history is missing its call ID".into());
            }
        }
        wire::MessageItem::Reasoning {
            encrypted_content: None,
            ..
        }
        | wire::MessageItem::ItemReference { .. } => return Ok(None),
        wire::MessageItem::Reasoning { .. } => {}
        _ => return Err("unsupported ChatGPT plan history item".into()),
    }
    Ok(Some(item))
}
fn push_message(items: &mut Vec<wire::MessageItem>, msg: Message) -> Result<(), BoxError> {
    for part in &msg.content {
        match part {
            ContentPart::InlineData { mime_type, .. }
            | ContentPart::FileData {
                mime_type: Some(mime_type),
                ..
            } if mime_type.starts_with("audio/") || mime_type.starts_with("video/") => {
                return Err("ChatGPT plan does not support audio or video inputs".into());
            }
            _ => {}
        }
    }
    for item in wire::message_into(msg) {
        if let Some(item) = normalize_item(item)? {
            items.push(item);
        }
    }
    Ok(())
}
fn pair_tool_calls(request: &mut CompletionRequest) {
    let mut pending: HashMap<String, VecDeque<String>> = HashMap::new();
    for part in request
        .chat_history
        .iter_mut()
        .flat_map(|m| m.content.iter_mut())
        .chain(request.content.iter_mut())
    {
        match part {
            ContentPart::ToolCall { name, call_id, .. } => {
                let id = call_id.get_or_insert_with(|| format!("call_{}", super::random_id()));
                pending
                    .entry(name.clone())
                    .or_default()
                    .push_back(id.clone());
            }
            ContentPart::ToolOutput { name, call_id, .. } => {
                if let Some(id) = pending.get_mut(name).and_then(VecDeque::pop_front)
                    && call_id.is_none()
                {
                    *call_id = Some(id);
                }
            }
            _ => {}
        }
    }
}
fn build_request(
    slug: &str,
    mut request: CompletionRequest,
) -> Result<(wire::CompletionRequest, Vec<Value>, Vec<Message>), BoxError> {
    pair_tool_calls(&mut request);
    let mut body = wire::CompletionRequest {
        model: slug.into(),
        stream: Some(true),
        instructions: (!request.instructions.is_empty()).then_some(request.instructions),
        additional_parameters: wire::AdditionalParameters {
            store: Some(false),
            include: Some(vec!["reasoning.encrypted_content".into()]),
            ..Default::default()
        },
        ..Default::default()
    };
    for value in request.raw_history {
        if let Ok(item) = wire::MessageItem::deserialize(&value)
            && !matches!(item, wire::MessageItem::Any(_))
        {
            if let Some(item) = normalize_item(item)? {
                body.input.push(item);
            }
        } else {
            push_message(
                &mut body.input,
                serde_json::from_value(value).map_err(|_| "unsupported conversation history")?,
            )?;
        }
    }
    let skip = body.input.len();
    for msg in request.chat_history {
        push_message(&mut body.input, msg)?;
    }
    let now = anda_engine::unix_ms();
    let mut chat = Vec::new();
    if let Some(mut msg) = request
        .documents
        .to_message(&anda_engine::rfc3339_datetime(now).unwrap_or_default())
    {
        msg.timestamp = Some(now);
        chat.push(msg.clone());
        push_message(&mut body.input, msg)?;
    }
    let mut content = request.content;
    if !request.prompt.is_empty() {
        content.insert(0, request.prompt.into());
    }
    if !content.is_empty() {
        let msg = Message {
            role: request.role.unwrap_or_else(|| "user".into()),
            content,
            timestamp: Some(now),
            ..Default::default()
        };
        chat.push(msg.clone());
        push_message(&mut body.input, msg)?;
    }
    if let Some(effort) = request.effort {
        body.additional_parameters.reasoning = Some(wire::Reasoning {
            effort: Some(effort.into()),
            generate_summary: None,
            summary: None,
        });
    }
    if let Some(schema) = request.output_schema {
        body.additional_parameters.text = Some(wire::TextConfig {
            format: Some(wire::TextFormat::JsonSchema(wire::StructuredOutputsInput {
                name: "structured_output".into(),
                schema: anda_core::normalize_strict_schema(schema),
                description: None,
                strict: Some(true),
            })),
            verbosity: None,
        });
    }
    if request.stop.is_some_and(|s| !s.is_empty()) {
        return Err("ChatGPT plan does not support stop sequences".into());
    }
    if !request.tools.is_empty() {
        let tools = request
            .tools
            .into_iter()
            .map(|tool| {
                let tool = tool.normalize_strict_parameters();
                wire::NamespaceToolDefinition::Function {
                    name: tool.name,
                    parameters: Some(tool.parameters),
                    strict: tool.strict,
                    description: Some(tool.description),
                    defer_loading: None,
                }
            })
            .collect();
        body.tools = vec![wire::ToolDefinition::Namespace {
            name: NAMESPACE.into(),
            description: "Anda Bot local tools".into(),
            tools,
        }];
        body.tool_choice = Some(if request.tool_choice_required {
            wire::ToolChoice::required()
        } else {
            wire::ToolChoice::auto()
        });
    }
    let history = body.input[skip..]
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
    Ok((body, history, chat))
}

async fn read_stream(
    response: reqwest::Response,
    cancel: &CancellationToken,
) -> Result<wire::CompletionResponse, BoxError> {
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    if !content_type.starts_with("text/event-stream") {
        // Some gateways buffer the stream or label SSE as JSON. Require a
        // terminal response even on this path; a 200 status alone is not success.
        let bytes = tokio::select! {
            _ = cancel.cancelled() => return Err("ChatGPT account was signed out".into()),
            bytes = super::read_bytes(response, 64 * 1024 * 1024) => bytes?,
        };
        return decode_buffered_response(&bytes, request_id.clone()).map_err(|error| {
            format!(
                "{error} (content-type: {content_type}, request: {})",
                request_id.as_deref().unwrap_or("unknown")
            )
            .into()
        });
    }
    let mut stream = response.bytes_stream();
    let mut decoder = SseDecoder {
        request_id,
        ..Default::default()
    };
    loop {
        let chunk = tokio::select! {_=cancel.cancelled()=>return Err("ChatGPT account was signed out".into()),chunk=tokio::time::timeout(Duration::from_secs(120),stream.next())=>chunk.map_err(|_|"ChatGPT stream timed out")?};
        match chunk {
            Some(chunk) => {
                if let Some(response) = decoder.push(&chunk?)? {
                    return Ok(response);
                }
            }
            None => {
                return Err(
                    "ChatGPT stream ended before response.completed; the request was not replayed"
                        .into(),
                );
            }
        }
    }
}
fn terminal_event(
    event: Value,
    request_id: Option<String>,
    output_items: &mut BTreeMap<u64, Value>,
) -> Result<Option<wire::CompletionResponse>, BoxError> {
    match event.get("type").and_then(Value::as_str) {
        Some("response.output_item.done") => {
            let index = event
                .get("output_index")
                .and_then(Value::as_u64)
                .ok_or("missing output item index")?;
            let item = event.get("item").cloned().ok_or("missing output item")?;
            output_items.insert(index, item);
            Ok(None)
        }
        Some("response.completed") => {
            let mut response: wire::CompletionResponse = serde_json::from_value(
                event
                    .get("response")
                    .cloned()
                    .ok_or("missing completed response")?,
            )?;
            if response.output.is_empty() {
                response.output = std::mem::take(output_items).into_values().collect();
            }
            Ok(Some(response))
        }
        Some("response.failed" | "response.incomplete" | "error") => {
            let error = event.get("response").unwrap_or(&event);
            let code = error
                .pointer("/error/code")
                .or_else(|| error.get("code"))
                .and_then(Value::as_str)
                .unwrap_or("incomplete_response");
            Err(model_error(
                ProviderError {
                    code: code.into(),
                    status: 200,
                    request_id,
                    param: None,
                },
                false,
            ))
        }
        _ => Ok(None),
    }
}
fn decode_buffered_response(
    bytes: &[u8],
    request_id: Option<String>,
) -> Result<wire::CompletionResponse, BoxError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "ChatGPT returned a non-text response")?
        .trim_start_matches('\u{feff}')
        .trim();
    if text.starts_with("data:") || text.starts_with("event:") || text.starts_with(':') {
        let mut decoder = SseDecoder {
            request_id,
            ..Default::default()
        };
        return decoder
            .push(format!("{text}\n\n").as_bytes())?
            .ok_or_else(|| "Buffered stream did not contain response.completed".into());
    }
    let value: Value =
        serde_json::from_str(text).map_err(|_| "ChatGPT returned neither SSE nor JSON")?;
    if value.get("status").and_then(Value::as_str) == Some("completed")
        && value.get("output").is_some_and(Value::is_array)
    {
        return Ok(serde_json::from_value(value)?);
    }
    let events = if let Value::Array(events) = value {
        events
    } else {
        vec![value]
    };
    let mut output_items = BTreeMap::new();
    for event in events {
        if let Some(response) = terminal_event(event, request_id.clone(), &mut output_items)? {
            return Ok(response);
        }
    }
    Err("ChatGPT response did not contain a completed response".into())
}

#[derive(Default)]
struct SseDecoder {
    pending: Vec<u8>,
    data: Vec<u8>,
    total: usize,
    request_id: Option<String>,
    output_items: BTreeMap<u64, Value>,
}
impl SseDecoder {
    fn push(&mut self, chunk: &[u8]) -> Result<Option<wire::CompletionResponse>, BoxError> {
        self.total += chunk.len();
        if self.total > 64 * 1024 * 1024 {
            return Err("ChatGPT stream exceeded 64 MiB".into());
        }
        self.pending.extend_from_slice(chunk);
        while let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<_> = self.pending.drain(..=end).collect();
            let line = line.strip_suffix(b"\n").unwrap_or(&line);
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.is_empty() {
                if self.data.is_empty() {
                    continue;
                }
                let data = std::mem::take(&mut self.data);
                if data == b"[DONE]" {
                    return Err("ChatGPT stream ended without a completed response".into());
                }
                let event: Value =
                    serde_json::from_slice(&data).map_err(|_| "invalid ChatGPT stream event")?;
                if let Some(response) =
                    terminal_event(event, self.request_id.clone(), &mut self.output_items)?
                {
                    return Ok(Some(response));
                }
            } else if let Some(data) = line.strip_prefix(b"data:") {
                if !self.data.is_empty() {
                    self.data.push(b'\n');
                }
                self.data
                    .extend_from_slice(data.strip_prefix(b" ").unwrap_or(data));
            }
        }
        if self.pending.len() > 4 * 1024 * 1024 {
            return Err("ChatGPT stream event is too large".into());
        }
        Ok(None)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn chatgpt_request_uses_plan_contract_and_namespaced_tools() {
        let req = CompletionRequest {
            prompt: "hello".into(),
            temperature: Some(0.5),
            max_output_tokens: Some(42),
            chat_history: vec![Message {
                role: "system".into(),
                content: vec!["rules".to_string().into()],
                ..Default::default()
            }],
            tools: vec![anda_core::FunctionDefinition {
                name: "read_file".into(),
                description: "Read".into(),
                parameters: json!({"type":"object","properties":{}}),
                strict: Some(true),
            }],
            ..Default::default()
        };
        let (body, history, _) = build_request("test-model", req).unwrap();
        let body = serde_json::to_value(body).unwrap();
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert!(body.get("temperature").is_none());
        assert!(body.get("max_output_tokens").is_none());
        assert_eq!(body["tools"][0]["type"], "namespace");
        assert_eq!(body["input"][0]["role"], "developer");
        assert_eq!(history.len(), 2);
    }
    #[test]
    fn chatgpt_accepts_buffered_terminal_responses_but_not_partial_json() {
        let response =
            json!({"id":"r","created_at":1,"model":"m","output":[],"status":"completed"});
        assert_eq!(
            decode_buffered_response(response.to_string().as_bytes(), None)
                .unwrap()
                .id,
            "r"
        );
        let event = json!({"type":"response.completed","response":response});
        assert!(
            decode_buffered_response(json!([event.clone()]).to_string().as_bytes(), None).is_ok()
        );
        assert!(decode_buffered_response(format!("data: {event}\n\n").as_bytes(), None).is_ok());
        assert!(
            decode_buffered_response(
                br#"{"type":"response.created","response":{"id":"r"}}"#,
                None
            )
            .is_err()
        );
        assert!(decode_buffered_response(b"<html>Sign in</html>", None).is_err());
    }

    #[test]
    fn chatgpt_aggregates_output_item_events_when_terminal_output_is_empty() {
        let item = json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"SIWC_OK"}]});
        let events = vec![
            json!({"type":"response.output_item.done","output_index":0,"item":item}),
            json!({"type":"response.completed","response":{"id":"r","created_at":1,"model":"m","output":[],"status":"completed"}}),
        ];
        let bytes = serde_json::to_vec(&events).unwrap();
        let mut response = decode_buffered_response(&bytes, None).unwrap();
        response.parse_output();
        assert_eq!(
            response.try_into(vec![], vec![]).unwrap().content,
            "SIWC_OK"
        );
        let mut decoder = SseDecoder::default();
        let mut completed = None;
        for event in events {
            if let Some(response) = decoder
                .push(format!("data: {event}\n\n").as_bytes())
                .unwrap()
            {
                completed = Some(response);
            }
        }
        let mut response = completed.unwrap();
        response.parse_output();
        assert_eq!(
            response.try_into(vec![], vec![]).unwrap().content,
            "SIWC_OK"
        );
    }

    #[test]
    fn chatgpt_stream_handles_fragmented_completion_and_rejects_false_success() {
        let response = json!({"type":"response.completed","response":{"id":"r","created_at":1,"model":"m","output":[],"status":"completed"}});
        let text = format!("data: {response}\r\n\r\n");
        let mut decoder = SseDecoder::default();
        let mut completed = None;
        for byte in text.as_bytes() {
            if let Some(r) = decoder.push(&[*byte]).unwrap() {
                completed = Some(r);
            }
        }
        assert_eq!(completed.unwrap().id, "r");
        assert!(SseDecoder::default().push(b"data: [DONE]\n\n").is_err());
        assert!(
            SseDecoder::default()
                .push(b"data: {\"type\":\"response.failed\"}\n\n")
                .is_err()
        );
    }
}
