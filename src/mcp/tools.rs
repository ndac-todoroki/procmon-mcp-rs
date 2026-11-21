//! Tool definitions and argument handling.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};

use crate::analysis::{Filter, summarize_file};
use crate::capture::{OVERHEAD, ProgressFn, run_capture};
use crate::cli::DEFAULT_TOP_N;
use crate::job::{CaptureRequest, JobId, MAX_DURATION_S, MIN_DURATION_S};
use crate::layout::Layout;
use crate::pmc::CaptureFilter;
use crate::report::{output_schema, to_json};
use crate::runner::CAPTURE_FILE;
use crate::task;

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
    }, {
        "name": "capture",
        "title": "Capture with Process Monitor",
        "description": "Run Sysinternals Process Monitor for a few seconds and summarize what \
            happened: event counts per process and operation, the most frequent \
            operation/path pairs with bytes read and written, files reopened periodically \
            with their intervals, and failed operations. Conditions are applied by Process \
            Monitor itself, so narrow the capture (process_name or pid) whenever possible. \
            Requires `procmon-mcp install` to have been run once as administrator.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "duration_s": {"type": "integer", "minimum": MIN_DURATION_S, "maximum": MAX_DURATION_S,
                               "default": DEFAULT_DURATION_S, "description": "Seconds to capture"},
                "process_name": {"type": "string", "description": "Image name, e.g. HuionTablet.exe"},
                "pid": {"type": "integer", "minimum": 0, "maximum": u32::MAX},
                "operations": {
                    "type": "array",
                    "items": {"type": "string"},
                    "maxItems": crate::pmc::MAX_OPERATIONS,
                    "description": "Operation names, e.g. [\"ReadFile\", \"RegSetValue\"]"
                },
                "path_contains": {"type": "string", "description": "Case-insensitive substring of the path"},
                "result": {"type": "string", "description": "Result text, e.g. NAME NOT FOUND"},
                "top_n": {"type": "integer", "minimum": 1, "maximum": MAX_TOP_N, "default": DEFAULT_TOP_N},
                "keep_log": {"type": "boolean", "default": false,
                             "description": "Keep the .pml log and return its path for summarize_log"}
            },
            "additionalProperties": false
        },
        "outputSchema": capture_output_schema(),
        "annotations": {"readOnlyHint": false, "destructiveHint": false, "openWorldHint": false}
    }])
}

const DEFAULT_DURATION_S: u64 = 10;

/// The summary schema plus the optional path of a kept log.
fn capture_output_schema() -> Value {
    let mut schema = output_schema();
    schema["properties"]["log_file"] = json!({
        "type": "string",
        "description": "Path of the kept .pml log (only with keep_log)"
    });
    schema
}

/// The request for the runner plus `top_n` and `keep_log`, validated exactly as the
/// runner will validate it.
fn capture_request(args: &Value) -> Result<(CaptureRequest, usize, bool), String> {
    let request = CaptureRequest {
        id: JobId::random().map_err(|e| format!("cannot create a capture ID: {e}"))?,
        duration_s: optional_u64(args, "duration_s", MAX_DURATION_S)?.unwrap_or(DEFAULT_DURATION_S),
        filter: CaptureFilter {
            process_name: optional_string(args, "process_name")?,
            pid: optional_u64(args, "pid", u64::from(u32::MAX))?.map(|n| n as u32),
            operations: optional_strings(args, "operations")?,
            path_contains: optional_string(args, "path_contains")?,
            result: optional_string(args, "result")?,
        },
    };
    CaptureRequest::parse(request.to_json().to_string().as_bytes())?;
    let top_n = optional_u64(args, "top_n", MAX_TOP_N)?.unwrap_or(DEFAULT_TOP_N as u64);
    if top_n == 0 {
        return Err("top_n must be at least 1".to_owned());
    }
    let keep_log = match &args["keep_log"] {
        Value::Null => false,
        Value::Bool(b) => *b,
        _ => return Err("keep_log must be true or false".to_owned()),
    };
    Ok((request, top_n as usize, keep_log))
}

