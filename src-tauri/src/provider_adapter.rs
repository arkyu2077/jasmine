//! Local protocol adapter for external model providers.
//!
//! Codex 0.135 only accepts `wire_api = "responses"` for configured model
//! providers. Some OpenAI-compatible routers only expose Chat Completions. This
//! adapter gives Codex a loopback `/v1/responses` endpoint and forwards the
//! request to the real provider's `/v1/chat/completions` endpoint.

use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

const MAX_HEADER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct ProviderAdapterConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: Option<String>,
}

pub struct ProviderAdapterGuard {
    base_url: String,
    task: JoinHandle<()>,
}

impl ProviderAdapterGuard {
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl Drop for ProviderAdapterGuard {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Clone)]
struct AdapterState {
    cfg: Arc<ProviderAdapterConfig>,
    client: reqwest::Client,
}

struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

struct HttpResponse {
    status: u16,
    content_type: String,
    body: Vec<u8>,
}

pub async fn start_adapter(cfg: ProviderAdapterConfig) -> Result<ProviderAdapterGuard, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| format!("bind provider adapter: {e}"))?;
    let addr = listener
        .local_addr()
        .map_err(|e| format!("read provider adapter address: {e}"))?;
    let base_url = format!("http://{addr}/v1");
    let state = AdapterState {
        cfg: Arc::new(cfg),
        client: reqwest::Client::new(),
    };

    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let state = state.clone();
            tokio::spawn(async move {
                if let Err(err) = handle_connection(stream, state).await {
                    tracing::warn!(module = "provider_adapter", error = %err, "provider adapter request failed");
                }
            });
        }
    });

    Ok(ProviderAdapterGuard { base_url, task })
}

async fn handle_connection(mut stream: TcpStream, state: AdapterState) -> Result<(), String> {
    let response = match read_http_request(&mut stream).await {
        Ok(req) => match route_request(req, state).await {
            Ok(response) => response,
            Err(err) => json_response(
                500,
                json!({ "error": { "message": err, "type": "jasmine_adapter_error" } }),
            ),
        },
        Err(err) => json_response(
            400,
            json!({ "error": { "message": err, "type": "jasmine_adapter_bad_request" } }),
        ),
    };
    write_http_response(&mut stream, response).await
}

async fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    let mut buf = Vec::new();
    let mut scratch = [0u8; 4096];
    let header_end = loop {
        if let Some(pos) = find_bytes(&buf, b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > MAX_HEADER_BYTES {
            return Err("request headers are too large".to_string());
        }
        let n = stream
            .read(&mut scratch)
            .await
            .map_err(|e| format!("read request: {e}"))?;
        if n == 0 {
            return Err("request closed before headers completed".to_string());
        }
        buf.extend_from_slice(&scratch[..n]);
    };

    let head = String::from_utf8_lossy(&buf[..header_end]);
    let mut lines = head.split("\r\n");
    let first = lines
        .next()
        .ok_or_else(|| "missing request line".to_string())?;
    let mut request_line = first.split_whitespace();
    let method = request_line
        .next()
        .ok_or_else(|| "missing method".to_string())?
        .to_string();
    let path = request_line
        .next()
        .ok_or_else(|| "missing path".to_string())?
        .to_string();

    let mut headers = HashMap::new();
    for line in lines {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        headers.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
    }

    let content_length = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buf[header_end + 4..].to_vec();
    while body.len() < content_length {
        let n = stream
            .read(&mut scratch)
            .await
            .map_err(|e| format!("read body: {e}"))?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&scratch[..n]);
    }
    body.truncate(content_length);

    Ok(HttpRequest { method, path, body })
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

async fn write_http_response(stream: &mut TcpStream, response: HttpResponse) -> Result<(), String> {
    let status_text = match response.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        _ => "OK",
    };
    let head = format!(
        "HTTP/1.1 {} {}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        response.status,
        status_text,
        response.content_type,
        response.body.len()
    );
    stream
        .write_all(head.as_bytes())
        .await
        .map_err(|e| format!("write response head: {e}"))?;
    stream
        .write_all(&response.body)
        .await
        .map_err(|e| format!("write response body: {e}"))?;
    let _ = stream.shutdown().await;
    Ok(())
}

async fn route_request(req: HttpRequest, state: AdapterState) -> Result<HttpResponse, String> {
    let path = req.path.split('?').next().unwrap_or(&req.path);
    match (req.method.as_str(), path) {
        ("GET", "/v1/models") => forward_models(state).await,
        ("POST", "/v1/responses") => forward_responses(req, state).await,
        _ => Ok(json_response(
            404,
            json!({ "error": { "message": "not found", "type": "jasmine_adapter_not_found" } }),
        )),
    }
}

async fn forward_models(state: AdapterState) -> Result<HttpResponse, String> {
    let url = endpoint_url(&state.cfg.base_url, "models");
    let mut req = state.client.get(url);
    if let Some(api_key) = state.cfg.api_key.as_deref().filter(|v| !v.is_empty()) {
        req = req.bearer_auth(api_key);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("provider /models request failed: {e}"))?;
    relay_response(resp).await
}

