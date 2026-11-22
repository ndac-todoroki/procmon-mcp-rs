//! Model Context Protocol over stdio, tools only.
//!
//! Serves both protocol eras on one stdio process:
//! - modern (2026-07-28): every request carries its version in `_meta`; no handshake.
//! - legacy (2025-11-25 and earlier): an `initialize` request opens a session.

mod progress;
mod tools;

use std::io::{BufRead, Write};

use serde_json::{Value, json};

use progress::ProgressReporter;

/// Seconds of capture between progress notifications.
const PROGRESS_INTERVAL_S: f64 = 5.0;

/// Revisions served with per-request `_meta`.
pub const MODERN_VERSIONS: &[&str] = &["2026-07-28"];
/// Revisions served after an `initialize` handshake, newest first.
pub const LEGACY_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

/// A JSON-RPC error response body.
#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> Self {
        RpcError {
            code,
            message: message.into(),
            data: None,
        }
    }

    fn to_json(&self) -> Value {
        let mut e = json!({"code": self.code, "message": self.message});
        if let Some(data) = &self.data {
            e["data"] = data.clone();
        }
        e
    }
}

/// Serves newline-delimited JSON-RPC from `input` until it ends.
pub fn serve(input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
    let mut server = Server::default();
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let mut notify_error = None;
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(message @ Value::Object(_)) => {
                let mut notify = |notification: Value| {
                    if notify_error.is_none()
                        && let Err(e) = write_message(&mut output, &notification)
                    {
                        notify_error = Some(e);
                    }
                };
                server.handle_with(&message, &mut notify)
            }
            // Batches were removed from MCP; a bare value is not a message either.
            Ok(_) => Some(error_response(
                Value::Null,
                RpcError::new(INVALID_REQUEST, "expected a single JSON-RPC message object"),
            )),
            Err(e) => Some(error_response(
                Value::Null,
                RpcError::new(PARSE_ERROR, format!("parse error: {e}")),
            )),
        };
        if let Some(e) = notify_error {
            return Err(e);
        }
        if let Some(response) = response {
            write_message(&mut output, &response)?;
        }
    }
    Ok(())
}

/// Writes one message as a line. serde_json's compact form never contains a raw newline,
/// as stdio framing requires.
fn write_message(output: &mut impl Write, message: &Value) -> std::io::Result<()> {
    serde_json::to_writer(&mut *output, message)?;
    output.write_all(b"\n")?;
    output.flush()
}

fn error_response(id: Value, error: RpcError) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": error.to_json()})
}

/// Which protocol era a request belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Era {
    Modern,
    Legacy,
}

/// Protocol state for one stdio connection.
#[derive(Debug, Default)]
pub struct Server {
    /// Set once a legacy client has sent `initialize` on this process.
    legacy_session: bool,
}

impl Server {
    /// Handles one incoming message and returns the response to send, if any.
    pub fn handle(&mut self, message: &Value) -> Option<Value> {
        self.handle_with(message, &mut |_| {})
    }