fn capture(args: &Value, progress: &mut ProgressFn<'_>) -> Result<ToolOutput, String> {
    let (request, top_n, keep_log) = capture_request(args)?;
    let layout = Layout::system().map_err(|e| format!("cannot locate ProgramData: {e}"))?;
    let installed = layout.procmon().is_file() && task::exists().unwrap_or(false);
    if !installed {
        return Err(
            "procmon-mcp is not installed: run `procmon-mcp install` once from an \
                    administrator terminal"
                .to_owned(),
        );
    }
    let _lock = CaptureLock::acquire(&layout)?;
    let timeout = Duration::from_secs(request.duration_s) + OVERHEAD;
    let job = run_capture(&layout, &request, task::run, timeout, progress)?;

    let log = job.join(CAPTURE_FILE);
    let f = &request.filter;
    let filter = Filter {
        process_name: f.process_name.clone(),
        pid: f.pid,
        operations: f.operations.clone(),
        path_contains: f.path_contains.clone(),
    };
    let summary = summarize_file(&log, filter).map_err(|e| format!("{}: {e}", log.display()));
    if !keep_log {
        // The user may delete finished jobs in out; leftovers are harmless anyway.
        let _ = std::fs::remove_dir_all(&job);
    }
    let mut json = to_json(&summary?, top_n);
    if keep_log {
        json["log_file"] = json!(log);
    }
    Ok(ToolOutput::structured(json))
}

/// Held for the whole capture. The file is opened without sharing, so a second server
/// process (another Claude Desktop window) cannot start a capture meanwhile, and Windows
/// releases it if this process dies.
struct CaptureLock(#[allow(dead_code)] std::fs::File);

impl CaptureLock {
    fn acquire(layout: &Layout) -> Result<CaptureLock, String> {
        use std::os::windows::fs::OpenOptionsExt;
        const ERROR_SHARING_VIOLATION: i32 = 32;
        let path = layout.jobs().join("capture.lock");
        match std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .share_mode(0)
            .open(&path)
        {
            Ok(file) => Ok(CaptureLock(file)),
            Err(e) if e.raw_os_error() == Some(ERROR_SHARING_VIOLATION) => {
                Err("another capture is already running; try again when it has finished".to_owned())
            }
            Err(e) => Err(format!("cannot lock {}: {e}", path.display())),
        }
    }
}

/// Runs a tool. `None` means there is no tool with that name.
pub fn call(name: &str, arguments: &Value, progress: &mut ProgressFn<'_>) -> Option<ToolOutput> {
    match name {
        "summarize_log" => Some(summarize_log(arguments).unwrap_or_else(ToolOutput::error)),
        "capture" => Some(capture(arguments, progress).unwrap_or_else(ToolOutput::error)),
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

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn capture_is_defined_with_bounded_duration_and_optional_log_file() {
        let tools = definitions();
        let tool = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "capture")
            .expect("capture defined");
        let duration = &tool["inputSchema"]["properties"]["duration_s"];
        assert_eq!(
            (duration["minimum"].as_u64(), duration["maximum"].as_u64()),
            (Some(1), Some(120))
        );
        assert!(tool["outputSchema"]["properties"]["log_file"].is_object());
        assert_eq!(tool["annotations"]["readOnlyHint"], false);
    }

    #[test]
    fn capture_request_defaults_to_ten_seconds_top_fifteen_and_no_log() {
        let (request, top_n, keep_log) = capture_request(&json!({})).unwrap();
        assert_eq!(request.duration_s, 10);
        assert_eq!(request.filter, crate::pmc::CaptureFilter::default());
        assert_eq!((top_n, keep_log), (DEFAULT_TOP_N, false));
    }

    #[test]
    fn capture_request_carries_every_condition() {
        let (request, top_n, keep_log) = capture_request(&json!({
            "duration_s": 15, "process_name": "a.exe", "pid": 7,
            "operations": ["ReadFile"], "path_contains": "cfg", "result": "NAME NOT FOUND",
            "top_n": 5, "keep_log": true
        }))
        .unwrap();
        assert_eq!(request.duration_s, 15);
        assert_eq!(request.filter.process_name.as_deref(), Some("a.exe"));
        assert_eq!(request.filter.pid, Some(7));
        assert_eq!(request.filter.operations, ["ReadFile"]);
        assert_eq!(request.filter.path_contains.as_deref(), Some("cfg"));
        assert_eq!(request.filter.result.as_deref(), Some("NAME NOT FOUND"));
        assert_eq!((top_n, keep_log), (5, true));
    }

    #[test]
    fn capture_request_rejects_what_the_runner_would_refuse() {
        let too_many_operations = vec!["ReadFile"; crate::pmc::MAX_OPERATIONS + 1];
        for args in [
            json!({"duration_s": 0}),
            json!({"duration_s": 121}),
            json!({"keep_log": "yes"}),
            json!({"pid": -1}),
            json!({"path_contains": "a\u{1}b"}),
            json!({ "operations": too_many_operations }),
        ] {
            assert!(capture_request(&args).is_err(), "accepted {args}");
        }
    }
}
