#![windows_subsystem = "windows"]
//! procmon-mcp-setup: installs procmon-mcp with a walkthrough, and uninstalls it.
//!
//! The window runs with normal rights; selecting Install or Uninstall starts this same
//! executable elevated as a worker (see `protocol`).

mod protocol;
mod text;
mod ui;
mod win;
mod wizard;
mod worker;

use protocol::Mode;
use text::Lang;
use wizard::Flow;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flow = match Mode::parse(&args) {
        Ok(Mode::Worker { task, id }) => std::process::exit(worker::run(task, &id)),
        Ok(Mode::Install) => Flow::Install,
        Ok(Mode::Uninstall) => Flow::Uninstall,
        Err(_) => std::process::exit(2),
    };
    let lang = std::env::var("PROCMON_MCP_LANG")
        .ok()
        .and_then(|v| Lang::from_override(&v))
        .unwrap_or_else(|| Lang::from_lang_id(win::ui_lang_id()));
    if ui::run(flow, lang).is_err() {
        let text = lang.text();
        win::message_box(text.window_title, text.runtime_missing);
        std::process::exit(1);
    }
}