async fn forward_responses(req: HttpRequest, state: AdapterState) -> Result<HttpResponse, String> {
    let body: Value = serde_json::from_slice(&req.body)
        .map_err(|e| format!("invalid responses request body: {e}"))?;
    let chat_payload = responses_to_chat_completions(&body, state.cfg.model.as_deref());
    let stream_requested = chat_payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    // Evidence for "the model can't see my reference image": count how many
    // image parts actually made it into the outbound request. 0 here means
    // either the agent never read/attached the image (it stopped at a preamble)
    // or codex sent it in a shape we don't recognise — distinct from "the
    // provider received the image but ignored it".
    let image_parts = count_image_parts(&chat_payload);
    tracing::info!(
        module = "provider_adapter",
        image_parts,
        stream = stream_requested,
        "forwarding /v1/responses → /v1/chat/completions"
    );

    let url = endpoint_url(&state.cfg.base_url, "chat/completions");
    let mut outbound = state
        .client
        .post(url)
        .header("content-type", "application/json")
        .header(
            "accept",
            if stream_requested {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .json(&chat_payload);
    if let Some(api_key) = state.cfg.api_key.as_deref().filter(|v| !v.is_empty()) {
        outbound = outbound.bearer_auth(api_key);
    }

    let resp = outbound
        .send()
        .await
        .map_err(|e| format!("provider chat completions request failed: {e}"))?;
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("read provider response: {e}"))?;

    if !(200..300).contains(&status) {
        return Ok(HttpResponse {
            status,
            content_type,
            body: bytes.to_vec(),
        });
    }

    if stream_requested {
        let raw = String::from_utf8_lossy(&bytes);
        // Don't launder a cut-off upstream stream into a clean completion. If
        // the provider closed the connection mid-turn (no `[DONE]`, no terminal
        // `finish_reason`), synthesizing `response.completed` makes Codex end
        // the turn successfully with only the partial text it received — the
        // user sees the agent stop dead with no error and no recovery. Surface
        // it as an error instead so the turn settles as failed (and the chat's
        // Stop/restart path can recover). See codex.rs turn/completed handling.
        if !chat_stream_terminated_cleanly(&raw) {
            tracing::warn!(
                module = "provider_adapter",
                bytes = bytes.len(),
                "upstream chat completions stream ended without [DONE]/finish_reason — treating as interrupted"
            );
            return Ok(json_response(
                502,
                json!({ "error": {
                    "message": "the model provider interrupted the response before it finished (no completion marker received)",
                    "type": "jasmine_adapter_truncated_stream",
                }}),
            ));
        }
        let model = chat_payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("external")
            .to_string();
        return Ok(HttpResponse {
            status: 200,
            content_type: "text/event-stream".to_string(),
            body: chat_stream_to_responses_sse(&raw, &model).into_bytes(),
        });
    }

    let chat: Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("invalid chat completions response: {e}"))?;
    let model = chat_payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("external");
    Ok(json_response(
        200,
        chat_completion_to_response_json(&chat, model),
    ))
}

async fn relay_response(resp: reqwest::Response) -> Result<HttpResponse, String> {
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();
    let body = resp
        .bytes()
        .await
        .map_err(|e| format!("read provider response: {e}"))?
        .to_vec();
    Ok(HttpResponse {
        status,
        content_type,
        body,
    })
}

fn json_response(status: u16, value: Value) -> HttpResponse {
    HttpResponse {
        status,
        content_type: "application/json".to_string(),
        body: serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec()),
    }
}

fn endpoint_url(base_url: &str, endpoint: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        endpoint.trim_start_matches('/')
    )
}

pub(crate) fn responses_to_chat_completions(
    responses: &Value,
    model_override: Option<&str>,
) -> Value {
    let mut out = Map::new();
    let model = model_override
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .or_else(|| responses.get("model").and_then(Value::as_str))
        .unwrap_or("gpt-5");
    out.insert("model".to_string(), json!(model));

    let mut messages = Vec::new();
    if let Some(instructions) = responses.get("instructions") {
        let text = value_to_text(instructions);
        if !text.trim().is_empty() {
            messages.push(json!({ "role": "system", "content": text }));
        }
    }
    match responses.get("input") {
        Some(Value::Array(items)) => {
            for item in items {
                append_input_item_as_chat_messages(item, &mut messages);
            }
        }
        Some(Value::String(text)) => messages.push(json!({ "role": "user", "content": text })),
        Some(item @ Value::Object(_)) => append_input_item_as_chat_messages(item, &mut messages),
        _ => {}
    }
    if messages.is_empty() {
        messages.push(json!({ "role": "user", "content": "" }));
    }
    out.insert("messages".to_string(), Value::Array(messages));

    out.insert(
        "stream".to_string(),
        responses.get("stream").cloned().unwrap_or(json!(true)),
    );

    if let Some(tools) = responses.get("tools").and_then(Value::as_array) {
        let converted: Vec<Value> = tools
            .iter()
            .filter_map(response_tool_to_chat_tool)
            .collect();
        if !converted.is_empty() {
            out.insert("tools".to_string(), Value::Array(converted));
        }
    }
    if let Some(tool_choice) = responses.get("tool_choice") {
        out.insert("tool_choice".to_string(), chat_tool_choice(tool_choice));
    }

    copy_if_present(responses, &mut out, "temperature");
    copy_if_present(responses, &mut out, "top_p");
    copy_if_present(responses, &mut out, "frequency_penalty");
    copy_if_present(responses, &mut out, "presence_penalty");
    copy_if_present(responses, &mut out, "parallel_tool_calls");
    if let Some(max_tokens) = responses.get("max_output_tokens") {
        out.insert("max_tokens".to_string(), max_tokens.clone());
    }

    // Forward the reasoning request so a reasoning-capable provider actually
    // thinks. Codex sends `reasoning: { effort }` (Responses API); tolerate a
    // bare `reasoning_effort` too. Without this the relay never emits reasoning,
    // so there are no "已思考" blocks to show. See chat_stream_to_responses_sse,
    // which turns the resulting reasoning back into Responses reasoning events.
    if let Some(effort) = responses
        .get("reasoning")
        .and_then(|r| r.get("effort"))
        .and_then(Value::as_str)
        .or_else(|| responses.get("reasoning_effort").and_then(Value::as_str))
    {
        out.insert("reasoning_effort".to_string(), json!(effort));
    }

    Value::Object(out)
}

fn append_input_item_as_chat_messages(item: &Value, messages: &mut Vec<Value>) {
    let Some(obj) = item.as_object() else {
        messages.push(json!({ "role": "user", "content": value_to_text(item) }));
        return;
    };
    match obj.get("type").and_then(Value::as_str) {
        Some("message") => {
            let role = match obj.get("role").and_then(Value::as_str).unwrap_or("user") {
                "assistant" => "assistant",
                "developer" | "system" => "system",
                "tool" => "tool",
                _ => "user",
            };
            messages.push(json!({
                "role": role,
                "content": content_to_chat_content(obj.get("content").unwrap_or(&Value::Null)),
            }));
        }
        Some("function_call") => {
            let call_id = obj
                .get("call_id")
                .or_else(|| obj.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("call");
            let name = obj.get("name").and_then(Value::as_str).unwrap_or("tool");
            let arguments = obj
                .get("arguments")
                .map(value_to_text)
                .unwrap_or_else(|| "{}".to_string());
            messages.push(json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments,
                    }
                }],
            }));
        }
        Some("function_call_output") => {
            let call_id = obj
                .get("call_id")
                .or_else(|| obj.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("call");
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": value_to_text(obj.get("output").unwrap_or(&Value::Null)),
            }));
        }
        _ => {
            messages.push(json!({
                "role": "user",
                "content": value_to_text(item),
            }));
        }
    }
}

