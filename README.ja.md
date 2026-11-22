# procmon-mcp-rs

[English](README.md)

Claude などの MCP クライアントから
[Sysinternals Process Monitor](https://learn.microsoft.com/sysinternals/downloads/procmon)
を数秒間だけ動かし、プロセスが何をしているかを集計して返す MCP サーバーです。
どのファイルやレジストリキーに、どのくらいの頻度でアクセスし、どれだけ読み書きしているか、
どのファイルを定期的に開き直しているか、どの操作が失敗しているかがわかります。

> 「HuionTablet.exe を Procmon で15秒間取得して、何をしているか教えて」
>
> → *`C:\Program Files\HuionTablet\res\user.cfg` を約312msごとに読み直しています（約14.6 MiB/s）。
> そのたびに存在しない `.cfg` を開こうとして、NAME NOT FOUND で失敗しています。*

> Windows 11 と Process Monitor 4.13 で動作を確認しています。
> 設計と判断の理由は [docs/PLAN.md](docs/PLAN.md) にあります。

## 機能

- **`capture`**: 指定した条件（プロセス名、PID、Operation、Path に含まれる文字列、Result）で
  Process Monitor を1〜120秒間動かし、集計結果を構造化データで返します。条件は Process Monitor
  自身が適用するので、記録されるのは条件に合うイベントだけです。ログは、残すよう指定しない限り削除します。
- **`summarize_log`**: 保存済みの `.pml` を、プロセス、PID、Operation、Path で絞り込んで集計します。
- **exe 1つで動く**: Rust の単一の実行ファイルです。Python、.NET、非同期ランタイムは不要で、
  HTTP や TLS の実装も同梱していません（ダウンロードには Windows の WinHTTP を使います）。
- **最小限の権限**: MCP サーバーは通常権限で動きます。管理者権限で起動するのは Process Monitor だけで、
  `install` で一度だけ設定するタスクスケジューラのタスクを経由します。
- **Process Monitor は同梱しない**: `install` が Sysinternals Live から取得します。
  Microsoft の署名を、インストール時と毎回の取得の前に確認します。
- **Process Monitor の設定を書き換えない**: Process Monitor は、取得に使った設定を GUI の設定として
  保存してしまいます。procmon-mcp は取得のたびに元の設定に戻します。

## 必要なもの

- Windows 10 または 11（x64 または ARM64）
- インストールするときに、管理者としてサインインしていること（1回だけ）
- Claude Desktop などの MCP クライアント
- セットアップを使う場合は
  [Windows App Runtime 2.x](https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads)

## インストール

### セットアップをダウンロードする

1. [最新のリリース](https://github.com/ndac-todoroki/procmon-mcp-rs/releases/latest)から
   `procmon-mcp-setup.exe` をダウンロードします。MCP サーバーも中に入っているので、必要なファイルはこれだけです。
   ダウンロードしたファイルを確かめるには、次のハッシュを同じリリースの `SHA256SUMS.txt` と比べます。

   ```powershell
   Get-FileHash .\procmon-mcp-setup.exe -Algorithm SHA256
   ```

2. 実行します。セットアップにはコード署名がないため、**「Windows によって PC が保護されました」** と
   表示されることがあります。その場合は **[詳細情報]** を選び、**[実行]** を選びます。

3. 画面の案内に従います。何を変更するかの説明と、Sysinternals のライセンスへの同意を経て、
   **[インストール]** を選んだときにだけ管理者権限を求めます。最後の画面には、Claude Desktop の設定を
   コピーするボタンがあります（[Claude Desktop に追加する](#claude-desktop-に追加する)を参照）。

セットアップは自身のコピーを残すので、**設定 → アプリ → インストールされているアプリ** から
procmon-mcp を削除できます。セットアップには Windows App Runtime 2.x が必要です（「必要なもの」を参照）。
MCP サーバーには不要です。リリースのファイルは x64 向けにビルドしています。

### 自分でビルドする

1. [Rust](https://rustup.rs/) 1.95 以降（MSVC のツールチェーン）と、Visual Studio Build Tools
   （**C++ によるデスクトップ開発**。Windows SDK を含む）をインストールします。

2. サーバーをビルドしてから、セットアップをビルドします。セットアップはサーバーを埋め込むので、
   この順番が必要です。

   ```powershell
   cargo build --release                        # target\release\procmon-mcp.exe
   cargo build --release -p procmon-mcp-setup   # target\release\procmon-mcp-setup.exe
   ```

3. `target\release\procmon-mcp-setup.exe` を[セットアップをダウンロードする](#セットアップをダウンロードする)と
   同じように実行します。セットアップを使わない場合は、**管理者として開いた**ターミナルで次を実行します。

   ```powershell
   .\target\release\procmon-mcp.exe install
   ```

   `install` は、同意を求める前に、次の内容をすべて表示します。

   - プログラム用の `C:\Program Files\procmon-mcp` と、`C:\ProgramData\ProcmonMcp`（`jobs`、`out`）を
     作成します。書き込めるのは管理者だけです。ただし、あなたは `jobs` に依頼を書き込むことと、
     `out` の結果を読んだり削除したりすることができます。
   - 自身を `C:\Program Files\procmon-mcp\procmon-mcp.exe` にコピーします。
   - `https://live.sysinternals.com/Procmon64.exe`（ARM64 では `Procmon64a.exe`）を
     `C:\Program Files\procmon-mcp\` にダウンロードします。Microsoft の署名が有効な場合だけ使います。
   - タスクスケジューラにタスク `ProcmonMcpCapture` を登録します。依頼があったときに、
     管理者権限で取得を実行します。

   Process Monitor は `/AcceptEula` を付けて起動します。これは、あなたの代わりに
   [Sysinternals のライセンス条項](https://learn.microsoft.com/sysinternals/license-terms)
   に同意することを意味するため、`install` で同意を求めます（`--accept-eula` を付けると質問を省略します）。

### Claude Desktop に追加する

Claude Desktop の `claude_desktop_config.json`（設定 → 開発者 → 構成を編集）にサーバーを追加し、
Claude Desktop を再起動します。

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

### 修復と、インストールできる人

セットアップや `install` をもう一度実行すると、インストールを修復または更新します。Claude Desktop が
`procmon-mcp.exe` を使用中だと置き換えられないので、先に Claude Desktop を終了してください。

インストールできるのは、管理者としてサインインしている場合だけです。取得用のタスクは、その利用者の
管理者権限で動くためです。別のアカウントの資格情報で昇格した場合は中止します。

## ツール

### `capture`

| 引数 | 型 | 既定値 | 意味 |
|---|---|---|---|
| `duration_s` | 整数（1〜120） | 10 | 取得する秒数 |
| `process_name` | 文字列 | | イメージ名（例: `HuionTablet.exe`） |
| `pid` | 整数 | | プロセス ID |
| `operations` | 文字列の配列 | | Operation（例: `["ReadFile", "RegSetValue"]`） |
| `path_contains` | 文字列 | | Path に含まれる文字列（大文字小文字は区別しない） |
| `result` | 文字列 | | Result（例: `NAME NOT FOUND`） |
| `top_n` | 整数（1〜50） | 15 | 各セクションに含める件数 |
| `keep_log` | 真偽値 | false | `.pml` を残し、そのパスを返す |

できるだけ条件で絞り込んでください。条件がないとシステム全体を記録するため、数秒で数百 MB に
達することがあります。

同時に取得できる Process Monitor は1つだけです。Process Monitor が既に開いている場合は、
閉じるよう求めるエラーを返します。開いている Process Monitor を勝手に閉じることはありません。

### `summarize_log`

| 引数 | 型 | 既定値 | 意味 |
|---|---|---|---|
| `pml_path` | 文字列 | （必須） | `.pml` ファイル。それ以外のファイルは受け付けません |
| `process_name`、`pid`、`operations`、`path_contains` | | | `capture` と同じ |
| `top_n` | 整数（1〜50） | 15 | 各セクションに含める件数 |

### 出力

どちらのツールも JSON を返します（ツールの `outputSchema` として宣言しています）。

- イベント数、期間、1秒あたりの件数
- プロセス別、Operation 別のイベント数
- 回数の多い Operation と Path の組み合わせ（`ReadFile` と `WriteFile` は合計バイト数と MiB/s 付き）
- 同じプロセスが5回以上開いたファイルと、開く間隔の平均・最小・最大（ms）
- 失敗した操作と、その NTSTATUS（`END OF FILE` は失敗に含めません）

各一覧には、`top_n` で切る前の件数も含めます。

## コマンドライン

```text
procmon-mcp install [--accept-eula]   取得の準備をする（管理者）
procmon-mcp update                    新しい Process Monitor があれば取得する（管理者）
procmon-mcp uninstall                 install で設定したものをすべて削除する（管理者）
procmon-mcp mcp                       標準入出力で MCP サーバーを動かす
procmon-mcp summarize <file.pml> [--process <name>] [--pid <pid>] [--operation <name>]...
                                 [--path-contains <text>] [--top <n>] [--json]
```

`update` は、`Procmon64.exe` が変わったかどうかだけを Sysinternals に問い合わせます（条件付きリクエスト）。
変わっていなければ何もダウンロードしません。`summarize` は同じ集計結果を文章で表示します。
`--json` を付けると、MCP と同じ JSON を表示します。

## 仕組み

```text
Claude Desktop ──stdio──> procmon-mcp.exe mcp        （通常権限）
                            │ jobs\request.json を書き、タスクを起動（COM）
                            ▼
                 タスク ProcmonMcpCapture
                            ▼
                 procmon-mcp.exe runner             （管理者権限）
                   ├ フォルダーの権限と Procmon の署名を確認
                   ├ 依頼を検証し、フィルター（.pmc）を書き出す
                   ├ Process Monitor の設定を保存
                   ├ Procmon64.exe /Quiet /BackingFile out\<id>\capture.pml /LoadConfig … /Runtime N
                   └ 設定を元に戻し、out\<id>\status.json を書く
                            ▼
                 procmon-mcp.exe mcp がログを読んで集計し、削除する
```

セキュリティについて:

- 管理者権限で動く runner が起動するのは、管理者だけが書き込める `C:\Program Files\procmon-mcp` にある `Procmon64.exe` だけです。
  しかも、Microsoft の署名がある場合に限ります。
- フォルダーの所有者や権限が `install` で設定したものと異なる場合は、実行を拒否します。
- `jobs` から読むのは小さな依頼ファイル1つだけです。受け付けるのは、16進の ID、1〜120秒の秒数、
  長さを制限したフィルターの文字列だけです。
- 書き込むのは `out` だけです。`out` では、ユーザーは読み取りと削除はできますが、ファイルを作れません。
  そのため、書き込み先をすり替えるものを置かれることはありません。
- システムのフォルダーの場所は、環境変数ではなく Windows の API から取得します。

## アンインストール

**設定 → アプリ → インストールされているアプリ → procmon-mcp → アンインストール** を使うか、
管理者として開いたターミナルで次を実行します。

```powershell
& "C:\Program Files\procmon-mcp\procmon-mcp.exe" uninstall
```

どちらも、タスク、「インストールされているアプリ」の項目、`C:\Program Files\procmon-mcp`、`C:\ProgramData\ProcmonMcp` を削除します。
先に Claude Desktop を終了してください。実行中のプログラムは別の場所へ移し、Windows の再起動時に削除します。
Claude Desktop の設定から `procmon` の項目も削除してください。

## 制限事項

- 読めるのは PML の形式バージョン9（現在の Process Monitor が書き出す形式。4.13 で確認）です。
  それ以外のバージョンは、明確なエラーで拒否します。
- ネットワークのイベントは数えますが、Path（アドレスとポート）はまだ解読しません。
- 取得には、指定した秒数より約7秒長くかかります（タスクの起動、ドライバーの準備、ログの書き出し）。

## 開発

リポジトリはワークスペースです。サーバー（`procmon-mcp`、ルートのパッケージ）と、サーバーを埋め込む
セットアップ（`setup/`、`procmon-mcp-setup`）からなります。ビルドの順番は[自分でビルドする](#自分でビルドする)を
参照してください。push のたびに CI が次の確認を行い、`v*` のタグを push するとセットアップを添付したリリースの下書きを作ります。

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo deny --workspace check
```

セットアップは [windows-reactor](https://github.com/microsoft/windows-rs)（WinUI 3）を使います。
必要な API を含む版が公開されるまで、コミットを固定して使っています。

一部のテストは、git の管理外の `reference/` フォルダーにある実際のログや実行ファイルと比較します。
ファイルがない場合、それらのテストはスキップします。開発の進め方は [CLAUDE.md](CLAUDE.md) と
[docs/PLAN.md](docs/PLAN.md) にあります。

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
