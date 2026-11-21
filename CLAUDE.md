# CLAUDE.md（procmon-mcp-rs）

設計と決定事項は `docs/PLAN.md` にある。作業前に読むこと。

## 言語

- コードコメント、テスト名、コミットメッセージ: 英語
- `README.md` は英語、`README.ja.md` は日本語。片方を変えたらもう片方も合わせる
- `docs/` 以下: 日本語

## 進め方

- TDD（t-wada 氏のプラクティス）。テストリスト → Red → Green → Refactor
- commit-as-you-go。1つのテストを通したら commit する粒度を目安にする
- 依存クレートを足すときは理由を説明して了承を得る。足したら `cargo deny check` を通す

## 参照ファイル

- 実ログ・設定・旧実装は `reference/`（`.gitignore` 済み）。内容は `docs/PLAN.md` の「正解データ」を参照
- `reference/` のファイルを使うテストは、ファイルがなければスキップする
- worktree で作業する場合、`reference/` はメインのチェックアウト側にしかない。
  テストは環境変数 `PROCMON_MCP_REFERENCE` でフォルダーを上書きできるようにする（既定はリポジトリ直下の `reference/`）

## 形式の参照元

PML / PMC の形式は procmon-parser（https://github.com/eronnen/procmon-parser）のドキュメントとソースに基づく。
公式の仕様はない。参照した箇所はコメントに残す。