fn response_tool_to_chat_tool(tool: &Value) -> Option<Value> {
    let obj = tool.as_object()?;
    if obj.get("type").and_then(Value::as_str) != Some("function") {
        return None;
    }
    let name = obj.get("name")?.as_str()?;
    let mut function = Map::new();
    function.insert("name".to_string(), json!(name));
    if let Some(description) = obj.get("description") {
        function.insert("description".to_string(), description.clone());
    }
    if let Some(parameters) = obj.get("parameters").or_else(|| obj.get("input_schema")) {
        function.insert("parameters".to_string(), parameters.clone());
    }
    if let Some(strict) = obj.get("strict") {
        function.insert("strict".to_string(), strict.clone());
    }
    Some(json!({ "type": "function", "function": Value::Object(function) }))
}

fn chat_tool_choice(value: &Value) -> Value {
    let Some(obj) = value.as_object() else {
        return value.clone();
    };
    if obj.get("type").and_then(Value::as_str) == Some("function") {
        if let Some(name) = obj.get("name") {
            return json!({ "type": "function", "function": { "name": name } });
        }
    }
    value.clone()
}

fn copy_if_present(src: &Value, dest: &mut Map<String, Value>, key: &str) {
    if let Some(value) = src.get(key) {
        dest.insert(key.to_string(), value.clone());
    }
}

fn value_to_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .map(content_part_to_text)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(obj) => {
            if let Some(text) = obj.get("text").and_then(Value::as_str) {
                return text.to_string();
            }
            if let Some(output) = obj.get("output") {
                return value_to_text(output);
            }
            serde_json::to_string(value).unwrap_or_default()
        }
        _ => value.to_string(),
    }
}

