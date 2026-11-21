//! Runs the built executable as an MCP server over real pipes, as Claude Desktop does.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

fn exchange(messages: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_procmon-mcp"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for m in messages {
            writeln!(stdin, "{m}").unwrap();
        }
        // Dropping stdin closes it; the server must then exit on its own.
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn modern(id: i64, method: &str, params: Value) -> Value {
    let mut params = params;
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

#[test]
fn modern_client_discovers_lists_and_exits_on_eof() {
    let responses = exchange(&[
        modern(1, "server/discover", json!({})),
        modern(2, "tools/list", json!({})),
    ]);
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["result"]["resultType"], "complete");
    assert_eq!(responses[1]["result"]["tools"][0]["name"], "summarize_log");
}

#[test]
fn summarize_log_over_stdio_matches_reference_summary() {
    let dir = std::env::var_os("PROCMON_MCP_REFERENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("reference"));
    let pml = dir.join("Logfile.PML");
    if !pml.is_file() {
        eprintln!("skipped: {} not found", pml.display());
        return;
    }
    let responses = exchange(&[modern(
        1,
        "tools/call",
        json!({"name": "summarize_log", "arguments": {"pml_path": pml, "top_n": 3}}),
    )]);
    let result = &responses[0]["result"];
    assert_eq!(result["isError"], false);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("Events: 6426 /"), "{text}");
    assert!(text.contains("186.4 MiB total"), "{text}");
}
