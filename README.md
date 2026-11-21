# procmon-mcp-rs

[日本語](README.ja.md)

An MCP server that lets Claude (or any MCP client) run
[Sysinternals Process Monitor](https://learn.microsoft.com/sysinternals/downloads/procmon)
for a few seconds and summarize what a process is doing: which files it reads and writes,
how often, how much, and which operations fail.

> **Status:** work in progress. See [docs/PLAN.md](docs/PLAN.md) (Japanese) for the design.

## Features (planned)

- `capture`: run Process Monitor with a filter built per call (process, PID, operation,
  path, result, duration) and return a summary. Logs are temporary and deleted afterwards.
- `summarize_log`: re-summarize a saved `.pml` file with different conditions.
- A single Rust executable with a small dependency tree. The MCP server runs with normal
  privileges; only Process Monitor is started elevated, through a scheduled task.
- Process Monitor itself is not bundled. It is downloaded from Sysinternals Live and its
  Authenticode signature is verified before every use.

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