fn content_part_to_text(value: &Value) -> String {
    let Some(obj) = value.as_object() else {
        return value_to_text(value);
    };
    if let Some(text) = obj.get("text").and_then(Value::as_str) {
        return text.to_string();
    }
    match obj.get("type").and_then(Value::as_str) {
        Some("input_image") => obj
            .get("image_url")
            .or_else(|| obj.get("url"))
            .and_then(Value::as_str)
            .map(|url| format!("[image: {url}]"))
            .unwrap_or_else(|| "[image]".to_string()),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

/// Pull the image URL out of a Responses `input_image` part. The Responses API
/// puts the URL directly on `image_url` as a string (`{"type":"input_image",
/// "image_url":"data:image/png;base64,…"}`), but tolerate the Chat-Completions
/// nested `{"url": …}` shape and a bare `url` field too.
/// Count `image_url` content parts across all messages of a Chat Completions
/// payload. Diagnostic only — see `forward_responses`.
fn count_image_parts(chat_payload: &Value) -> usize {
    chat_payload
        .get("messages")
        .and_then(Value::as_array)
        .map(|messages| {
            messages
                .iter()
                .filter_map(|m| m.get("content").and_then(Value::as_array))
                .flatten()
                .filter(|part| part.get("type").and_then(Value::as_str) == Some("image_url"))
                .count()
        })
        .unwrap_or(0)
}

fn input_image_url(obj: &Map<String, Value>) -> Option<String> {
    let value = obj.get("image_url").or_else(|| obj.get("url"))?;
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Object(inner) => inner.get("url").and_then(Value::as_str).map(String::from),
        _ => None,
    }
}

/// Build a Chat Completions message `content` from a Responses `content` value,
/// **preserving images**. The old path flattened everything through
/// `value_to_text`, turning an image into the literal text `[image: <url>]` —
/// so a vision-capable provider behind the adapter could never actually see a
/// reference image the agent had read. Chat Completions represents vision as an
/// array of typed parts (`{"type":"image_url","image_url":{"url":…}}`), so when
/// the content carries at least one image we emit that array form; otherwise we
/// keep the plain-string form for maximum provider compatibility.
fn content_to_chat_content(value: &Value) -> Value {
    let Value::Array(parts) = value else {
        return json!(value_to_text(value));
    };
    let mut out = Vec::new();
    let mut has_image = false;
    for part in parts {
        if let Some(obj) = part.as_object() {
            let is_image = matches!(
                obj.get("type").and_then(Value::as_str),
                Some("input_image") | Some("image_url")
            );
            if is_image {
                if let Some(url) = input_image_url(obj) {
                    has_image = true;
                    out.push(json!({ "type": "image_url", "image_url": { "url": url } }));
                    continue;
                }
            }
        }
        let text = content_part_to_text(part);
        if !text.is_empty() {
            out.push(json!({ "type": "text", "text": text }));
        }
    }
    if has_image {
        Value::Array(out)
    } else {
        json!(value_to_text(value))
    }
}

#[derive(Default)]
struct ToolCallState {
    output_index: Option<usize>,
    item_id: String,
    call_id: String,
    name: String,
    arguments: String,
    emitted_arguments_len: usize,
    emitted: bool,
    done: bool,
}

const THINK_OPEN: &str = "<think>";
const THINK_CLOSE: &str = "</think>";

/// Streaming splitter that separates inline `<think>…</think>` reasoning from
/// visible answer text. Relays like modelsrouter's gpt-5.5 have no structured
/// reasoning field — when asked to reason they inline it as `<think>…</think>`
/// in `content`. Native Codex shows reasoning as its own block, so to match
/// that we pull the think segments out here and emit them as Responses
/// reasoning events. Tag-aware across chunk boundaries: a tag split between two
/// SSE chunks (`…<thi` then `nk>…`) is held in `carry` until it completes.
#[derive(Default)]
struct ThinkSplitter {
    in_think: bool,
    carry: String,
}

/// Length of the longest suffix of `buf` that is a (partial) prefix of `needle`
/// — i.e. a tag that might still complete on the next chunk. Char-boundary safe
/// so multibyte (e.g. Chinese) content isn't split mid-codepoint.
fn partial_tag_suffix_len(buf: &str, needle: &str) -> usize {
    let max = needle.len().saturating_sub(1);
    let start = buf.len().saturating_sub(max);
    for i in start..buf.len() {
        if !buf.is_char_boundary(i) {
            continue;
        }
        let suffix = &buf[i..];
        if needle.starts_with(suffix) {
            return buf.len() - i;
        }
    }
    0
}

impl ThinkSplitter {
    /// Feed a content delta; returns `(visible, reasoning)` extracted from it.
    fn push(&mut self, chunk: &str) -> (String, String) {
        let mut buf = std::mem::take(&mut self.carry);
        buf.push_str(chunk);
        let (mut visible, mut reasoning) = (String::new(), String::new());
        loop {
            let needle = if self.in_think {
                THINK_CLOSE
            } else {
                THINK_OPEN
            };
            if let Some(pos) = buf.find(needle) {
                let before = &buf[..pos];
                if self.in_think {
                    reasoning.push_str(before);
                } else {
                    visible.push_str(before);
                }
                self.in_think = !self.in_think;
                buf = buf[pos + needle.len()..].to_string();
            } else {
                let keep = partial_tag_suffix_len(&buf, needle);
                let emit_to = buf.len() - keep;
                let emit = &buf[..emit_to];
                if self.in_think {
                    reasoning.push_str(emit);
                } else {
                    visible.push_str(emit);
                }
                self.carry = buf[emit_to..].to_string();
                break;
            }
        }
        (visible, reasoning)
    }

    /// Flush any held tag fragment at end of stream.
    fn flush(&mut self) -> (String, String) {
        let rest = std::mem::take(&mut self.carry);
        if self.in_think {
            (String::new(), rest)
        } else {
            (rest, String::new())
        }
    }
}

/// Open a Responses reasoning output item once (idempotent), so Codex renders a
/// "已思考" block. Mirrors `ensure_text_item` for the reasoning channel.
fn ensure_reasoning_item(
    events: &mut String,
    seq: &mut u64,
    item_id: &mut Option<String>,
    output_index: &mut Option<usize>,
    next_output_index: &mut usize,
) {
    if item_id.is_some() {
        return;
    }
    let id = format!("rs_{}", nanoid::nanoid!(12));
    let oi = *next_output_index;
    *next_output_index += 1;
    *item_id = Some(id.clone());
    *output_index = Some(oi);
    push_response_event(
        events,
        seq,
        "response.output_item.added",
        json!({ "output_index": oi, "item": { "id": id, "type": "reasoning", "summary": [] } }),
    );
    push_response_event(
        events,
        seq,
        "response.reasoning_summary_part.added",
        json!({
            "item_id": id,
            "output_index": oi,
            "summary_index": 0,
            "part": { "type": "summary_text", "text": "" },
        }),
    );
}

#[allow(clippy::too_many_arguments)]
fn push_text_delta(
    events: &mut String,
    seq: &mut u64,
    seg: &str,
    item_id: &mut Option<String>,
    output_index: &mut Option<usize>,
    next_output_index: &mut usize,
    accum: &mut String,
) {
    if seg.is_empty() {
        return;
    }
    ensure_text_item(events, seq, item_id, output_index, next_output_index);
    let id = item_id.as_deref().unwrap_or_default();
    let oi = output_index.unwrap_or(0);
    push_response_event(
        events,
        seq,
        "response.output_text.delta",
        json!({ "item_id": id, "output_index": oi, "content_index": 0, "delta": seg }),
    );
    accum.push_str(seg);
}

#[allow(clippy::too_many_arguments)]
fn push_reasoning_delta(
    events: &mut String,
    seq: &mut u64,
    seg: &str,
    item_id: &mut Option<String>,
    output_index: &mut Option<usize>,
    next_output_index: &mut usize,
    accum: &mut String,
) {
    if seg.is_empty() {
        return;
    }
    ensure_reasoning_item(events, seq, item_id, output_index, next_output_index);
    let id = item_id.as_deref().unwrap_or_default();
    let oi = output_index.unwrap_or(0);
    push_response_event(
        events,
        seq,
        "response.reasoning_summary_text.delta",
        json!({ "item_id": id, "output_index": oi, "summary_index": 0, "delta": seg }),
    );
    accum.push_str(seg);
}

fn finish_reasoning_item(
    events: &mut String,
    seq: &mut u64,
    item_id: &Option<String>,
    output_index: Option<usize>,
    text: &str,
) -> Option<Value> {
    let (id, oi) = (item_id.as_deref()?, output_index?);
    push_response_event(
        events,
        seq,
        "response.reasoning_summary_text.done",
        json!({ "item_id": id, "output_index": oi, "summary_index": 0, "text": text }),
    );
    let item = json!({
        "id": id,
        "type": "reasoning",
        "summary": [{ "type": "summary_text", "text": text }],
    });
    push_response_event(
        events,
        seq,
        "response.output_item.done",
        json!({ "output_index": oi, "item": item }),
    );
    Some(item)
}

pub(crate) fn chat_stream_to_responses_sse(raw: &str, model: &str) -> String {
    let trimmed = raw.trim_start();
    if trimmed.starts_with('{') {
        if let Ok(chat) = serde_json::from_str::<Value>(trimmed) {
            return chat_completion_to_response_sse(&chat, model);
        }
    }

    let response_id = format!("resp_{}", nanoid::nanoid!(12));
    let created_at = unix_seconds();
    let mut seq = 0u64;
    let mut events = String::new();
    let mut output_items: Vec<Value> = Vec::new();
    let mut next_output_index = 0usize;
    let mut text_item_id: Option<String> = None;
    let mut text_output_index: Option<usize> = None;
    let mut text = String::new();
    let mut tool_calls: Vec<ToolCallState> = Vec::new();
    // Reasoning channel: inline <think>…</think> in content, or a provider's
    // native `reasoning_content`/`reasoning` delta field, surfaced as Responses
    // reasoning events so Codex shows a "已思考" block.
    let mut splitter = ThinkSplitter::default();
    let mut reasoning_item_id: Option<String> = None;
    let mut reasoning_output_index: Option<usize> = None;
    let mut reasoning_text = String::new();

    push_response_event(
        &mut events,
        &mut seq,
        "response.created",
        json!({
            "response": response_object(&response_id, created_at, "in_progress", model, Vec::new()),
        }),
    );

    for event in parse_sse_events(raw) {
        let data = event.data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let Ok(chunk) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        for choice in chunk
            .get("choices")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(delta) = choice.get("delta") {
                // Native reasoning fields some relays use (DeepSeek-R1 style).
                // These are already reasoning, no <think> parsing needed.
                if let Some(r) = delta
                    .get("reasoning_content")
                    .or_else(|| delta.get("reasoning"))
                    .or_else(|| delta.get("reasoning_text"))
                    .and_then(Value::as_str)
                {
                    push_reasoning_delta(
                        &mut events,
                        &mut seq,
                        r,
                        &mut reasoning_item_id,
                        &mut reasoning_output_index,
                        &mut next_output_index,
                        &mut reasoning_text,
                    );
                }
                if let Some(content) = delta.get("content") {
                    let delta_text = chat_delta_content_to_text(content);
                    if !delta_text.is_empty() {
                        // Split inline <think>…</think> out of the visible text.
                        let (visible, reasoning_seg) = splitter.push(&delta_text);
                        push_reasoning_delta(
                            &mut events,
                            &mut seq,
                            &reasoning_seg,
                            &mut reasoning_item_id,
                            &mut reasoning_output_index,
                            &mut next_output_index,
                            &mut reasoning_text,
                        );
                        push_text_delta(
                            &mut events,
                            &mut seq,
                            &visible,
                            &mut text_item_id,
                            &mut text_output_index,
                            &mut next_output_index,
                            &mut text,
                        );
                    }
                }
                if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                    for call_delta in calls {
                        apply_tool_call_delta(
                            call_delta,
                            &mut tool_calls,
                            &mut events,
                            &mut seq,
                            &mut next_output_index,
                        );
                    }
                }
            }
        }
    }

    // End of stream: flush any held tag fragment, then close the reasoning item
    // (before the message, so output order is reasoning → text → tools).
    let (flush_visible, flush_reasoning) = splitter.flush();
    push_reasoning_delta(
        &mut events,
        &mut seq,
        &flush_reasoning,
        &mut reasoning_item_id,
        &mut reasoning_output_index,
        &mut next_output_index,
        &mut reasoning_text,
    );
    push_text_delta(
        &mut events,
        &mut seq,
        &flush_visible,
        &mut text_item_id,
        &mut text_output_index,
        &mut next_output_index,
        &mut text,
    );
    if let Some(item) = finish_reasoning_item(
        &mut events,
        &mut seq,
        &reasoning_item_id,
        reasoning_output_index,
        &reasoning_text,
    ) {
        output_items.push(item);
    }

    if let (Some(item_id), Some(output_index)) = (text_item_id.as_deref(), text_output_index) {
        let item = message_output_item(item_id, &text, "completed");
        push_response_event(
            &mut events,
            &mut seq,
            "response.output_text.done",
            json!({
                "item_id": item_id,
                "output_index": output_index,
                "content_index": 0,
                "text": text,
            }),
        );
        push_response_event(
            &mut events,
            &mut seq,
            "response.content_part.done",
            json!({
                "item_id": item_id,
                "output_index": output_index,
                "content_index": 0,
                "part": { "type": "output_text", "text": text, "annotations": [] },
            }),
        );
        push_response_event(
            &mut events,
            &mut seq,
            "response.output_item.done",
            json!({ "output_index": output_index, "item": item }),
        );
        output_items.push(item);
    }

    for call in &mut tool_calls {
        finish_tool_call(call, &mut events, &mut seq, &mut next_output_index);
        output_items.push(function_call_output_item(call));
    }

    push_response_event(
        &mut events,
        &mut seq,
        "response.completed",
        json!({
            "response": response_object(&response_id, created_at, "completed", model, output_items),
        }),
    );

    events
}

