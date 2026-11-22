# procmon-mcp-rs 実装計画

## サマリー

Claude Desktop から「プロセス X を N 秒 Procmon で取得して、何をしているか教えて」と頼める MCP サーバーを、Rust の単一 exe として作る。

- MCP サーバーは通常権限で動き、取得だけを昇格したタスク（同じ exe の `runner` サブコマンド）に任せる。
- Procmon は Microsoft の配布元から自動で入手し、Authenticode 署名で検証する。
- 取得時のフィルター（.pmc）は Rust で生成する。GUI 既定の除外ルールをテンプレートとして持ち、呼び出しごとの条件を足す。
- PML の読み出しと集計は Rust で実装する。形式は [procmon-parser](https://github.com/eronnen/procmon-parser) のドキュメントに基づく。
- 依存クレートは最小限にする（MCP 層は `serde_json` で自作し、tokio を使わない）。

元の要件は `reference/procmon-mcp-rs-brief.md`（コミット対象外）にある。本書はその後の検討で決めた内容で、食い違う場合は本書を優先する。

## 決定事項

| 項目 | 決定 | 理由 |
|---|---|---|
| 昇格側の実装 | exe の `runner` サブコマンド（PowerShell の `runner.ps1` は使わない） | 入力検証と PMC 生成を昇格側で行い、ユーザーが書き込める場所を昇格側が読み書きしないため |
| Procmon の入手 | `install` 時に `https://live.sysinternals.com/Procmon64.exe`（ARM64 は `Procmon64a.exe`）を取得 | 常に配布元の最新版を使い、パス指定を不要にするため |
| Procmon の検証 | `WinVerifyTrust` で署名が有効、かつ署名者が Microsoft Corporation であること。`install` / `update` 時と、毎回の起動前に確認 | 公式のハッシュ値は公開されていない。署名は Microsoft がハッシュに署名したものなので、ハッシュ照合より強い |
| Procmon の更新 | `update` コマンドで明示的に行う。前回の `ETag` を使った条件付き GET で、変更がなければ本体を取得しない（`304 Not Modified`） | バージョン番号を事前に知るにはスクレイピングが必要になるため避ける。取得のたびに通信しない。昇格側でダウンロードしない |
| フィルター | 最初から PMC を生成する。GUI 既定の除外ルールをテンプレートにし、`DestructiveFilter=1` を設定する | 取得時に絞り込み、ログを小さくするため。`DestructiveFilter=0` だと除外したイベントも PML に書き込まれる |
| MCP 層 | `serde_json` で JSON-RPC over stdio を自作する（`Value` で扱い、`serde` の derive は使わない） | 同時に処理するのは2〜3件だけで、`std::thread` で足りる。rmcp + tokio は73クレート |
| MCP のバージョン | 2026-07-28（リクエストごとに `_meta` でバージョンを送る方式）と、2025-11-25 以前（`initialize` で始める方式）の両方に応じる | 仕様の後方互換の節で認められている方式。Claude Desktop がどちらで話すかに依存しないため |
| モデルへの案内 | `capture` の説明文に「いつ使うか」（ファイルやレジストリの読み書き、ディスクを使い続ける原因、開けない・保存できない理由）を書く。サーバー全体の進め方（プロセスで絞る、記録中に利用者が操作を再現する、`keep_log` と `summarize_log`）は `instructions` に書き、2026-07-28 は `server/discover`、それ以前は `initialize` で返す | 「procmon を使って」と言われなくても使えるようにするため。ツールの説明文はどのクライアントでもモデルに届くが、`instructions` をシステムプロンプトに入れるかはクライアント次第なので、使う判断に要ることは説明文にも書く。仕様に従い、`instructions` ではツールの説明文を繰り返さない |
| 集計結果の言語 | 英語のみ（切り替えなし） | 読むのは Claude で、ユーザーの言語で説明し直すため |
| 集計結果の形式 | MCP では JSON（`outputSchema` を宣言し、`structuredContent` と、同じ JSON を文字列にした `content` を返す）。CLI の `summarize` は文章で、`--json` で JSON | 数値が曖昧にならず、項目を足しても壊れない。生の値と計算済みの値を両方入れ、各セクションに `top_n` で切る前の件数（`distinct`）を付ける |
| Windows API | `windows` クレートで行う（署名の確認、昇格の判定、ACL の設定と確認）。タスクの登録・起動・削除はタスクスケジューラの COM（ITaskService）で、定義は XML の文字列で渡す。unsafe なコードは `src/win/` にまとめる | 当初は `windows-sys` と `schtasks.exe` だったが見直した。実測でバイナリサイズの差は数 KB。`Result` を返す関数、`Owned` によるハンドルの自動解放、エラー型が使え、COM でタスクを直接扱えるので外部プロセスと XML ファイルの書き出しが不要になる。代わりに proc-macro 系（syn など）の依存とビルド時間が増える |
| HTTP | `nyquest` + `nyquest-backend-winhttp`（WinHTTP を使う。依存は `windows-sys`） | 自前の unsafe な WinHTTP コード（約150行）をなくすため。若いクレート（0.5 / 0.2）で依存も増える（約17クレート、+55KB）点は承知のうえで採用 |
| EULA の同意 | `install` で Sysinternals のライセンスの URL を示し、y/N で尋ねる。`--accept-eula` で省略できる | `/AcceptEula` はユーザーの代わりに同意することになるため |
| PML パーサー | procmon-parser のドキュメントに基づいて、必要な部分だけを実装する。実ファイルとの突き合わせは最低限にする | 公式の仕様はない。procmon-parser が事実上の参照実装 |
| インストーラー | `install` / `uninstall` は最初から CLI で作り、GUI は最後のフェーズで作る | GUI は外側だけ。作ったものをすべて記録し、アンインストールできる設計を先に固める |
| GUI インストーラーの方式 | WinUI 3 を `windows-reactor`（windows-rs の宣言的 UI ライブラリ）で作る。Windows App Runtime 2.x はインストール済みを前提にする（framework-dependent） | TaskDialog や Aero Wizard より現在の Windows の見た目に近く、ライト/ダーク、Mica、アクセシビリティが標準で付く。ランタイムを同梱すると約120個のファイルになり、exe 1つで配れなくなる |
| `windows-reactor` の版 | 0.101（crates.io 未公開）を git のコミット固定で使う | 0.100 にはフォントを指定する API がなく、パスを等幅で表示できない |
| インストール先 | プログラム（`procmon-mcp.exe`、`Procmon64.exe`、セットアップ、状態の `state.json`）は `C:\Program Files\procmon-mcp`、依頼と結果（`jobs`、`out`）は `C:\ProgramData\ProcmonMcp` | 当初はすべて ProgramData に置いていた。Program Files が Windows の慣習どおりのプログラムの置き場所で、AppLocker の既定のルールなどは Windows と Program Files 以下の実行だけを許すため、ProgramData の exe は止められることがある。依頼と結果はデータなので ProgramData に残す。どちらのフォルダーも自前の ACL を設定し、runner が毎回確認する |
| GUI の配布形態 | 利用者がダウンロードするのは `procmon-mcp-setup.exe` の1つだけ。本体の `procmon-mcp.exe` を埋め込み、インストール時に `C:\Program Files\procmon-mcp` へ書き出す。セットアップ自身もそこにコピーし、アンインストーラーとして「設定 → アプリ」に登録する（HKLM の Uninstall キー） | 本体（MCP サーバー）に WinUI のランタイム依存を持ち込まないため。インストール後にセットアップを消されてもアンインストールできるようにするため |
| GUI の昇格 | 画面は通常権限で動かし、[インストール] を押したときに、画面のない作業用プロセスを昇格して起動する | ウォークスルーの間は UAC を出さないため |
| 対象の利用者 | サインインしている利用者自身が管理者の場合だけに対応する。昇格したアカウントとサインイン中の利用者が違う（別アカウントの資格情報で昇格した）ときは、CLI も GUI も中止する | タスクは利用者として「最上位の特権」で動くため、標準ユーザーでは昇格せず、別の管理者として動かすとその人がサインインしていないと起動できない。標準ユーザーへの対応（タスクを SYSTEM で動かす）は、使いたいのが管理者だけなので見送る |
| GUI の画面構成 | ようこそ → ライセンス → 確認 → インストール → 完了の一本道。戻るはタイトルバー左上（WinUI の TitleBar）、操作ボタンは右下の1つだけ。キャンセルボタンは置かない（閉じるボタンで足りる） | 現在の WinUI の慣習に合わせるため。キャンセルは押し間違えやすいため |
| GUI の言語 | Windows の表示言語に合わせて切り替える（まずは英語と日本語）。パスや識別子は翻訳せず、常に等幅フォント（Cascadia Mono）で表示する | 日本語環境のプロポーショナルフォントでは `\` が `¥` で表示されるため |
| CI とリリース | GitHub Actions。push と PR では Windows で fmt、clippy、テストを、Linux で `cargo deny` を実行する。`v*` のタグの push（または手動実行）で、サーバー → セットアップの順に release ビルドし、`procmon-mcp-setup.exe` と `SHA256SUMS.txt` を添付した Release を**下書き**で作る。公開は人が確認してから行う。Action はコミットの SHA で固定する。コード署名はしない | ビルドを手元の環境に依存させないため。下書きにするのは、利用者がダウンロードする前に人が中身を見るため。タグの付け替えで実行内容が変わらないよう SHA で固定する。署名には証明書（有料、または審査あり）が要るので見送り、SmartScreen の警告の越え方を README に書く |
| SDDL の比較 | runner はフォルダーから読んだ SDDL と、期待する SDDL を Windows に一度通した形（`canonical_sddl`）とで比べる | Windows は一部の SID を略号で書く（組み込みの Administrator は `LA`）。文字列のまま比べると、組み込みの Administrator でインストールした場合に取得できなかった（1.0.0 まで。CI が組み込みの Administrator で動くことで見つかった） |
| ライセンス | MIT | procmon-parser（MIT, Copyright (c) 2020 Ely Ronnen）に合わせる |

## 構成

```
Claude Desktop ──stdio──> procmon-mcp.exe（通常権限、MCP サーバー）
                              │ jobs\request.json を書く（ID と秒数、条件）
                              ▼
                  タスク ProcmonMcpCapture を起動（COM）
                              ▼
        procmon-mcp.exe runner（最上位の特権）
            ├ request を検証し、PMC を生成（out\<id>\filter.pmc）
            ├ Procmon64.exe の署名を確認
            └ Procmon64.exe /AcceptEula /Quiet /Minimized
                  /BackingFile out\<id>\capture.pml /LoadConfig out\<id>\filter.pmc /Runtime <秒>
                              ▼
          MCP サーバーが PML を読んで集計 → 結果を返す → 一時ファイルを削除
```

### 置き場所

| パス | 書き込み | 内容 |
|---|---|---|
| `C:\Program Files\procmon-mcp\` | 管理者のみ | `procmon-mcp.exe`、`Procmon64.exe`、`procmon-mcp-setup.exe`、取得時の `ETag` などの状態 |
| `C:\ProgramData\ProcmonMcp\jobs\` | ユーザー | `request.json`（MCP サーバーから昇格側への依頼） |
| `C:\ProgramData\ProcmonMcp\out\<id>\` | 管理者のみ（完成後にユーザーへ読み取りを許可） | `filter.pmc`、`capture.pml`、`done` / `error` |

## セキュリティ設計

- 昇格側が実行するのは、`C:\Program Files\procmon-mcp\`（管理者のみ書き込み可）にある、署名を確認した exe だけ。PATH の検索や、引数で渡されたパスは使わない。
- 昇格側は、ユーザーが書き込める場所に書き込まない。ジャンクションによる任意ファイル上書き（Python 版にあった問題）を防ぐため。
- 昇格側が受け付ける入力は検証する。
  - ID は32桁の16進、秒数は1〜120の整数。
  - フィルター条件は、決まった列・関係の組み合わせと、長さを制限した文字列だけ。
- `summarize_log` は `.pml` 以外を拒否する。
- 依存クレートはバージョンを固定し、`cargo deny` を通す。
- 管理者権限が必要な操作（`install`、`uninstall`、タスクの登録、ACL の変更）を開発中に実行するときは、事前に内容を説明して了承を得る。
- `/AcceptEula` を付けるため、`install` 時に Sysinternals のライセンスを示して同意を得る。

## フェーズ

| フェーズ | 内容 | 確認方法 |
|---|---|---|
| 0 | リポジトリの骨組み、README（procmon-parser への言及と謝辞）、LICENSE（MIT）、`THIRD_PARTY_NOTICES`、`cargo deny` | `cargo build` と `cargo deny check` |
| 1 | PML の読み出しと集計。CLI の `procmon-mcp summarize <pml>` で完結させる | `Logfile.PML` と正解データ（下記） |
| 2 | MCP 層（自作）と `summarize_log` ツール | JSON の入出力の単体テストと、Claude Desktop からの呼び出し |
| 3 | PMC の読み書き、既定テンプレート、条件からのルール生成 | `ProcmonConfiguration.pmc` の読み込みと書き戻しでバイト一致。フィルター適用結果の照合（下記） |
| 4 | `install` / `uninstall` / `update` / `runner`、ダウンロードと署名確認、`capture` ツール | 実機で確認（管理者権限の操作は事前に了承を得る） |
| 5 | GUI のインストーラー（WinUI 3、決定事項を参照）。Claude Desktop への登録は、完了画面で設定の JSON をコピーするボタンにする | 画面は自動操作で確認済み。UAC を伴うインストールとアンインストール（Program Files への配置を含む）は実機で確認済み（2026-11-22） |

開発は TDD（t-wada 氏のプラクティス）で進め、commit-as-you-go を守る。

## 正解データ

参照ファイルは `reference/` に置く（`.gitignore` 済み）。テストはファイルがない場合にスキップする。

| ファイル | 内容 |
|---|---|
| `Logfile.PML` | `HuionTablet.exe` のみを Include して約13秒取得したもの |
| `Logfile_default_nofilter.PML` | 既定のフィルターのみの状態で取得し、All events で保存したもの |
| `Logfile_default_filter.PML` | 同上を、Events displayed using current filter で保存したもの |
| `ProcmonConfiguration.pmc` | 既定のフィルターのみの状態で書き出した設定（Procmon 4.13） |
| `procmon-mcp-rs-brief.md` | 元の要件メモ |
| `procmon-mcp.zip` | Python 版の参考実装 |

### `Logfile.PML` の集計結果（Linux 上の Python 版 procmon-parser 0.4.0 による）

- イベント総数14,266（うち `Process_Profiling` が7,840。除いた6,426が集計対象）、期間は約12.8〜13.0秒
- Operation 別: ReadFile 6,300 / CreateFile 84 / CloseFile 42
- `C:\Program Files\HuionTablet\res\user.cfg` の ReadFile: 6,300回、合計195,454,056バイト（186.4 MiB）、約14.56 MiB/s
  - 読み取りサイズは4,096が3,108回、57,344が2,646回、61,440が504回、612が42回
- `user.cfg` の CreateFile: 42回。間隔は平均312ms（最小約284、最大約329）
- `C:\Program Files\HuionTablet\.cfg` の CreateFile: 42回。すべて NAME NOT FOUND（0xC0000034）
- `user.cfg` の ReadFile の最後の読み取り（42回）は 0xC0000011（END OF FILE）で、正常

### フィルターの解釈の照合

`Logfile_default_nofilter.PML` に、Rust で実装した既定テンプレートのフィルターを適用した結果が、`Logfile_default_filter.PML` のイベントと一致することを確認する。

結果（`tests/filter_reference.rs`）:

- Profiling 以外のイベントは、順番も含めて完全に一致した（373,458件）。
- Procmon は「Events displayed using current filter」で保存するとき、除外ルールがあっても `Process Profiling` を**すべて**残す（9,057件。Procmon64.exe や System の分も含む）。
- 集計では Profiling を除外しているので、結果には影響しない。`DestructiveFilter=1` で取得したログにも Profiling が残るかは、フェーズ4で確認する。

## 集計の内容（Python 版 `analysis.py` と同じ）

- イベント数、期間、1秒あたりの件数
- プロセス別、Operation 別
- Operation × Path（回数順）。ReadFile / WriteFile は合計容量と MiB/s
- 定期的に開かれているファイル（CreateFile が5回以上）の間隔（平均・最小・最大 ms）
- 失敗した操作（NTSTATUS を名前に変換）。`0xC0000011`（END OF FILE）は失敗に含めない

## 未確認・要検証

### 実機で確認済み（2026-11-21、Procmon 4.13）

- 通常権限の MCP サーバーから COM でタスクを起動すると、UAC なしで昇格して動く。
- `/LoadConfig` で読み込ませた生成 PMC は効く（`explorer.exe` の Include で `Explorer.EXE` だけが記録された。大文字小文字は区別しない）。
- `DestructiveFilter=1` で取得したログには Profiling イベントが入らない。
- **Procmon は終了時に、読み込んだ設定を `HKCU\Software\Sysinternals\Process Monitor` へ書き戻す**（FilterRules、DestructiveFilter、Logfile、ウィンドウ位置など）。→ runner が取得の前に保存し、後で元に戻す。
- **Procmon が既に起動していると、2つ目のインスタンスは取得できない**（約45秒後に空のログで終了した）。→ MCP サーバーと runner の両方で検出し、「Procmon を閉じてから再実行して」と返す。ユーザーの Procmon は止めない（`/Terminate` は使わない）。
- 5秒の取得は全体で約12秒かかる（タスクの起動、ドライバーの初期化、ログの書き出し）。

### 未確認

- （解決済み）実際の PMC の既定ルールは24本で、procmon-parser の README の例より1本多い。24本目は「Event Class is IPC → Exclude」。
- （解決済み）PMC の `DbgHelpPath` は runner が `GetSystemDirectory` から組み立てる。
- PMC のフォント設定が日本語環境の値（Yu Gothic UI）。HKCU を復元するので GUI への影響はないが、他の言語環境での表示は未確認。
- ネットワークイベントの Path（ホストとポートのテーブルが必要）は未実装。
- `/OpenLog ... /SaveAs x.csv` が昇格なしで動くか（PML の検証に使う場合）。
- 他のバージョンの Procmon で作った PML が読めるか。
- （対応済み）`install` の表示：実行内容（作成・コピー・ダウンロード元と保存先・タスク）を同意の前に示し、各手順に Done! を表示する。