    /// Like `handle`, but sends notifications raised while handling (such as progress)
    /// through `notify` before the response is returned.
    pub fn handle_with(&mut self, message: &Value, notify: &mut dyn FnMut(Value)) -> Option<Value> {
        let id = message.get("id")?.clone();
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let outcome = if method == "initialize" {
            Ok(self.initialize(&params))
        } else {
            self.era_of(&params).and_then(|era| {
                let mut result = self.dispatch(method, &params, notify)?;
                if era == Era::Modern {
                    result["resultType"] = json!("complete");
                    result["_meta"] = json!({"io.modelcontextprotocol/serverInfo": server_info()});
                }
                Ok(result)
            })
        };
        Some(match outcome {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e.to_json()}),
        })
    }

    /// Modern requests name their version in `_meta`; anything else needs a legacy session.
    fn era_of(&self, params: &Value) -> Result<Era, RpcError> {
        let meta = &params["_meta"];
        let Some(version) = meta["io.modelcontextprotocol/protocolVersion"].as_str() else {
            return if self.legacy_session {
                Ok(Era::Legacy)
            } else {
                Err(RpcError::new(
                    INVALID_PARAMS,
                    "missing _meta io.modelcontextprotocol/protocolVersion",
                ))
            };
        };
        if !MODERN_VERSIONS.contains(&version) {
            let mut e = RpcError::new(UNSUPPORTED_PROTOCOL_VERSION, "Unsupported protocol version");
            e.data = Some(json!({"supported": supported_versions(), "requested": version}));
            return Err(e);
        }
        if !meta["io.modelcontextprotocol/clientCapabilities"].is_object() {
            return Err(RpcError::new(
                INVALID_PARAMS,
                "missing _meta io.modelcontextprotocol/clientCapabilities",
            ));
        }
        Ok(Era::Modern)
    }

    fn dispatch(
        &mut self,
        method: &str,
        params: &Value,
        notify: &mut dyn FnMut(Value),
    ) -> Result<Value, RpcError> {
        match method {
            "server/discover" => Ok(json!({
                "supportedVersions": supported_versions(),
                "capabilities": capabilities(),
                "instructions": INSTRUCTIONS,
                "ttlMs": CACHE_TTL_MS,
                "cacheScope": "public",
            })),
            // Removed in 2026-07-28 but still sent by legacy clients.
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({
                "tools": tools::definitions(),
                "ttlMs": CACHE_TTL_MS,
                "cacheScope": "public",
            })),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                let arguments = match &params["arguments"] {
                    Value::Null => json!({}),
                    a => a.clone(),
                };
                let token = Some(params["_meta"]["progressToken"].clone()).filter(|t| !t.is_null());
                let mut reporter = ProgressReporter::new(token, notify, PROGRESS_INTERVAL_S);
                let mut progress = |elapsed, total| reporter.report(elapsed, total);
                let output = tools::call(name, &arguments, &mut progress).ok_or_else(|| {
                    RpcError::new(INVALID_PARAMS, format!("Unknown tool: {name}"))
                })?;
                let mut result = json!({
                    "content": [{"type": "text", "text": output.text}],
                    "isError": output.is_error,
                });
                if let Some(structured) = output.structured {
                    result["structuredContent"] = structured;
                }
                Ok(result)
            }
            _ => Err(RpcError::new(
                METHOD_NOT_FOUND,
                format!("method not found: {method}"),
            )),
        }
    }

    fn initialize(&mut self, params: &Value) -> Value {
        let requested = params["protocolVersion"].as_str().unwrap_or("");
        // Echo a version we support; otherwise offer our newest legacy version and let
        // the client decide whether to disconnect.
        let version = LEGACY_VERSIONS
            .iter()
            .find(|v| **v == requested)
            .unwrap_or(&LEGACY_VERSIONS[0]);
        self.legacy_session = true;
        json!({
            "protocolVersion": version,
            "capabilities": capabilities(),
            "serverInfo": server_info(),
            "instructions": INSTRUCTIONS,
        })
    }
}

/// How long clients may cache `server/discover` and `tools/list`: neither changes while
/// the server runs.
const CACHE_TTL_MS: u64 = 24 * 60 * 60 * 1000;

/// Guidance for the model on using this server as a whole, returned by `server/discover`
/// (2026-07-28) and `initialize` (earlier). Clients may add it to the system prompt.
/// The specification asks not to repeat the tool descriptions, so this covers when to
/// reach for `capture` and how to run one with the user.
const INSTRUCTIONS: &str = "\
procmon-mcp shows what Windows programs actually do, by running Sysinternals Process \
Monitor for a few seconds. Use it when a question is about a program's real behavior \
rather than its documentation: which files or registry keys it reads or writes, what \
keeps the disk busy, or why it fails to find, open or save something.

- Narrow `capture` to the program (process_name or pid) whenever it is known. A \
system-wide capture grows to hundreds of MB in seconds.
- Only what happens during the capture is recorded, and recording starts a few seconds \
after the call. If the behavior needs the user, such as clicking Save or starting the program, \
tell them what to do and when before calling `capture`, and give them enough seconds.
- To look at the same events again from another angle, capture with `keep_log` and \
pass the returned path to `summarize_log` instead of capturing again.";

fn supported_versions() -> Vec<&'static str> {
    MODERN_VERSIONS
        .iter()
        .chain(LEGACY_VERSIONS)
        .copied()
        .collect()
}

fn capabilities() -> Value {
    json!({"tools": {}})
}