fn chat_completion_to_response_sse(chat: &Value, model: &str) -> String {
    let response = chat_completion_to_response_json(chat, model);
    let response_id = response
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("resp_external");
    let created_at = response
        .get("created_at")
        .and_then(Value::as_u64)
        .unwrap_or_else(unix_seconds);
    let output = response
        .get("output")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut seq = 0u64;
    let mut events = String::new();
    push_response_event(
        &mut events,
        &mut seq,
        "response.created",
        json!({ "response": response_object(response_id, created_at, "in_progress", model, Vec::new()) }),
    );
    for (output_index, item) in output.iter().enumerate() {
        push_response_event(
            &mut events,
            &mut seq,
            "response.output_item.added",
            json!({ "output_index": output_index, "item": started_item(item) }),
        );
        if item.get("type").and_then(Value::as_str) == Some("message") {
            let text = item
                .get("content")
                .and_then(Value::as_array)
                .and_then(|parts| parts.first())
                .and_then(|part| part.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let item_id = item.get("id").and_then(Value::as_str).unwrap_or("msg");
            push_response_event(
                &mut events,
                &mut seq,
                "response.content_part.added",
                json!({
                    "item_id": item_id,
                    "output_index": output_index,
                    "content_index": 0,
                    "part": { "type": "output_text", "text": "", "annotations": [] },
                }),
            );
            if !text.is_empty() {
                push_response_event(
                    &mut events,
                    &mut seq,
                    "response.output_text.delta",
                    json!({
                        "item_id": item_id,
                        "output_index": output_index,
                        "content_index": 0,
                        "delta": text,
                    }),
                );
            }
            push_response_event(
                &mut events,
                &mut seq,
                "response.output_text.done",
                json!({
                    "item_id": item_id,
                    "output_index": output_index,
                    "content_index": 0,
                    "text": text,
                }),
            );
            push_response_event(
                &mut events,
                &mut seq,
                "response.content_part.done",
                json!({
                    "item_id": item_id,
                    "output_index": output_index,
                    "content_index": 0,
                    "part": { "type": "output_text", "text": text, "annotations": [] },
                }),
            );
        }
        push_response_event(
            &mut events,
            &mut seq,
            "response.output_item.done",
            json!({ "output_index": output_index, "item": item }),
        );
    }
    push_response_event(
        &mut events,
        &mut seq,
        "response.completed",
        json!({ "response": response }),
    );
    events
}

pub(crate) fn chat_completion_to_response_json(chat: &Value, model: &str) -> Value {
    let response_id = format!("resp_{}", nanoid::nanoid!(12));
    let created_at = unix_seconds();
    let mut output = Vec::new();
    if let Some(message) = chat
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
    {
        if let Some(content) = message.get("content") {
            let text = chat_delta_content_to_text(content);
            if !text.is_empty() {
                output.push(message_output_item(
                    &format!("msg_{}", nanoid::nanoid!(12)),
                    &text,
                    "completed",
                ));
            }
        }
        if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
            for tool in tool_calls {
                let mut state = ToolCallState {
                    output_index: Some(output.len()),
                    item_id: format!("fc_{}", nanoid::nanoid!(12)),
                    call_id: tool
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("call")
                        .to_string(),
                    name: tool
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or("tool")
                        .to_string(),
                    arguments: tool
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .map(value_to_text)
                        .unwrap_or_else(|| "{}".to_string()),
                    emitted_arguments_len: 0,
                    emitted: true,
                    done: true,
                };
                output.push(function_call_output_item(&mut state));
            }
        }
    }
    response_object(&response_id, created_at, "completed", model, output)
}

