use std::process::ExitCode;

use procmon_mcp::analysis::summarize_file;
use procmon_mcp::cli::{Command, USAGE, parse_args};
use procmon_mcp::report::render;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = match parse_args(&args) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("procmon-mcp: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match command {
        Command::Summarize {
            path,
            filter,
            top_n,
        } => match summarize_file(&path, filter) {
            Ok(summary) => {
                print!("{}", render(&summary, top_n));
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("procmon-mcp: {}: {e}", path.display());
                ExitCode::FAILURE
            }
        },
    }
}
