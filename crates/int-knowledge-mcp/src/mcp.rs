//! A minimal Model Context Protocol server over stdio.
//!
//! MCP on stdio is newline-delimited JSON-RPC 2.0: requests arrive on stdin,
//! responses go out on stdout, and **nothing else may ever touch stdout** or the
//! client's parser desynchronises. All diagnostics go to stderr.
//!
//! This implements the handful of methods a tools-only server needs
//! (`initialize`, `tools/list`, `tools/call`, `ping`) rather than depending on a
//! larger framework, which keeps the module self-contained enough to drop into
//! any Intentio app that wants to expose tools.

use std::io::{BufRead, BufReader, Write};

use serde_json::{json, Value};

/// Protocol revisions this server knows how to speak.
const SUPPORTED_PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
const DEFAULT_PROTOCOL: &str = "2025-06-18";

#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    /// Shown to the model as guidance on how to use this server.
    pub instructions: String,
}

#[derive(Debug, Clone)]
pub struct Tool {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool's arguments.
    pub input_schema: Value,
}

impl Tool {
    pub fn new(name: &str, description: &str, input_schema: Value) -> Self {
        Tool { name: name.to_string(), description: description.to_string(), input_schema }
    }
}

/// The result of a successful tool call.
#[derive(Debug)]
pub struct ToolOutput {
    /// Human/model-readable text. Structured data should be JSON here.
    pub text: String,
}

impl ToolOutput {
    /// Part of the transport's API surface; not every server returns plain text.
    #[allow(dead_code)]
    pub fn text(text: impl Into<String>) -> Self {
        ToolOutput { text: text.into() }
    }

    /// Render a value as pretty JSON — the form models parse most reliably.
    pub fn json(value: &Value) -> Self {
        ToolOutput {
            text: serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
        }
    }
}

pub trait ToolProvider {
    fn server_info(&self) -> ServerInfo;
    fn tools(&self) -> Vec<Tool>;
    /// Run a tool. `Err` becomes an MCP tool error, which the model can see and
    /// recover from — reserve it for bad input and failed operations.
    fn call(&mut self, name: &str, arguments: &Value) -> Result<ToolOutput, String>;
}

/// Read requests from stdin and serve them until the stream closes.
pub fn serve(mut provider: impl ToolProvider) -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return Ok(()), // client closed the pipe
            Ok(_) => {}
            Err(err) => {
                eprintln!("[mcp] stdin read failed: {err}");
                return Err(err);
            }
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(err) => {
                eprintln!("[mcp] malformed request: {err}");
                write_message(&mut out, &error_response(Value::Null, -32700, "parse error"))?;
                continue;
            }
        };

        // Batches are legal JSON-RPC; handle them so clients that use them work.
        let responses = match &request {
            Value::Array(items) => items.iter().filter_map(|item| handle(&mut provider, item)).collect(),
            _ => handle(&mut provider, &request).into_iter().collect::<Vec<_>>(),
        };
        for response in responses {
            write_message(&mut out, &response)?;
        }
    }
}

fn write_message(out: &mut impl Write, message: &Value) -> std::io::Result<()> {
    writeln!(out, "{message}")?;
    // Clients block on a response; an unflushed buffer looks like a hang.
    out.flush()
}

/// Handle one request. Returns `None` for notifications, which take no response.
fn handle(provider: &mut impl ToolProvider, request: &Value) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    // Notifications are identified by the absence of an id.
    let Some(id) = id else {
        if method == "notifications/initialized" {
            eprintln!("[mcp] client initialized");
        }
        return None;
    };

    let response = match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(DEFAULT_PROTOCOL);
            let protocol = if SUPPORTED_PROTOCOLS.contains(&requested) { requested } else { DEFAULT_PROTOCOL };
            let info = provider.server_info();
            success(
                id,
                json!({
                    "protocolVersion": protocol,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": info.name, "version": info.version },
                    "instructions": info.instructions,
                }),
            )
        }
        "ping" => success(id, json!({})),
        "tools/list" => {
            let tools: Vec<Value> = provider
                .tools()
                .into_iter()
                .map(|tool| {
                    json!({
                        "name": tool.name,
                        "description": tool.description,
                        "inputSchema": tool.input_schema,
                    })
                })
                .collect();
            success(id, json!({ "tools": tools }))
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            if name.is_empty() {
                return Some(error_response(id, -32602, "tools/call requires a tool name"));
            }
            match provider.call(name, &arguments) {
                Ok(output) => success(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": output.text }],
                        "isError": false,
                    }),
                ),
                // Tool failures are results, not protocol errors: the model reads
                // the message and can correct its next call.
                Err(message) => success(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": message }],
                        "isError": true,
                    }),
                ),
            }
        }
        // Declared capabilities say tools only; answer the optional lists empty
        // rather than erroring, since some clients probe for them regardless.
        "resources/list" => success(id, json!({ "resources": [] })),
        "resources/templates/list" => success(id, json!({ "resourceTemplates": [] })),
        "prompts/list" => success(id, json!({ "prompts": [] })),
        other => error_response(id, -32601, &format!("unknown method: {other}")),
    };

    Some(response)
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

