use std::process::ExitCode;

use procmon_mcp::analysis::summarize_file;
use procmon_mcp::cli::{Command, USAGE, parse_args};
use procmon_mcp::install::{InstallOptions, UpdateOutcome};
use procmon_mcp::report::{render, to_json};

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
        Command::Mcp => match procmon_mcp::mcp::serve(std::io::stdin().lock(), std::io::stdout()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("procmon-mcp: stdio error: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Runner => {
            let result = procmon_mcp::layout::Layout::system()
                .map_err(|e| format!("cannot locate ProgramData: {e}"))
                .and_then(|layout| procmon_mcp::runner::run(&layout));
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(message) => {
                    eprintln!("procmon-mcp runner: {message}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Install { accept_eula } => {
            report(procmon_mcp::install::install(&InstallOptions {
                accept_eula,
            }))
        }
        Command::Update => report(procmon_mcp::install::update().map(|outcome| match outcome {
            UpdateOutcome::Updated => eprintln!("Process Monitor was updated."),
            UpdateOutcome::UpToDate => eprintln!("Process Monitor is up to date."),
        })),
        Command::Uninstall => report(procmon_mcp::install::uninstall()),
        Command::Summarize {
            path,
            filter,
            top_n,
            json,
        } => match summarize_file(&path, filter) {
            Ok(summary) if json => {
                println!("{:#}", to_json(&summary, top_n));
                ExitCode::SUCCESS
            }
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

fn report(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("procmon-mcp: {message}");
            ExitCode::FAILURE
        }
    }
}