fn ensure_text_item(
    events: &mut String,
    seq: &mut u64,
    text_item_id: &mut Option<String>,
    text_output_index: &mut Option<usize>,
    next_output_index: &mut usize,
) {
    if text_item_id.is_some() {
        return;
    }
    let item_id = format!("msg_{}", nanoid::nanoid!(12));
    let output_index = *next_output_index;
    *next_output_index += 1;
    *text_item_id = Some(item_id.clone());
    *text_output_index = Some(output_index);
    push_response_event(
        events,
        seq,
        "response.output_item.added",
        json!({
            "output_index": output_index,
            "item": message_output_item(&item_id, "", "in_progress"),
        }),
    );
    push_response_event(
        events,
        seq,
        "response.content_part.added",
        json!({
            "item_id": item_id,
            "output_index": output_index,
            "content_index": 0,
            "part": { "type": "output_text", "text": "", "annotations": [] },
        }),
    );
}

fn apply_tool_call_delta(
    delta: &Value,
    calls: &mut Vec<ToolCallState>,
    events: &mut String,
    seq: &mut u64,
    next_output_index: &mut usize,
) {
    let index = delta.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
    while calls.len() <= index {
        calls.push(ToolCallState::default());
    }
    let call = &mut calls[index];
    if let Some(id) = delta.get("id").and_then(Value::as_str) {
        call.call_id = id.to_string();
    }
    if call.call_id.is_empty() {
        call.call_id = format!("call_{}", nanoid::nanoid!(12));
    }
    if call.item_id.is_empty() {
        call.item_id = format!("fc_{}", nanoid::nanoid!(12));
    }
    if let Some(function) = delta.get("function") {
        if let Some(name) = function.get("name").and_then(Value::as_str) {
            call.name.push_str(name);
        }
        if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
            call.arguments.push_str(arguments);
        }
    }
    maybe_emit_tool_call(call, events, seq, next_output_index);
}

fn maybe_emit_tool_call(
    call: &mut ToolCallState,
    events: &mut String,
    seq: &mut u64,
    next_output_index: &mut usize,
) {
    if !call.emitted {
        if call.name.is_empty() {
            return;
        }
        let output_index = *next_output_index;
        *next_output_index += 1;
        call.output_index = Some(output_index);
        call.emitted = true;
        push_response_event(
            events,
            seq,
            "response.output_item.added",
            json!({
                "output_index": output_index,
                "item": {
                    "id": call.item_id,
                    "type": "function_call",
                    "call_id": call.call_id,
                    "name": call.name,
                    "arguments": "",
                    "status": "in_progress",
                },
            }),
        );
    }

    let output_index = call.output_index.unwrap_or(0);
    if call.arguments.len() > call.emitted_arguments_len {
        let delta = &call.arguments[call.emitted_arguments_len..];
        call.emitted_arguments_len = call.arguments.len();
        push_response_event(
            events,
            seq,
            "response.function_call_arguments.delta",
            json!({
                "item_id": call.item_id,
                "output_index": output_index,
                "delta": delta,
            }),
        );
    }
}

fn finish_tool_call(
    call: &mut ToolCallState,
    events: &mut String,
    seq: &mut u64,
    next_output_index: &mut usize,
) {
    if call.done {
        return;
    }
    if call.name.is_empty() {
        call.name = "tool".to_string();
    }
    maybe_emit_tool_call(call, events, seq, next_output_index);
    let output_index = call.output_index.unwrap_or(0);
    push_response_event(
        events,
        seq,
        "response.function_call_arguments.done",
        json!({
            "item_id": call.item_id,
            "output_index": output_index,
            "arguments": call.arguments,
        }),
    );
    push_response_event(
        events,
        seq,
        "response.output_item.done",
        json!({
            "output_index": output_index,
            "item": function_call_output_item(call),
        }),
    );
    call.done = true;
}

fn function_call_output_item(call: &ToolCallState) -> Value {
    json!({
        "id": call.item_id,
        "type": "function_call",
        "call_id": call.call_id,
        "name": call.name,
        "arguments": call.arguments,
        "status": "completed",
    })
}

fn message_output_item(item_id: &str, text: &str, status: &str) -> Value {
    json!({
        "id": item_id,
        "type": "message",
        "role": "assistant",
        "status": status,
        "content": [{
            "type": "output_text",
            "text": text,
            "annotations": [],
        }],
    })
}

fn started_item(item: &Value) -> Value {
    let mut item = item.clone();
    if let Some(obj) = item.as_object_mut() {
        obj.insert("status".to_string(), json!("in_progress"));
        if obj.get("type").and_then(Value::as_str) == Some("message") {
            obj.insert("content".to_string(), json!([]));
        }
    }
    item
}

