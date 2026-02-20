use std::env;

/// Icon set for different terminal capabilities
#[derive(Debug, Clone)]
pub struct IconSet {
    pub folder: &'static str,
    pub file: &'static str,
    pub loading: &'static str,
    pub error: &'static str,
    pub success: &'static str,
    pub empty: &'static str,
    pub search: &'static str,
    pub refresh: &'static str,
}

impl IconSet {
    /// Fancy Unicode/Emoji icons for modern terminals
    pub const UNICODE: IconSet = IconSet {
        folder: "📁",
        file: "📄",
        loading: "🔄",
        error: "❌",
        success: "✅",
        empty: "📭",
        search: "🔍",
        refresh: "🔄",
    };

    /// ASCII fallback icons for basic terminals
    pub const ASCII: IconSet = IconSet {
        folder: "[DIR]",
        file: "[FILE]",
        loading: "[LOADING]",
        error: "[ERROR]",
        success: "[OK]",
        empty: "[EMPTY]",
        search: "[SEARCH]",
        refresh: "[REFRESH]",
    };

    /// Minimal symbols for very basic terminals
    pub const MINIMAL: IconSet = IconSet {
        folder: "D",
        file: "F",
        loading: "*",
        error: "!",
        success: "+",
        empty: "-",
        search: "?",
        refresh: "~",
    };
}

/// Detect terminal capabilities and return appropriate icon set
pub fn detect_terminal_icons() -> IconSet {
    // Check for explicit override first
    if let Ok(val) = env::var("BLOBRS_ICONS") {
        match val.to_lowercase().as_str() {
            "unicode" | "emoji" | "fancy" => return IconSet::UNICODE,
            "ascii" => return IconSet::ASCII,
            "minimal" | "basic" => return IconSet::MINIMAL,
            _ => {}
        }
    }

    // Check terminal type and capabilities
    if is_unicode_capable_terminal() {
        IconSet::UNICODE
    } else if is_ascii_capable_terminal() {
        IconSet::ASCII
    } else {
        IconSet::MINIMAL
    }
}

/// Check if terminal supports Unicode/emoji
fn is_unicode_capable_terminal() -> bool {
    // Check for modern terminals that support Unicode well
    let term_support = if let Ok(term) = env::var("TERM") {
        match term.as_str() {
            // Modern terminals with good Unicode support
            "xterm-256color" | "screen-256color" | "tmux-256color" => true,
            // Explicitly check for terminals known to support Unicode
            t if t.contains("kitty") || t.contains("alacritty") || t.contains("wezterm") => true,
            // iTerm2, Terminal.app on macOS
            t if t.contains("iterm") || t.contains("apple") => true,
            // VS Code integrated terminal
            t if t.contains("vscode") => true,
            _ => false,
        }
    } else {
        false
    };

    term_support || is_utf8_locale() || is_modern_terminal_program()
}

/// Check if terminal supports basic ASCII box drawing
fn is_ascii_capable_terminal() -> bool {
    // Most terminals support ASCII, so this is our fallback
    // Only return false for very minimal environments
    !matches!(
        env::var("TERM").as_deref(),
        Ok("dumb") | Ok("unknown") | Err(_)
    )
}

/// Check if locale supports UTF-8
fn is_utf8_locale() -> bool {
    for var in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Ok(locale) = env::var(var)
            && (locale.to_uppercase().contains("UTF-8") || locale.to_uppercase().contains("UTF8"))
        {
            return true;
        }
    }
    false
}

/// Check for modern terminal programs
fn is_modern_terminal_program() -> bool {
    // Check various environment variables that indicate modern terminals
    for var in [
        "KITTY_WINDOW_ID",
        "ALACRITTY_SOCKET",
        "WEZTERM_EXECUTABLE",
        "ITERM_SESSION_ID",
        "VSCODE_INJECTION",
        "TERM_PROGRAM",
    ] {
        if env::var(var).is_ok() {
            return true;
        }
    }

    // Check TERM_PROGRAM specifically
    if let Ok(term_program) = env::var("TERM_PROGRAM") {
        match term_program.as_str() {
            "iTerm.app" | "Apple_Terminal" | "vscode" | "Hyper" | "Tabby" => return true,
            _ => {}
        }
    }

    // Check for Windows Terminal
    if env::var("WT_SESSION").is_ok() {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_icon_sets() {
        let unicode = IconSet::UNICODE;
        let ascii = IconSet::ASCII;
        let minimal = IconSet::MINIMAL;

        assert_eq!(unicode.folder, "📁");
        assert_eq!(ascii.folder, "[DIR]");
        assert_eq!(minimal.folder, "D");
    }

    #[test]
    fn test_detection_with_override() {
        unsafe {
            env::set_var("BLOBRS_ICONS", "ascii");
        }
        let icons = detect_terminal_icons();
        assert_eq!(icons.folder, "[DIR]");
        unsafe {
            env::remove_var("BLOBRS_ICONS");
        }
    }
}
