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

/// Protocol state for one stdio connection.
#[derive(Debug, Default)]
pub struct Server {}

impl Server {
    /// Handles one incoming message and returns the response to send, if any.
    pub fn handle(&mut self, message: &Value) -> Option<Value> {
        let id = message.get("id")?.clone();
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "initialize" => self.initialize(&params),
            _ => return None,
        };
        Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }

    fn initialize(&mut self, params: &Value) -> Value {
        let requested = params["protocolVersion"].as_str().unwrap_or("");
        // Echo a version we support; otherwise offer our newest legacy version and let
        // the client decide whether to disconnect.
        let version = LEGACY_VERSIONS
            .iter()
            .find(|v| **v == requested)
            .unwrap_or(&LEGACY_VERSIONS[0]);
        json!({
            "protocolVersion": version,
            "capabilities": capabilities(),
            "serverInfo": server_info(),
        })
    }
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
}