fn response_object(
    response_id: &str,
    created_at: u64,
    status: &str,
    model: &str,
    output: Vec<Value>,
) -> Value {
    json!({
        "id": response_id,
        "object": "response",
        "created_at": created_at,
        "status": status,
        "model": model,
        "output": output,
    })
}

fn push_response_event(events: &mut String, seq: &mut u64, event: &str, mut data: Value) {
    if let Some(obj) = data.as_object_mut() {
        obj.insert("type".to_string(), json!(event));
        obj.insert("sequence_number".to_string(), json!(*seq));
    }
    *seq += 1;
    events.push_str("event: ");
    events.push_str(event);
    events.push('\n');
    events.push_str("data: ");
    events.push_str(&serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_string()));
    events.push_str("\n\n");
}

/// True if the upstream Chat Completions response actually finished, vs. got
/// cut off mid-stream. A clean finish is one of:
///   - a non-streamed body (a single JSON `chat.completion` object), or
///   - an SSE stream carrying the `[DONE]` sentinel, or
///   - at least one chunk with a non-null `finish_reason`.
/// An SSE stream that produced chunks but none of those markers was almost
/// certainly interrupted (provider/network drop); we must NOT present it as a
/// successful completion. An empty body is likewise not a valid completion.
fn chat_stream_terminated_cleanly(raw: &str) -> bool {
    let trimmed = raw.trim_start();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with('{') {
        // Non-streamed: the whole response object arrived in one shot.
        return true;
    }
    for event in parse_sse_events(raw) {
        let data = event.data.trim();
        if data == "[DONE]" {
            return true;
        }
        let Ok(chunk) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        let has_finish = chunk
            .get("choices")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|c| {
                c.get("finish_reason")
                    .map(|v| !v.is_null())
                    .unwrap_or(false)
            });
        if has_finish {
            return true;
        }
    }
    false
}

struct SseEvent {
    data: String,
}

fn parse_sse_events(raw: &str) -> Vec<SseEvent> {
    let mut events = Vec::new();
    let mut data_lines = Vec::new();
    for line in raw.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            if !data_lines.is_empty() {
                events.push(SseEvent {
                    data: data_lines.join("\n"),
                });
                data_lines.clear();
            }
            continue;
        }
        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.trim_start().to_string());
        }
    }
    if !data_lines.is_empty() {
        events.push(SseEvent {
            data: data_lines.join("\n"),
        });
    }
    events
}