fn server_info() -> Value {
    json!({"name": "procmon-mcp", "version": env!("CARGO_PKG_VERSION")})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: i64, method: &str, params: Value) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
    }

    fn initialize(version: &str) -> Value {
        request(
            1,
            "initialize",
            json!({
                "protocolVersion": version,
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0"}
            }),
        )
    }

    #[test]
    fn legacy_initialize_echoes_a_supported_version_and_declares_tools() {
        let mut server = Server::default();
        let response = server.handle(&initialize("2025-06-18")).unwrap();
        assert_eq!(response["id"], 1);
        let result = &response["result"];
        assert_eq!(result["protocolVersion"], "2025-06-18");
        assert!(result["capabilities"]["tools"].is_object());
        assert_eq!(result["serverInfo"]["name"], "procmon-mcp");
    }

    #[test]
    fn legacy_initialize_offers_newest_legacy_version_for_unknown_ones() {
        let mut server = Server::default();
        let response = server.handle(&initialize("1999-01-01")).unwrap();
        assert_eq!(response["result"]["protocolVersion"], LEGACY_VERSIONS[0]);
    }

    #[test]
    fn notifications_get_no_response() {
        let mut server = Server::default();
        server.handle(&initialize("2025-11-25"));
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert_eq!(server.handle(&note), None);
    }

    fn legacy_server() -> Server {
        let mut server = Server::default();
        server.handle(&initialize("2025-11-25"));
        server
    }

    #[test]
    fn unknown_methods_are_method_not_found() {
        let response = legacy_server()
            .handle(&request(2, "resources/list", json!({})))
            .unwrap();
        assert_eq!(response["id"], 2);
        assert_eq!(response["error"]["code"], -32601);
    }

    fn modern(id: i64, method: &str, version: &str) -> Value {
        request(
            id,
            method,
            json!({"_meta": {
                "io.modelcontextprotocol/protocolVersion": version,
                "io.modelcontextprotocol/clientCapabilities": {}
            }}),
        )
    }

    #[test]
    fn discover_lists_every_supported_version_and_tools_capability() {
        let response = Server::default()
            .handle(&modern(1, "server/discover", "2026-07-28"))
            .unwrap();
        let result = &response["result"];
        assert_eq!(result["resultType"], "complete");
        let versions = result["supportedVersions"].as_array().unwrap();
        assert!(versions.contains(&json!("2026-07-28")));
        assert!(versions.contains(&json!("2025-11-25")));
        assert!(result["capabilities"]["tools"].is_object());
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
            "procmon-mcp"
        );
    }

    #[test]
    fn discover_may_be_cached_by_any_client() {
        let response = Server::default()
            .handle(&modern(1, "server/discover", "2026-07-28"))
            .unwrap();
        let result = &response["result"];
        assert!(result["ttlMs"].is_u64(), "{result}");
        assert_eq!(result["cacheScope"], "public");
    }

    #[test]
    fn both_eras_give_the_same_server_instructions() {
        let modern = Server::default()
            .handle(&modern(1, "server/discover", "2026-07-28"))
            .unwrap();
        let legacy = Server::default().handle(&initialize("2025-11-25")).unwrap();
        let instructions = modern["result"]["instructions"].as_str().unwrap();
        assert!(instructions.contains("capture"), "{instructions}");
        assert_eq!(legacy["result"]["instructions"], instructions);
    }

    #[test]
    fn modern_request_with_unsupported_version_names_supported_ones() {
        let response = Server::default()
            .handle(&modern(3, "server/discover", "1900-01-01"))
            .unwrap();
        let error = &response["error"];
        assert_eq!(error["code"], -32022);
        assert_eq!(error["data"]["requested"], "1900-01-01");
        assert!(
            error["data"]["supported"]
                .as_array()
                .unwrap()
                .contains(&json!("2026-07-28"))
        );
    }

    #[test]
    fn request_without_version_or_session_is_invalid_params() {
        let response = Server::default()
            .handle(&request(4, "tools/list", json!({})))
            .unwrap();
        assert_eq!(response["error"]["code"], -32602);
    }

    #[test]
    fn legacy_session_answers_ping() {
        let response = legacy_server()
            .handle(&request(5, "ping", json!({})))
            .unwrap();
        assert_eq!(response["result"], json!({}));
    }

    #[test]
    fn tools_list_returns_cacheable_tool_definitions() {
        let response = legacy_server()
            .handle(&request(6, "tools/list", json!({})))
            .unwrap();
        let result = &response["result"];
        assert!(
            result["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["name"] == "summarize_log")
        );
        // Required on tools/list results from 2026-07-28; ignored by older clients.
        assert!(result["ttlMs"].is_u64());
        assert_eq!(result["cacheScope"], "public");
    }

    fn call(name: &str, arguments: Value) -> Value {
        legacy_server()
            .handle(&request(
                7,
                "tools/call",
                json!({"name": name, "arguments": arguments}),
            ))
            .unwrap()
    }

    #[test]
    fn tools_call_with_unknown_name_is_invalid_params() {
        assert_eq!(call("format_disk", json!({}))["error"]["code"], -32602);
    }

    #[test]
    fn tools_call_returns_tool_failures_as_error_results() {
        let result = &call("summarize_log", json!({"pml_path": "notes.txt"}))["result"];
        assert_eq!(result["isError"], true);
        assert_eq!(result["content"][0]["type"], "text");
        assert!(
            result["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains(".pml")
        );
    }
}

#[cfg(test)]
mod serve_tests {
    use super::*;

    fn run(input: &str) -> Vec<Value> {
        let mut out = Vec::new();
        serve(input.as_bytes(), &mut out).unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn answers_each_request_line_with_one_response_line() {
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            "\n",
        );
        let responses = run(input);
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0]["id"], 1);
        assert_eq!(responses[1]["id"], 2);
    }

    #[test]
    fn reports_unparsable_lines_as_parse_errors() {
        let responses = run("{not json\n");
        assert_eq!(responses[0]["error"]["code"], -32700);
        assert_eq!(responses[0]["id"], Value::Null);
    }

    #[test]
    fn rejects_batches_and_non_objects_as_invalid_requests() {
        let responses = run("[]\n42\n");
        assert_eq!(responses.len(), 2);
        assert!(responses.iter().all(|r| r["error"]["code"] == -32600));
    }
}