// ---------------------------------------------------------------------------
// argument helpers
// ---------------------------------------------------------------------------

/// Read a required string argument.
pub fn require_str(args: &Value, key: &str) -> Result<String, String> {
    match args.get(key) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(value.trim().to_string()),
        Some(Value::String(_)) => Err(format!("`{key}` must not be empty")),
        Some(_) => Err(format!("`{key}` must be a string")),
        None => Err(format!("`{key}` is required")),
    }
}

/// Read an optional string argument, treating empty as absent.
pub fn opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub fn opt_bool(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(default)
}

pub fn opt_usize(args: &Value, key: &str, default: usize) -> usize {
    args.get(key).and_then(Value::as_u64).map(|value| value as usize).unwrap_or(default)
}

/// Read a string array argument, accepting a lone string as a single-item list.
pub fn opt_str_list(args: &Value, key: &str) -> Vec<String> {
    match args.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        Some(Value::String(single)) => {
            single.split(',').map(str::trim).filter(|v| !v.is_empty()).map(str::to_string).collect()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;

    impl ToolProvider for Echo {
        fn server_info(&self) -> ServerInfo {
            ServerInfo {
                name: "echo".into(),
                version: "1.0.0".into(),
                instructions: "test server".into(),
            }
        }
        fn tools(&self) -> Vec<Tool> {
            vec![Tool::new("echo", "Echo a value", json!({"type": "object"}))]
        }
        fn call(&mut self, name: &str, arguments: &Value) -> Result<ToolOutput, String> {
            if name != "echo" {
                return Err(format!("unknown tool: {name}"));
            }
            Ok(ToolOutput::text(require_str(arguments, "value")?))
        }
    }

    fn request(method: &str, params: Value) -> Value {
        json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
    }

    #[test]
    fn initialize_echoes_a_supported_protocol() {
        let response = handle(&mut Echo, &request("initialize", json!({"protocolVersion": "2024-11-05"}))).unwrap();
        assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(response["result"]["serverInfo"]["name"], "echo");
    }

    #[test]
    fn initialize_falls_back_for_unknown_protocols() {
        let response = handle(&mut Echo, &request("initialize", json!({"protocolVersion": "1999-01-01"}))).unwrap();
        assert_eq!(response["result"]["protocolVersion"], DEFAULT_PROTOCOL);
    }

    #[test]
    fn notifications_get_no_response() {
        let notification = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert!(handle(&mut Echo, &notification).is_none());
    }

    #[test]
    fn tool_errors_come_back_as_results() {
        let response = handle(&mut Echo, &request("tools/call", json!({"name": "echo", "arguments": {}}))).unwrap();
        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"]["content"][0]["text"].as_str().unwrap().contains("`value` is required"));
    }

    #[test]
    fn successful_calls_carry_text_content() {
        let params = json!({"name": "echo", "arguments": {"value": "hello"}});
        let response = handle(&mut Echo, &request("tools/call", params)).unwrap();
        assert_eq!(response["result"]["isError"], false);
        assert_eq!(response["result"]["content"][0]["text"], "hello");
    }

    #[test]
    fn unknown_methods_are_protocol_errors() {
        let response = handle(&mut Echo, &request("does/not/exist", json!({}))).unwrap();
        assert_eq!(response["error"]["code"], -32601);
    }

    #[test]
    fn optional_lists_are_answered_empty() {
        let response = handle(&mut Echo, &request("prompts/list", json!({}))).unwrap();
        assert_eq!(response["result"]["prompts"], json!([]));
    }

    #[test]
    fn argument_helpers_coerce_sensibly() {
        let args = json!({"a": " x ", "b": "", "list": ["one", " two "], "csv": "a, b"});
        assert_eq!(require_str(&args, "a").unwrap(), "x");
        assert!(require_str(&args, "b").is_err());
        assert_eq!(opt_str(&args, "b"), None);
        assert_eq!(opt_str_list(&args, "list"), vec!["one", "two"]);
        assert_eq!(opt_str_list(&args, "csv"), vec!["a", "b"]);
        assert_eq!(opt_usize(&args, "missing", 7), 7);
    }
}