fn chat_delta_content_to_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .map(content_part_to_text)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        _ => value_to_text(value),
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_payload_maps_messages_and_tools_to_chat() {
        let responses = json!({
            "model": "gpt-5.5",
            "instructions": "Be concise.",
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "Hello" }]
                }
            ],
            "tools": [{
                "type": "function",
                "name": "exec_command",
                "description": "Run command",
                "parameters": { "type": "object" }
            }],
            "stream": true,
            "tool_choice": "auto"
        });

        let chat = responses_to_chat_completions(&responses, None);

        assert_eq!(chat["model"], "gpt-5.5");
        assert_eq!(chat["messages"][0]["role"], "system");
        assert_eq!(chat["messages"][1]["role"], "user");
        assert_eq!(chat["messages"][1]["content"], "Hello");
        assert_eq!(chat["tools"][0]["function"]["name"], "exec_command");
        assert_eq!(chat["tool_choice"], "auto");
    }

    #[test]
    fn model_override_wins_over_responses_model() {
        let responses = json!({
            "model": "ignored",
            "input": "Hello"
        });

        let chat = responses_to_chat_completions(&responses, Some("gpt-5.5"));

        assert_eq!(chat["model"], "gpt-5.5");
    }

    #[test]
    fn chat_stream_maps_text_to_responses_events() {
        let raw = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"he\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"llo\"}}]}\n\n",
            "data: [DONE]\n\n"
        );

        let sse = chat_stream_to_responses_sse(raw, "gpt-5.5");

        assert!(sse.contains("event: response.output_text.delta"));
        assert!(sse.contains("\"delta\":\"he\""));
        assert!(sse.contains("\"text\":\"hello\""));
        assert!(sse.contains("event: response.completed"));
    }

    #[test]
    fn chat_stream_maps_tool_calls_to_responses_events() {
        let raw = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"exec_command\",\"arguments\":\"{\\\"cmd\\\":\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"pwd\\\"}\"}}]}}]}\n\n",
            "data: [DONE]\n\n"
        );

        let sse = chat_stream_to_responses_sse(raw, "gpt-5.5");

        assert!(sse.contains("event: response.output_item.added"));
        assert!(sse.contains("event: response.function_call_arguments.delta"));
        assert!(sse.contains("event: response.function_call_arguments.done"));
        assert!(sse.contains("\"name\":\"exec_command\""));
        assert!(sse.contains("{\\\"cmd\\\":\\\"pwd\\\"}"));
    }

    #[test]
    fn input_image_is_preserved_as_chat_image_url() {
        let responses = json!({
            "model": "gpt-5.5",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "what is this?" },
                    { "type": "input_image", "image_url": "data:image/png;base64,AAAA" }
                ]
            }]
        });

        let chat = responses_to_chat_completions(&responses, None);
        let content = &chat["messages"][0]["content"];

        // Must be the array (vision) form, NOT a flattened "[image: …]" string.
        assert!(content.is_array(), "content should be an array of parts");
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "what is this?");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(content[1]["image_url"]["url"], "data:image/png;base64,AAAA");
        assert_eq!(count_image_parts(&chat), 1);
    }

    #[test]
    fn text_only_content_stays_a_plain_string() {
        let responses = json!({
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "hello" }]
            }]
        });
        let chat = responses_to_chat_completions(&responses, None);
        // No image → keep the simple string form for max provider compatibility.
        assert_eq!(chat["messages"][0]["content"], "hello");
        assert_eq!(count_image_parts(&chat), 0);
    }

    /// Build a Chat Completions SSE stream from a list of `content` deltas.
    fn content_sse(chunks: &[&str]) -> String {
        let mut s = String::new();
        for c in chunks {
            s.push_str(&format!(
                "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}}}}]}}\n\n",
                serde_json::to_string(c).unwrap()
            ));
        }
        s.push_str("data: [DONE]\n\n");
        s
    }

    #[test]
    fn inline_think_becomes_reasoning_events() {
        let raw = content_sse(&["<think>secret reasoning</think>", "visible answer"]);
        let out = chat_stream_to_responses_sse(&raw, "m");
        // Reasoning surfaced as a Responses reasoning item + summary deltas.
        assert!(out.contains("\"type\":\"reasoning\""));
        assert!(out.contains("response.reasoning_summary_text.delta"));
        assert!(out.contains("\"text\":\"secret reasoning\"")); // reasoning .done
                                                                // The think text must NOT leak into the visible message.
        assert!(out.contains("response.output_text.delta"));
        assert!(out.contains("\"text\":\"visible answer\"")); // message .done
        assert!(!out.contains("\"text\":\"<think>secret reasoning</think>visible answer\""));
    }

    #[test]
    fn think_tags_split_across_chunks_are_handled() {
        // <think> and </think> are each split across two SSE chunks.
        let raw = content_sse(&["<thi", "nk>hidden</thi", "nk>shown"]);
        let out = chat_stream_to_responses_sse(&raw, "m");
        assert!(out.contains("\"text\":\"hidden\"")); // reasoning .done
        assert!(out.contains("\"text\":\"shown\"")); // message .done
    }

    #[test]
    fn native_reasoning_content_field_becomes_reasoning() {
        let raw = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking hard\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"the answer\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let out = chat_stream_to_responses_sse(raw, "m");
        assert!(out.contains("response.reasoning_summary_text.delta"));
        assert!(out.contains("\"text\":\"thinking hard\""));
        assert!(out.contains("\"text\":\"the answer\""));
    }

    #[test]
    fn plain_content_emits_no_reasoning_item() {
        let raw = content_sse(&["just", " text"]);
        let out = chat_stream_to_responses_sse(&raw, "m");
        assert!(!out.contains("\"type\":\"reasoning\""));
        assert!(out.contains("\"text\":\"just text\""));
    }

    #[test]
    fn reasoning_effort_is_forwarded() {
        let nested = responses_to_chat_completions(
            &json!({ "model": "m", "input": "hi", "reasoning": { "effort": "medium" } }),
            None,
        );
        assert_eq!(nested["reasoning_effort"], "medium");
        let bare = responses_to_chat_completions(
            &json!({ "model": "m", "input": "hi", "reasoning_effort": "high" }),
            None,
        );
        assert_eq!(bare["reasoning_effort"], "high");
    }

    #[test]
    fn clean_stream_with_done_is_complete() {
        let raw = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        assert!(chat_stream_terminated_cleanly(raw));
    }

    #[test]
    fn clean_stream_with_finish_reason_is_complete() {
        let raw =
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\n";
        assert!(chat_stream_terminated_cleanly(raw));
    }

    #[test]
    fn non_streamed_json_object_is_complete() {
        let raw = "{\"choices\":[{\"message\":{\"content\":\"hi\"}}]}";
        assert!(chat_stream_terminated_cleanly(raw));
    }

    #[test]
    fn truncated_stream_without_terminator_is_incomplete() {
        // Chunks arrived but the provider dropped the connection before any
        // `[DONE]` or terminal `finish_reason` — this is the interruption case.
        let raw = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"我先看一下\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"参考图\"}}]}\n\n"
        );
        assert!(!chat_stream_terminated_cleanly(raw));
    }

    #[test]
    fn empty_body_is_incomplete() {
        assert!(!chat_stream_terminated_cleanly(""));
        assert!(!chat_stream_terminated_cleanly("   \n"));
    }

    #[tokio::test]
    async fn adapter_returns_error_on_truncated_upstream_stream() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let provider_addr = listener.local_addr().unwrap();
        let provider_base_url = format!("http://{provider_addr}/v1");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = read_http_request(&mut stream).await.unwrap();
            // Stream a chunk but never send `[DONE]` / finish_reason.
            let body = "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n";
            write_http_response(
                &mut stream,
                HttpResponse {
                    status: 200,
                    content_type: "text/event-stream".to_string(),
                    body: body.as_bytes().to_vec(),
                },
            )
            .await
            .unwrap();
        });

        let adapter = start_adapter(ProviderAdapterConfig {
            base_url: provider_base_url,
            api_key: None,
            model: Some("gpt-5.5".to_string()),
        })
        .await
        .unwrap();
        let resp = reqwest::Client::new()
            .post(format!("{}/responses", adapter.base_url()))
            .json(&json!({ "model": "x", "input": "hi", "stream": true }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 502);
        let body = resp.text().await.unwrap();
        assert!(body.contains("jasmine_adapter_truncated_stream"));
    }

    #[tokio::test]
    async fn adapter_forwards_responses_to_chat_completions() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let provider_addr = listener.local_addr().unwrap();
        let provider_base_url = format!("http://{provider_addr}/v1");
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let req = read_http_request(&mut stream).await.unwrap();
            tx.send((req.method, req.path, req.body)).unwrap();
            let body = concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n",
                "data: [DONE]\n\n"
            );
            write_http_response(
                &mut stream,
                HttpResponse {
                    status: 200,
                    content_type: "text/event-stream".to_string(),
                    body: body.as_bytes().to_vec(),
                },
            )
            .await
            .unwrap();
        });

        let adapter = start_adapter(ProviderAdapterConfig {
            base_url: provider_base_url,
            api_key: Some("sk-test".to_string()),
            model: Some("gpt-5.5".to_string()),
        })
        .await
        .unwrap();
        let resp = reqwest::Client::new()
            .post(format!("{}/responses", adapter.base_url()))
            .json(&json!({
                "model": "ignored",
                "input": "Hello",
                "stream": true,
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);
        let text = resp.text().await.unwrap();
        assert!(text.contains("event: response.output_text.delta"));
        assert!(text.contains("\"delta\":\"hello\""));

        let (method, path, body) = rx.await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(method, "POST");
        assert_eq!(path, "/v1/chat/completions");
        assert_eq!(body["model"], "gpt-5.5");
        assert_eq!(body["messages"][0]["content"], "Hello");
        assert_eq!(body["stream"], true);
    }
}
