//! Command-line parsing. Hand-written to keep the dependency tree small.

use std::path::PathBuf;

use crate::analysis::Filter;

pub const DEFAULT_TOP_N: usize = 15;

pub const USAGE: &str = "\
Usage:
  procmon-mcp install [--accept-eula]      set up captures (run as administrator)
  procmon-mcp update                       get a newer Process Monitor, if any (administrator)
  procmon-mcp uninstall                    remove everything install set up (administrator)
  procmon-mcp mcp                          run the MCP server on stdin/stdout
  procmon-mcp summarize <file.pml> [options]

Summarize options:
  --process <name>        only events of this process image name
  --pid <pid>             only events of this process ID
  --operation <name>      only this operation (repeatable)
  --path-contains <text>  only events whose path contains this text
  --top <n>               entries per section (default 15)
  --json                  print the JSON returned to MCP clients
";

#[derive(Debug, PartialEq)]
pub enum Command {
    /// Run the MCP server on stdin/stdout.
    Mcp,
    /// Run the pending capture elevated; started by the scheduled task only.
    Runner,
    /// Set up ProgramData\ProcmonMcp, Process Monitor and the scheduled task (elevated).
    Install { accept_eula: bool },
    /// Download Process Monitor again if Sysinternals has a newer one (elevated).
    Update,
    /// Remove everything install set up (elevated).
    Uninstall,
    Summarize {
        path: PathBuf,
        filter: Filter,
        top_n: usize,
        /// Print the JSON that MCP clients receive instead of the text report.
        json: bool,
    },
}

/// Parses the arguments after the program name.
pub fn parse_args(args: &[String]) -> Result<Command, String> {
    match args.split_first() {
        Some((cmd, [])) if cmd == "mcp" => Ok(Command::Mcp),
        Some((cmd, [])) if cmd == "runner" => Ok(Command::Runner),
        Some((cmd, _)) if cmd == "runner" => Err("runner takes no arguments".to_owned()),
        Some((cmd, _)) if cmd == "mcp" => Err("mcp takes no arguments".to_owned()),
        Some((cmd, rest)) if cmd == "install" => match rest {
            [] => Ok(Command::Install { accept_eula: false }),
            [flag] if flag == "--accept-eula" => Ok(Command::Install { accept_eula: true }),
            _ => Err("install accepts only --accept-eula".to_owned()),
        },
        Some((cmd, [])) if cmd == "update" => Ok(Command::Update),
        Some((cmd, [])) if cmd == "uninstall" => Ok(Command::Uninstall),
        Some((cmd, _)) if cmd == "update" || cmd == "uninstall" => {
            Err(format!("{cmd} takes no arguments"))
        }
        Some((cmd, rest)) if cmd == "summarize" => parse_summarize(rest),
        Some((cmd, _)) => Err(format!("unknown command: {cmd}")),
        None => Err("missing command".to_owned()),
    }
}

fn parse_summarize(args: &[String]) -> Result<Command, String> {
    let mut path = None;
    let mut filter = Filter::default();
    let mut top_n = DEFAULT_TOP_N;
    let mut json = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{arg} needs a value"))
        };
        match arg.as_str() {
            "--process" => filter.process_name = Some(value()?),
            "--pid" => filter.pid = Some(parse_number(arg, &value()?)?),
            "--operation" => filter.operations.push(value()?),
            "--path-contains" => filter.path_contains = Some(value()?),
            "--json" => json = true,
            "--top" => {
                top_n = parse_number(arg, &value()?)?;
                if top_n == 0 {
                    return Err("--top must be at least 1".to_owned());
                }
            }
            opt if opt.starts_with("--") => return Err(format!("unknown option: {opt}")),
            _ if path.is_some() => return Err(format!("unexpected argument: {arg}")),
            _ => path = Some(PathBuf::from(arg)),
        }
    }
    let path = path.ok_or("missing <file.pml>")?;
    Ok(Command::Summarize {
        path,
        filter,
        top_n,
        json,
    })
}

fn parse_number<T: std::str::FromStr>(option: &str, value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("{option} expects a number, got {value:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn summarize_takes_a_path_with_default_options() {
        assert_eq!(
            parse_args(&args("summarize log.pml")),
            Ok(Command::Summarize {
                path: "log.pml".into(),
                filter: Filter::default(),
                top_n: DEFAULT_TOP_N,
                json: false,
            })
        );
    }

    #[test]
    fn summarize_accepts_every_filter_option_in_any_order() {
        let parsed = parse_args(&args(
            "summarize --top 5 --operation ReadFile log.pml --pid 42 \
             --process a.exe --operation CreateFile --path-contains cfg",
        ));
        assert_eq!(
            parsed,
            Ok(Command::Summarize {
                path: "log.pml".into(),
                filter: Filter {
                    process_name: Some("a.exe".into()),
                    pid: Some(42),
                    operations: vec!["ReadFile".into(), "CreateFile".into()],
                    path_contains: Some("cfg".into()),
                },
                top_n: 5,
                json: false,
            })
        );
    }

    #[test]
    fn summarize_json_switches_output_to_json() {
        let Ok(Command::Summarize { json, .. }) = parse_args(&args("summarize --json a.pml"))
        else {
            panic!("not parsed as summarize");
        };
        assert!(json);
    }

    #[test]
    fn setup_commands_parse() {
        assert_eq!(
            parse_args(&args("install")),
            Ok(Command::Install { accept_eula: false })
        );
        assert_eq!(
            parse_args(&args("install --accept-eula")),
            Ok(Command::Install { accept_eula: true })
        );
        assert_eq!(parse_args(&args("update")), Ok(Command::Update));
        assert_eq!(parse_args(&args("uninstall")), Ok(Command::Uninstall));
    }

    #[test]
    fn runner_runs_the_pending_capture() {
        assert_eq!(parse_args(&args("runner")), Ok(Command::Runner));
    }

    #[test]
    fn mcp_starts_the_stdio_server() {
        assert_eq!(parse_args(&args("mcp")), Ok(Command::Mcp));
    }

    #[test]
    fn rejects_missing_or_unknown_input() {
        for bad in [
            "",
            "frobnicate",
            "mcp --verbose",
            "runner now",
            "install --yes",
            "update now",
            "uninstall --force",
            "summarize",
            "summarize a.pml b.pml",
            "summarize a.pml --pid",
            "summarize a.pml --pid abc",
            "summarize a.pml --top 0",
            "summarize a.pml --color",
        ] {
            assert!(parse_args(&args(bad)).is_err(), "accepted: {bad:?}");
        }
    }
}
