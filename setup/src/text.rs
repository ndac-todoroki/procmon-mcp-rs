//! User-visible strings in each supported language.

/// A language the setup is translated into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Ja,
}

impl Lang {
    /// The language for a Windows language ID (LANGID), English unless translated.
    pub fn from_lang_id(id: u16) -> Lang {
        // The low 10 bits are the primary language, the rest the region.
        const LANG_JAPANESE: u16 = 0x11;
        if id & 0x3ff == LANG_JAPANESE {
            Lang::Ja
        } else {
            Lang::En
        }
    }

    /// `PROCMON_MCP_LANG=en|ja`, for checking a translation on any system.
    pub fn from_override(value: &str) -> Option<Lang> {
        match value {
            "en" => Some(Lang::En),
            "ja" => Some(Lang::Ja),
            _ => None,
        }
    }
}

/// Defines `Text`, its English and Japanese tables, and `Text::all` for checks.
macro_rules! text_table {
    ($($field:ident: $en:expr, $ja:expr;)*) => {
        /// Every user-visible string. Paths and identifiers are not translated and not
        /// in here: they are shown in a monospace font, because a proportional Japanese
        /// font draws `\` as `¥`.
        pub struct Text {
            $(pub $field: &'static str,)*
        }

        pub const EN: Text = Text { $($field: $en,)* };
        pub const JA: Text = Text { $($field: $ja,)* };

        impl Text {
            #[cfg(test)]
            fn all(&self) -> Vec<(&'static str, &'static str)> {
                vec![$((stringify!($field), self.$field),)*]
            }
        }
    };
}

text_table! {
    window_title: "procmon-mcp Setup", "procmon-mcp セットアップ";
    step_of: "Step {n} of {total} \u{00B7} {name}", "ステップ {n}/{total} \u{00B7} {name}";
    step_welcome: "Welcome", "ようこそ";
    step_license: "License", "ライセンス";
    step_review: "Review", "確認";
    step_install: "Install", "インストール";
    step_done: "Done", "完了";
    next: "Next", "次へ";
    install: "Install", "インストール";
    finish: "Finish", "完了";
    close: "Close", "閉じる";
    try_again: "Try again", "もう一度試す";

    welcome_heading: "Welcome to procmon-mcp", "procmon-mcp へようこそ";
    welcome_body:
        "procmon-mcp lets Claude run Sysinternals Process Monitor for a few seconds and tell \
         you what a process is doing.",
        "procmon-mcp を使うと、Claude が Sysinternals Process Monitor を数秒間動かして、\
         プロセスが何をしているかを教えてくれます。";
    already_installed:
        "procmon-mcp is already installed. Continuing repairs it and gets the latest Process \
         Monitor.",
        "procmon-mcp は既にインストールされています。続けると、修復して最新の Process Monitor \
         を取得します。";
    feature_capture: "Capture on request", "依頼に応じて記録";
    feature_capture_detail:
        "Only the processes, operations and paths Claude asks for",
        "Claude が指定したプロセス、操作、パスだけを記録します";
    feature_summary: "Summaries, not raw logs", "生のログではなく集計";
    feature_summary_detail:
        "Hot files, read and write volume, periodic reopens, failures",
        "よくアクセスするファイル、読み書きの量、定期的な読み直し、失敗した操作";
    feature_settings: "Your Procmon settings stay yours", "Process Monitor の設定はそのまま";
    feature_settings_detail:
        "Restored after every capture",
        "記録のたびに元の設定に戻します";

    heading_procmon: "Process Monitor", "Process Monitor";
    license_body:
        "Setup downloads Process Monitor directly from Microsoft Sysinternals:",
        "Process Monitor を Microsoft Sysinternals から直接ダウンロードします。";
    license_signature:
        "The file is kept only if it carries a valid Microsoft signature, and the signature is \
         checked again before every capture.",
        "Microsoft の有効な署名がある場合だけ保存します。署名は記録のたびにも確認します。";
    license_title: "License", "ライセンス";
    license_message:
        "procmon-mcp starts Process Monitor with /AcceptEula, which accepts the Sysinternals \
         Software License Terms on your behalf.",
        "procmon-mcp は Process Monitor を /AcceptEula を付けて起動します。これは、あなたの代わりに \
         Sysinternals ソフトウェア ライセンス条項に同意することを意味します。";
    license_link: "Read the Sysinternals Software License Terms", "Sysinternals ソフトウェア ライセンス条項を読む";
    license_accept: "I accept the Sysinternals Software License Terms", "Sysinternals ソフトウェア ライセンス条項に同意します";

    review_heading: "Ready to install", "インストールの準備ができました";
    review_body: "Setup will make these changes:", "次の変更を行います。";
    review_folder: "Create the procmon-mcp folder", "procmon-mcp のフォルダーを作成";
    review_download: "Download Process Monitor", "Process Monitor をダウンロード";
    review_task:
        "Register a scheduled task that runs captures as administrator",
        "管理者権限で記録を実行するタスクを登録";
    review_uninstaller:
        "Add an uninstaller to Installed apps",
        "「インストールされているアプリ」にアンインストーラーを追加";
    admin_title: "Administrator permission", "管理者権限";
    admin_message:
        "Windows will ask for permission when you select Install. procmon-mcp can be installed \
         only by a signed-in administrator.",
        "[インストール] を選ぶと、Windows が許可を求めます。インストールできるのは、管理者として\
         サインインしている場合だけです。";

    installing_heading: "Installing\u{2026}", "インストールしています\u{2026}";
    progress_folder: "Create the procmon-mcp folder", "procmon-mcp のフォルダーを作成";
    progress_copy: "Copy the programs", "プログラムをコピー";
    progress_download:
        "Download Process Monitor and verify its signature",
        "Process Monitor をダウンロードして署名を確認";
    progress_register:
        "Register the scheduled task and the uninstaller",
        "タスクとアンインストーラーを登録";

    done_heading: "procmon-mcp is installed", "procmon-mcp をインストールしました";
    done_body:
        "One last step: add this to \"mcpServers\" in Claude Desktop's claude_desktop_config.json \
         (Settings \u{2192} Developer \u{2192} Edit Config), then restart Claude Desktop.",
        "最後に、Claude Desktop の claude_desktop_config.json（設定 \u{2192} 開発者 \u{2192} \
         構成を編集）の \"mcpServers\" にこれを追加して、Claude Desktop を再起動してください。";
    copy: "Copy settings", "設定をコピー";
    copied: "Copied", "コピーしました";
    uninstall_note:
        "To remove procmon-mcp later, use Settings \u{2192} Apps \u{2192} Installed apps.",
        "アンインストールは、設定 \u{2192} アプリ \u{2192} インストールされているアプリ から行えます。";

    failed_heading: "Setup could not finish", "セットアップを完了できませんでした";
    elevation_declined:
        "Windows did not grant administrator permission.",
        "管理者権限が許可されませんでした。";

    uninstall_heading: "Uninstall procmon-mcp", "procmon-mcp のアンインストール";
    uninstall_body:
        "This removes the scheduled task and the procmon-mcp folder, including the Process \
         Monitor that Setup downloaded. Quit Claude Desktop first.",
        "記録用のタスクと、procmon-mcp のフォルダー（セットアップがダウンロードした Process Monitor \
         を含む）を削除します。先に Claude Desktop を終了してください。";
    uninstall: "Uninstall", "アンインストール";
    uninstalling_heading: "Uninstalling\u{2026}", "アンインストールしています\u{2026}";
    uninstalled_heading: "procmon-mcp has been removed", "procmon-mcp を削除しました";
    uninstalled_body:
        "Remove the \"procmon\" entry from Claude Desktop's claude_desktop_config.json as well.",
        "Claude Desktop の claude_desktop_config.json から \"procmon\" の項目も削除してください。";
    uninstall_failed_heading: "Uninstall could not finish", "アンインストールを完了できませんでした";
    in_use_note:
        "Some files were in use and will be deleted when Windows restarts.",
        "使用中のファイルがあったため、一部は Windows の再起動時に削除されます。";
    runtime_missing:
        "procmon-mcp Setup needs the Windows App Runtime 2.x. Install it from Microsoft \
         (learn.microsoft.com/windows/apps/windows-app-sdk/downloads) and run Setup again.",
        "procmon-mcp のセットアップには Windows App Runtime 2.x が必要です。Microsoft から\
         インストールして（learn.microsoft.com/windows/apps/windows-app-sdk/downloads）、\
         もう一度実行してください。";
}

impl Lang {
    pub fn text(self) -> &'static Text {
        match self {
            Lang::En => &EN,
            Lang::Ja => &JA,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_string_is_translated() {
        for lang in [Lang::En, Lang::Ja] {
            for (name, value) in lang.text().all() {
                assert!(!value.trim().is_empty(), "{lang:?}.{name} is empty");
            }
        }
        // Japanese strings must not be English leftovers (identifiers aside).
        for ((name, en), (_, ja)) in EN.all().into_iter().zip(JA.all()) {
            if en.chars().any(|c| c.is_ascii_lowercase()) && name != "heading_procmon" {
                assert_ne!(en, ja, "{name} is not translated");
            }
        }
    }

    #[test]
    fn no_string_holds_a_path() {
        for lang in [Lang::En, Lang::Ja] {
            for (name, value) in lang.text().all() {
                assert!(!value.contains('\\'), "{lang:?}.{name}: {value}");
            }
        }
    }

    #[test]
    fn step_label_keeps_its_placeholders() {
        for lang in [Lang::En, Lang::Ja] {
            let step = lang.text().step_of;
            assert!(step.contains("{n}") && step.contains("{total}") && step.contains("{name}"));
        }
    }

    #[test]
    fn japanese_language_ids_select_japanese() {
        assert_eq!(Lang::from_lang_id(0x0411), Lang::Ja);
    }

    #[test]
    fn other_language_ids_fall_back_to_english() {
        for id in [0x0409, 0x0809, 0x0804, 0x0407, 0x0000] {
            assert_eq!(Lang::from_lang_id(id), Lang::En, "{id:#06x}");
        }
    }

    #[test]
    fn override_accepts_only_known_codes() {
        assert_eq!(Lang::from_override("ja"), Some(Lang::Ja));
        assert_eq!(Lang::from_override("en"), Some(Lang::En));
        assert_eq!(Lang::from_override("fr"), None);
        assert_eq!(Lang::from_override(""), None);
    }
}
