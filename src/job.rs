//! The capture request the MCP server hands to the elevated runner, and the status the
//! runner reports back.
//!
//! The runner reads the request from a user-writable directory, so it trusts nothing in
//! it: the ID becomes a directory name and the filter ends up in Procmon's configuration.

use serde_json::{Value, json};

use crate::pmc::CaptureFilter;

pub const MIN_DURATION_S: u64 = 1;
pub const MAX_DURATION_S: u64 = 120;
/// Requests are a few hundred bytes; anything larger is not ours.
pub const MAX_REQUEST_BYTES: u64 = 64 * 1024;

/// A capture ID: 32 lowercase hex digits, safe to use as a directory name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobId(String);

impl JobId {
    pub fn parse(s: &str) -> Result<JobId, String> {
        let valid = s.len() == 32 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if valid {
            Ok(JobId(s.to_owned()))
        } else {
            Err("id must be 32 lowercase hex digits".to_owned())
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRequest {
    pub id: JobId,
    pub duration_s: u64,
    pub filter: CaptureFilter,
}

impl CaptureRequest {
    pub fn to_json(&self) -> Value {
        let f = &self.filter;
        let mut filter = json!({});
        if let Some(v) = &f.process_name {
            filter["process_name"] = json!(v);
        }
        if let Some(v) = f.pid {
            filter["pid"] = json!(v);
        }
        if !f.operations.is_empty() {
            filter["operations"] = json!(f.operations);
        }
        if let Some(v) = &f.path_contains {
            filter["path_contains"] = json!(v);
        }
        if let Some(v) = &f.result {
            filter["result"] = json!(v);
        }
        json!({"id": self.id.as_str(), "duration_s": self.duration_s, "filter": filter})
    }

    /// Parses and validates a request. Errors never echo the input back.
    pub fn parse(bytes: &[u8]) -> Result<CaptureRequest, String> {
        let v: Value = serde_json::from_slice(bytes).map_err(|_| "request is not valid JSON")?;
        let obj = only_keys(&v, &["id", "duration_s", "filter"], "request")?;
        let id = JobId::parse(obj.get("id").and_then(Value::as_str).unwrap_or(""))?;
        let duration_s = obj
            .get("duration_s")
            .and_then(Value::as_u64)
            .filter(|d| (MIN_DURATION_S..=MAX_DURATION_S).contains(d))
            .ok_or("duration_s must be an integer from 1 to 120")?;
        let filter = parse_filter(obj.get("filter").unwrap_or(&Value::Null))?;
        // The runner builds Procmon's configuration from this; refuse anything it would.
        crate::pmc::capture_configuration(&filter).map_err(|e| e.to_string())?;
        Ok(CaptureRequest {
            id,
            duration_s,
            filter,
        })
    }
}

fn parse_filter(v: &Value) -> Result<CaptureFilter, String> {
    let obj = only_keys(
        v,
        &[
            "process_name",
            "pid",
            "operations",
            "path_contains",
            "result",
        ],
        "filter",
    )?;
    let text = |key: &str| -> Result<Option<String>, String> {
        match obj.get(key) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(format!("filter.{key} must be a string")),
        }
    };
    let pid = match obj.get("pid") {
        None => None,
        Some(p) => Some(
            p.as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or("filter.pid must be an integer from 0 to 4294967295")?,
        ),
    };
    let operations = match obj.get("operations") {
        None => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|i| i.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .ok_or("filter.operations must be an array of strings")?,
        Some(_) => return Err("filter.operations must be an array of strings".to_owned()),
    };
    Ok(CaptureFilter {
        process_name: text("process_name")?,
        pid,
        operations,
        path_contains: text("path_contains")?,
        result: text("result")?,
    })
}

/// The object's map, if `v` is an object with no keys other than `allowed`.
fn only_keys<'a>(
    v: &'a Value,
    allowed: &[&str],
    what: &str,
) -> Result<&'a serde_json::Map<String, Value>, String> {
    let obj = v
        .as_object()
        .ok_or_else(|| format!("{what} must be a JSON object"))?;
    if obj.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(format!("{what} has unexpected fields"));
    }
    Ok(obj)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdef0123456789abcdef";

    fn request() -> CaptureRequest {
        CaptureRequest {
            id: JobId::parse(ID).unwrap(),
            duration_s: 15,
            filter: CaptureFilter {
                process_name: Some("HuionTablet.exe".into()),
                pid: Some(42),
                operations: vec!["ReadFile".into(), "CreateFile".into()],
                path_contains: Some("user.cfg".into()),
                result: Some("NAME NOT FOUND".into()),
            },
        }
    }

    #[test]
    fn round_trips_through_json() {
        let r = request();
        let bytes = r.to_json().to_string().into_bytes();
        assert_eq!(CaptureRequest::parse(&bytes), Ok(r));
    }

    #[test]
    fn unset_conditions_are_omitted_and_parse_back_as_unset() {
        let r = CaptureRequest {
            filter: CaptureFilter::default(),
            ..request()
        };
        assert_eq!(
            r.to_json(),
            json!({"id": ID, "duration_s": 15, "filter": {}})
        );
        let bytes = r.to_json().to_string().into_bytes();
        assert_eq!(CaptureRequest::parse(&bytes), Ok(r));
    }

    #[test]
    fn job_ids_are_exactly_32_lowercase_hex_digits() {
        assert!(JobId::parse(ID).is_ok());
        for bad in [
            "",
            "0123456789ABCDEF0123456789ABCDEF",
            "0123456789abcdef0123456789abcde",
            "0123456789abcdef0123456789abcdef0",
            r"..\..\..\Windows\System32\x00000",
            "0123456789abcdef0123456789abcdeg",
        ] {
            assert!(JobId::parse(bad).is_err(), "accepted {bad:?}");
        }
    }

    fn with(key: &str, value: Value) -> Vec<u8> {
        let mut v = request().to_json();
        v[key] = value;
        v.to_string().into_bytes()
    }

    #[test]
    fn rejects_durations_outside_1_to_120_seconds() {
        for bad in [json!(0), json!(121), json!(1.5), json!("10"), json!(-1)] {
            assert!(
                CaptureRequest::parse(&with("duration_s", bad.clone())).is_err(),
                "{bad}"
            );
        }
        assert!(CaptureRequest::parse(&with("duration_s", json!(120))).is_ok());
    }

    #[test]
    fn rejects_bad_ids_and_unknown_or_missing_fields() {
        assert!(CaptureRequest::parse(&with("id", json!("../x"))).is_err());
        assert!(CaptureRequest::parse(&with("output", json!(r"C:\Windows"))).is_err());
        let mut missing = request().to_json();
        missing.as_object_mut().unwrap().remove("duration_s");
        assert!(CaptureRequest::parse(missing.to_string().as_bytes()).is_err());
        assert!(CaptureRequest::parse(b"[]").is_err());
        assert!(CaptureRequest::parse(b"not json").is_err());
    }

    #[test]
    fn rejects_filters_that_the_capture_configuration_would_refuse() {
        let mut v = request().to_json();
        v["filter"]["path_contains"] = json!("a\u{0}b");
        assert!(CaptureRequest::parse(v.to_string().as_bytes()).is_err());
        let mut v = request().to_json();
        v["filter"]["pid"] = json!("42");
        assert!(CaptureRequest::parse(v.to_string().as_bytes()).is_err());
        let mut v = request().to_json();
        v["filter"]["color"] = json!("red");
        assert!(CaptureRequest::parse(v.to_string().as_bytes()).is_err());
    }

    #[test]
    fn errors_do_not_echo_the_input() {
        let secret = "C:\\Users\\victim\\secret.txt";
        let err = CaptureRequest::parse(&with("id", json!(secret))).unwrap_err();
        assert!(!err.contains("secret"), "{err}");
    }
}
