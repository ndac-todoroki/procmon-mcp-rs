# procmon-mcp-rs

[日本語](README.ja.md)

An MCP server that lets Claude (or any MCP client) run
[Sysinternals Process Monitor](https://learn.microsoft.com/sysinternals/downloads/procmon)
for a few seconds and summarize what a process is doing: which files and registry keys it
touches, how often, how much it reads and writes, which files it reopens periodically, and
which operations fail.

> "Capture HuionTablet.exe with Procmon for 15 seconds and tell me what it's doing."
>
> → *It rereads `C:\Program Files\HuionTablet\res\user.cfg` every ~312 ms (about 14.6 MiB/s),
> and each time also tries to open a nonexistent `.cfg`, which fails with NAME NOT FOUND.*

> Tested on Windows 11 with Process Monitor 4.13. The design and its rationale are in
> [docs/PLAN.md](docs/PLAN.md) (Japanese).

## Features

- **`capture`**: runs Process Monitor for 1–120 seconds with conditions you give (process
  name, PID, operations, path substring, result) and returns a structured summary. The
  conditions are applied by Process Monitor itself, so only matching events are recorded.
  The log is deleted afterwards unless you ask to keep it.
- **`summarize_log`**: summarizes a saved `.pml` file, optionally narrowed by process, PID,
  operation or path.
- **One executable, no runtime**: a single Rust binary, no Python, no .NET, no async
  runtime, no bundled HTTP or TLS stack (downloads go through Windows' WinHTTP).
- **Least privilege**: the MCP server runs with normal rights. Only Process Monitor is
  started with administrator rights, through a scheduled task set up once by `install`.
- **Process Monitor is not bundled**: `install` downloads it from Sysinternals Live, and
  its Microsoft signature is verified at install time and before every capture.
- **Your Process Monitor settings stay yours**: Process Monitor saves the configuration it
  ran with into your GUI settings; procmon-mcp restores them after each capture.

## Requirements

- Windows 10 or 11 (x64 or ARM64)
- Signed in as an administrator, to install (once)
- An MCP client such as Claude Desktop
- For the graphical setup: the
  [Windows App Runtime 2.x](https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads)

## Installation

### Download the setup

1. Download `procmon-mcp-setup.exe` from the
   [latest release](https://github.com/ndac-todoroki/procmon-mcp-rs/releases/latest).
   It contains the MCP server, so it is the only file you need. To check the download,
   compare the hash below with `SHA256SUMS.txt` from the same release:

   ```powershell
   Get-FileHash .\procmon-mcp-setup.exe -Algorithm SHA256
   ```

2. Run it. The setup is not code-signed, so Windows may show **Windows protected your
   PC**; select **More info**, then **Run anyway**.

3. Follow the walkthrough. It explains what it will change, asks you to accept the
   Sysinternals license, and asks for administrator permission only when you select
   **Install**. The last page has a button that copies the Claude Desktop settings (see
   [Add it to Claude Desktop](#add-it-to-claude-desktop)).

The setup keeps a copy of itself, so procmon-mcp can be removed from
**Settings → Apps → Installed apps**. It needs the Windows App Runtime 2.x (see
Requirements); the MCP server does not. The release is built for x64.

### Build it yourself

1. Install [Rust](https://rustup.rs/) 1.95 or later with the MSVC toolchain, and the
   Visual Studio Build Tools (**Desktop development with C++**, which includes the
   Windows SDK).

2. Build the server, then the setup, which embeds the server and so must come second:

   ```powershell
   cargo build --release                        # target\release\procmon-mcp.exe
   cargo build --release -p procmon-mcp-setup   # target\release\procmon-mcp-setup.exe
   ```

3. Either run `target\release\procmon-mcp-setup.exe` as in
   [Download the setup](#download-the-setup), or install from an **administrator**
   terminal without the setup:

   ```powershell
   .\target\release\procmon-mcp.exe install
   ```

   Before asking for consent, `install` lists everything it will do:

   - create `C:\Program Files\procmon-mcp` for the programs, and
     `C:\ProgramData\ProcmonMcp` (`jobs`, `out`), writable only by administrators,
     except that you may add requests to `jobs` and read or delete results in `out`
   - copy itself to `C:\Program Files\procmon-mcp\procmon-mcp.exe`
   - download `https://live.sysinternals.com/Procmon64.exe` (`Procmon64a.exe` on ARM64)
     to `C:\Program Files\procmon-mcp\`, keeping it only if its Microsoft signature is
     valid
   - register the scheduled task `ProcmonMcpCapture`, which runs a capture with
     administrator rights when asked

   Process Monitor is started with `/AcceptEula`, which accepts the
   [Sysinternals Software License Terms](https://learn.microsoft.com/sysinternals/license-terms)
   on your behalf; `install` asks you to accept them (`--accept-eula` skips the question).

### Add it to Claude Desktop

Add the server to Claude Desktop's `claude_desktop_config.json`
(Settings → Developer → Edit Config) and restart Claude Desktop:

```json
{
  "mcpServers": {
    "procmon": {
      "command": "C:\\Program Files\\procmon-mcp\\procmon-mcp.exe",
      "args": ["mcp"]
    }
  }
}
```

### Repairing and who can install

Running the setup or `install` again repairs or refreshes an installation. Quit Claude
Desktop first, since it keeps `procmon-mcp.exe` in use.

Only an administrator who is signed in can install: the capture task runs as that user
with their administrator rights. Elevating with another account's credentials is refused.

## Tools

### `capture`

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `duration_s` | integer, 1–120 | 10 | Seconds to capture |
| `process_name` | string | | Image name, e.g. `HuionTablet.exe` |
| `pid` | integer | | Process ID |
| `operations` | string[] | | Operations, e.g. `["ReadFile", "RegSetValue"]` |
| `path_contains` | string | | Case-insensitive substring of the path |
| `result` | string | | Result, e.g. `NAME NOT FOUND` |
| `top_n` | integer, 1–50 | 15 | Entries per section |
| `keep_log` | boolean | false | Keep the `.pml` and return its path |

Narrow the capture whenever you can: without conditions Process Monitor records the whole
system, which easily reaches hundreds of MB in a few seconds.

Only one Process Monitor can capture at a time. If Process Monitor is already open, the
tool returns an error asking you to close it; it never closes it for you.

### `summarize_log`

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `pml_path` | string | (required) | A `.pml` file; other files are refused |
| `process_name`, `pid`, `operations`, `path_contains` | | | As for `capture` |
| `top_n` | integer, 1–50 | 15 | Entries per section |

### Output

Both tools return JSON (declared as the tool's `outputSchema`):

- event count, duration and events per second
- event counts per process and per operation
- the most frequent operation and path pairs, with bytes and MiB/s for `ReadFile` and
  `WriteFile`
- files opened 5 or more times by the same process, with the mean, minimum and maximum
  interval in ms
- failed operations with their NTSTATUS (`END OF FILE` is not counted as a failure)

Each list also gives how many distinct entries existed before it was cut to `top_n`.

## Command line

```text
procmon-mcp install [--accept-eula]   set up captures (administrator)
procmon-mcp update                    get a newer Process Monitor, if any (administrator)
procmon-mcp uninstall                 remove everything install set up (administrator)
procmon-mcp mcp                       run the MCP server on stdin/stdout
procmon-mcp summarize <file.pml> [--process <name>] [--pid <pid>] [--operation <name>]...
                                 [--path-contains <text>] [--top <n>] [--json]
```

`update` asks Sysinternals only whether `Procmon64.exe` changed (a conditional request);
nothing is downloaded when it has not. `summarize` prints the same summary as text, or as
the MCP JSON with `--json`.

## How it works

```text
Claude Desktop ──stdio──> procmon-mcp.exe mcp        (normal rights)
                            │ writes jobs\request.json, starts the task (COM)
                            ▼
                 scheduled task ProcmonMcpCapture
                            ▼
                 procmon-mcp.exe runner             (elevated)
                   ├ checks directory permissions and Procmon's signature
                   ├ validates the request, writes the filter (.pmc)
                   ├ saves your Process Monitor settings
                   ├ Procmon64.exe /Quiet /BackingFile out\<id>\capture.pml /LoadConfig … /Runtime N
                   └ restores your settings, writes out\<id>\status.json
                            ▼
                 procmon-mcp.exe mcp reads the log, summarizes it, deletes it
```

Security notes:

- The elevated runner only ever starts the `Procmon64.exe` in the administrator-only
  `C:\Program Files\procmon-mcp`, and only if it is signed by Microsoft.
- It refuses to run if any directory's owner or permissions differ from what `install`
  set.
- It reads one small request file from `jobs`, which accepts only a hex ID, a duration of
  1–120 s and bounded filter text.
- It writes only to `out`, where you can read and delete but not create files, so nothing
  can be planted to redirect its writes.
- System folders come from Windows APIs, not from environment variables.

## Uninstall

Use **Settings → Apps → Installed apps → procmon-mcp → Uninstall**, or, from an
administrator terminal:

```powershell
& "C:\Program Files\procmon-mcp\procmon-mcp.exe" uninstall
```

Either removes the scheduled task, the Installed apps entry, `C:\Program Files\procmon-mcp`
and `C:\ProgramData\ProcmonMcp`.
Quit Claude Desktop first; programs still running are moved aside and deleted when Windows
restarts. Remove the `procmon` entry from Claude Desktop's configuration as well.

## Limitations

- Reads PML format version 9, written by current Process Monitor releases (tested with
  4.13); other versions are refused with an explicit error.
- Network events are counted but their path (addresses and ports) is not decoded yet.
- A capture takes about 7 seconds longer than requested (task start, driver setup,
  writing the log).

## Development

The repository is a workspace: the server (`procmon-mcp`, the root package) and the
graphical setup (`setup/`, `procmon-mcp-setup`), which embeds the server. See
[Build it yourself](#build-it-yourself) for the build order. CI runs these checks on
every push, and pushing a `v*` tag drafts a release with the setup attached:

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo deny --workspace check
```

The setup uses [windows-reactor](https://github.com/microsoft/windows-rs) (WinUI 3),
pinned to a commit until a release with the APIs it needs is published.

Some tests compare against real logs and binaries in the git-ignored `reference/`
directory and are skipped when the files are absent. The development workflow is
described in [CLAUDE.md](CLAUDE.md) and [docs/PLAN.md](docs/PLAN.md) (Japanese).

## Acknowledgements

This project stands on the shoulders of
[**procmon-parser**](https://github.com/eronnen/procmon-parser) by Ely Ronnen.

Process Monitor's PML (log) and PMC (configuration) formats are not officially documented.
procmon-parser reverse-engineered them and published the results as documentation and a
Python library. The PML and PMC code here is a Rust reimplementation based on that work.
Huge thanks to its author and contributors.

procmon-parser is licensed under the MIT License. See
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

This project is not affiliated with or endorsed by Microsoft or Sysinternals.

## License

[MIT](LICENSE)
