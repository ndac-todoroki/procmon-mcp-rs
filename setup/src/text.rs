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

#[cfg(test)]
mod tests {
    use super::*;

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
