//! Model Context Protocol over stdio, tools only.
//!
//! Serves both protocol eras on one stdio process:
//! - modern (2026-07-28): every request carries its version in `_meta`; no handshake.
//! - legacy (2025-11-25 and earlier): an `initialize` request opens a session.

use serde_json::{Value, json};

/// Revisions served with per-request `_meta`.
pub const MODERN_VERSIONS: &[&str] = &["2026-07-28"];
/// Revisions served after an `initialize` handshake, newest first.
pub const LEGACY_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

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
        let id = message.get("id")?.clone();
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let outcome = if method == "initialize" {
            Ok(self.initialize(&params))
        } else {
            self.era_of(&params).and_then(|era| {
                let mut result = self.dispatch(method, &params)?;
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

    fn dispatch(&mut self, method: &str, _params: &Value) -> Result<Value, RpcError> {
        match method {
            "server/discover" => Ok(json!({
                "supportedVersions": supported_versions(),
                "capabilities": capabilities(),
            })),
            // Removed in 2026-07-28 but still sent by legacy clients.
            "ping" => Ok(json!({})),
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
        })
    }
}

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
}
