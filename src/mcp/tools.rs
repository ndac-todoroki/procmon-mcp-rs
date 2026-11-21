//! Tool definitions and argument handling.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::analysis::{Filter, summarize_file};
use crate::capture::ProgressFn;
use crate::cli::DEFAULT_TOP_N;
use crate::report::{output_schema, to_json};

/// Upper bound for `top_n`, keeping responses small enough for a model's context.
const MAX_TOP_N: u64 = 50;

/// Result of a tool run.
#[derive(Debug, PartialEq)]
pub struct ToolOutput {
    /// Text content. For structured results this is the same JSON, serialized, for
    /// clients that ignore `structuredContent`.
    pub text: String,
    /// `structuredContent`, conforming to the tool's `outputSchema`.
    pub structured: Option<Value>,
    pub is_error: bool,
}

impl ToolOutput {
    fn structured(value: Value) -> Self {
        ToolOutput {
            text: value.to_string(),
            structured: Some(value),
            is_error: false,
        }
    }

    fn error(text: impl Into<String>) -> Self {
        ToolOutput {
            text: text.into(),
            structured: None,
            is_error: true,
        }
    }
}

/// The `tools` array for `tools/list`.
pub fn definitions() -> Value {
    json!([{
        "name": "summarize_log",
        "title": "Summarize a Process Monitor log",
        "description": "Summarize a saved Process Monitor log (.pml): event counts per \
            process and operation, the most frequent operation/path pairs with bytes read \
            and written, files reopened periodically with their intervals, and failed \
            operations. Optional conditions narrow which events are counted.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "pml_path": {"type": "string", "description": "Path of the .pml file"},
                "process_name": {"type": "string", "description": "Image name, e.g. HuionTablet.exe"},
                "pid": {"type": "integer", "minimum": 0, "maximum": u32::MAX},
                "operations": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Operation names, e.g. [\"ReadFile\", \"RegSetValue\"]"
                },
                "path_contains": {"type": "string", "description": "Case-insensitive substring of the path"},
                "top_n": {"type": "integer", "minimum": 1, "maximum": MAX_TOP_N, "default": DEFAULT_TOP_N}
            },
            "required": ["pml_path"],
            "additionalProperties": false
        },
        "outputSchema": output_schema(),
        "annotations": {"readOnlyHint": true, "openWorldHint": false}
    }])
}

/// Runs a tool. `None` means there is no tool with that name.
pub fn call(name: &str, arguments: &Value, _progress: &mut ProgressFn<'_>) -> Option<ToolOutput> {
    match name {
        "summarize_log" => Some(summarize_log(arguments).unwrap_or_else(ToolOutput::error)),
        _ => None,
    }
}

fn summarize_log(args: &Value) -> Result<ToolOutput, String> {
    let path = PathBuf::from(required_string(args, "pml_path")?);
    let filter = Filter {
        process_name: optional_string(args, "process_name")?,
        pid: optional_u64(args, "pid", u64::from(u32::MAX))?.map(|n| n as u32),
        operations: optional_strings(args, "operations")?,
        path_contains: optional_string(args, "path_contains")?,
    };
    let top_n = optional_u64(args, "top_n", MAX_TOP_N)?.unwrap_or(DEFAULT_TOP_N as u64);
    if top_n == 0 {
        return Err("top_n must be at least 1".to_owned());
    }
    // Only Process Monitor logs: the tool must not become a way to probe arbitrary files.
    let is_pml = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pml"));
    if !is_pml {
        return Err(format!("only .pml files are accepted: {}", path.display()));
    }
    let summary = summarize_file(&path, filter).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(ToolOutput::structured(to_json(&summary, top_n as usize)))
}

fn required_string(args: &Value, key: &str) -> Result<String, String> {
    optional_string(args, key)?.ok_or_else(|| format!("{key} is required"))
}

fn optional_string(args: &Value, key: &str) -> Result<Option<String>, String> {
    match &args[key] {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.clone())),
        _ => Err(format!("{key} must be a string")),
    }
}

fn optional_u64(args: &Value, key: &str, max: u64) -> Result<Option<u64>, String> {
    match &args[key] {
        Value::Null => Ok(None),
        v => match v.as_u64() {
            Some(n) if n <= max => Ok(Some(n)),
            _ => Err(format!("{key} must be an integer from 0 to {max}")),
        },
    }
}

fn optional_strings(args: &Value, key: &str) -> Result<Vec<String>, String> {
    match &args[key] {
        Value::Null => Ok(Vec::new()),
        Value::Array(items) => items
            .iter()
            .map(|i| {
                i.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("{key} must be an array of strings"))
            })
            .collect(),
        _ => Err(format!("{key} must be an array of strings")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_log_is_defined_with_pml_path_required() {
        let tools = definitions();
        let tool = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "summarize_log")
            .expect("summarize_log defined");
        assert_eq!(tool["inputSchema"]["type"], "object");
        assert_eq!(tool["inputSchema"]["required"], json!(["pml_path"]));
        assert!(tool["description"].as_str().unwrap().len() > 20);
    }

    #[test]
    fn unknown_tool_is_none() {
        assert_eq!(call("format_disk", &json!({}), &mut |_, _| {}), None);
    }

    fn summarize(arguments: Value) -> ToolOutput {
        call("summarize_log", &arguments, &mut |_, _| {}).unwrap()
    }

    #[test]
    fn summarize_log_rejects_files_that_are_not_pml() {
        let out = summarize(json!({"pml_path": r"C:\Windows\win.ini"}));
        assert!(out.is_error);
        assert!(out.text.contains(".pml"), "{}", out.text);
    }

    #[test]
    fn summarize_log_reports_bad_argument_types_as_tool_errors() {
        for args in [
            json!({}),
            json!({"pml_path": 3}),
            json!({"pml_path": "a.pml", "pid": "abc"}),
            json!({"pml_path": "a.pml", "pid": -1}),
            json!({"pml_path": "a.pml", "operations": "ReadFile"}),
            json!({"pml_path": "a.pml", "top_n": 0}),
            json!({"pml_path": "a.pml", "top_n": 51}),
        ] {
            let out = summarize(args.clone());
            assert!(out.is_error, "accepted {args}");
        }
    }

    #[test]
    fn summarize_log_reports_unreadable_files_as_tool_errors() {
        let out = summarize(json!({"pml_path": r"Z:\does\not\exist.pml"}));
        assert!(out.is_error);
    }
}
