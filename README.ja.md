# procmon-mcp-rs

[English](README.md)

Claude などの MCP クライアントから
[Sysinternals Process Monitor](https://learn.microsoft.com/sysinternals/downloads/procmon)
を数秒間だけ動かし、プロセスが何をしているか（どのファイルを、どのくらいの頻度・量で読み書きしているか、
どの操作が失敗しているか）を集計して返す MCP サーバーです。

> **状態:** 開発中です。設計は [docs/PLAN.md](docs/PLAN.md) を参照してください。

## 機能（予定）

- `capture`: 呼び出しごとに条件（プロセス、PID、Operation、Path、Result、秒数）からフィルターを作って
  Process Monitor で取得し、集計結果を返します。ログは一時ファイルで、取得後に削除します。
- `summarize_log`: 保存済みの `.pml` を、条件を変えて再集計します。
- 依存の少ない、Rust の単一 exe です。MCP サーバーは通常権限で動き、タスクスケジューラ経由で
  Process Monitor だけを昇格して起動します。
- Process Monitor 本体は同梱しません。Sysinternals Live から取得し、使うたびに Authenticode 署名を確認します。

## 謝辞

このプロジェクトは、Ely Ronnen 氏による
[**procmon-parser**](https://github.com/eronnen/procmon-parser) の成果の上に成り立っています。

Process Monitor の PML（ログ）と PMC（設定）の形式は、公式には公開されていません。
procmon-parser はこれらを解析し、ドキュメントと Python ライブラリとして公開しました。
本プロジェクトの PML と PMC の処理は、その成果をもとに Rust で再実装したものです。
作者とコントリビューターの皆さまに深く感謝します。

procmon-parser は MIT License で公開されています。
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) を参照してください。

本プロジェクトは Microsoft および Sysinternals とは関係がなく、承認を受けたものでもありません。

## ライセンス

[MIT](LICENSE)
