//! Colored console output for the interactive commands (install, update, uninstall).
//!
//! Colors are ANSI escape sequences. They are used only on a terminal and never when
//! `NO_COLOR` is set, so redirected output and installer logs stay plain text.

/// Colors used in messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// The product name and section headings.
    Accent,
    Success,
    Warning,
    /// JSON object keys.
    Key,
    /// JSON string values.
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub enabled: bool,
}

impl Style {
    pub fn plain() -> Style {
        Style { enabled: false }
    }

    /// Wraps `text` in the escape sequence for `color` when colors are enabled.
    pub fn paint(&self, text: &str, color: Color) -> String {
        if !self.enabled {
            return text.to_owned();
        }
        let code = match color {
            Color::Accent => "1;36", // bold cyan
            Color::Success => "32",  // green
            Color::Warning => "33",  // yellow
            Color::Key => "36",      // cyan
            Color::Value => "32",    // green
        };
        format!("\x1b[{code}m{text}\x1b[0m")
    }

    /// Colors object keys and string values of pretty-printed JSON.
    pub fn highlight_json(&self, json: &str) -> String {
        let chars: Vec<char> = json.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] != '"' {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            // A string token runs to the next quote that is not escaped.
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                i += if chars[i] == '\\' { 2 } else { 1 };
            }
            i = (i + 1).min(chars.len());
            let token: String = chars[start..i].iter().collect();
            let is_key = chars[i..]
                .iter()
                .find(|c| !c.is_whitespace())
                .is_some_and(|&c| c == ':');
            let color = if is_key { Color::Key } else { Color::Value };
            out.push_str(&self.paint(&token, color));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: Style = Style { enabled: true };

    #[test]
    fn plain_style_leaves_text_untouched() {
        assert_eq!(
            Style::plain().paint("procmon-mcp", Color::Accent),
            "procmon-mcp"
        );
        let json = "{\n  \"a\": \"b\"\n}";
        assert_eq!(Style::plain().highlight_json(json), json);
    }

    #[test]
    fn enabled_style_wraps_text_and_resets() {
        let painted = ON.paint("procmon-mcp", Color::Accent);
        assert!(painted.starts_with("\x1b["), "{painted:?}");
        assert!(painted.contains("procmon-mcp"));
        assert!(painted.ends_with("\x1b[0m"), "{painted:?}");
        assert_ne!(painted, ON.paint("procmon-mcp", Color::Success));
    }

    #[test]
    fn json_keys_and_string_values_get_different_colors() {
        let json = "{\n  \"command\": \"C:\\\\bin\\\\procmon-mcp.exe\",\n  \"args\": [\n    \"mcp\"\n  ]\n}";
        let out = ON.highlight_json(json);
        assert!(out.contains(&ON.paint("\"command\"", Color::Key)), "{out}");
        assert!(
            out.contains(&ON.paint("\"C:\\\\bin\\\\procmon-mcp.exe\"", Color::Value)),
            "{out}"
        );
        assert!(out.contains(&ON.paint("\"mcp\"", Color::Value)), "{out}");
        // Removing the escape sequences gives the original JSON back.
        let stripped: String = strip_escapes(&out);
        assert_eq!(stripped, json);
    }

    fn strip_escapes(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }
}
