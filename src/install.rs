//! `install`, `update` and `uninstall`: set up and remove everything under
//! `C:\ProgramData\ProcmonMcp` plus the scheduled task. All of them run elevated.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

/// Process Monitor is fetched from here; nothing else is downloaded.
pub const SYSINTERNALS_LIVE: &str = "live.sysinternals.com";
pub const EULA_URL: &str = "https://learn.microsoft.com/sysinternals/license-terms";
/// Procmon64.exe is about 2 MB; refuse anything absurdly larger.
pub const MAX_PROCMON_BYTES: usize = 64 * 1024 * 1024;

/// The Sysinternals Live path of the Procmon build for an architecture
/// (`std::env::consts::ARCH`).
pub fn procmon_download_path(arch: &str) -> &'static str {
    match arch {
        "aarch64" => "/Procmon64a.exe",
        _ => "/Procmon64.exe",
    }
}

/// Asks whether the user accepts the Sysinternals license. Only an explicit yes counts.
pub fn confirm_eula(mut input: impl BufRead, mut output: impl Write) -> bool {
    let _ = write!(
        output,
        "procmon-mcp starts Process Monitor with /AcceptEula, which accepts the \
         Sysinternals Software License Terms on your behalf:\n  {EULA_URL}\n\
         Do you accept them? [y/N] "
    );
    let _ = output.flush();
    let mut line = String::new();
    if input.read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// What install remembers between runs, stored in `bin\state.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    /// ETag of the Procmon64.exe currently installed, for conditional updates.
    pub procmon_etag: Option<String>,
}

impl State {
    pub fn to_json(&self) -> Value {
        json!({"procmon_etag": self.procmon_etag})
    }

    /// A missing or unreadable state is treated as empty: the next update downloads again.
    pub fn parse(bytes: &[u8]) -> State {
        let v: Value = serde_json::from_slice(bytes).unwrap_or(Value::Null);
        State {
            procmon_etag: v["procmon_etag"].as_str().map(str::to_owned),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downloads_the_arm64_build_on_arm64_and_the_x64_build_otherwise() {
        assert_eq!(procmon_download_path("x86_64"), "/Procmon64.exe");
        assert_eq!(procmon_download_path("aarch64"), "/Procmon64a.exe");
    }

    fn answer(input: &str) -> (bool, String) {
        let mut out = Vec::new();
        let ok = confirm_eula(input.as_bytes(), &mut out);
        (ok, String::from_utf8(out).unwrap())
    }

    #[test]
    fn eula_prompt_shows_the_license_url_and_accepts_only_yes() {
        let (ok, shown) = answer("y\n");
        assert!(ok);
        assert!(shown.contains(EULA_URL), "{shown}");
        for yes in ["Y\n", "yes\n", " YES \r\n"] {
            assert!(answer(yes).0, "{yes:?}");
        }
        for no in ["\n", "n\n", "no\n", "sure\n", ""] {
            assert!(!answer(no).0, "{no:?}");
        }
    }

    #[test]
    fn state_round_trips_and_tolerates_garbage() {
        let state = State {
            procmon_etag: Some("\"5b2da45f646ddd1:0\"".into()),
        };
        assert_eq!(State::parse(state.to_json().to_string().as_bytes()), state);
        assert_eq!(State::parse(b"not json"), State::default());
        assert_eq!(State::parse(b"{}"), State::default());
    }
}
