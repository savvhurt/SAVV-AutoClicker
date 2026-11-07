use std::fmt;

use anyhow::{Result, bail};

#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN,
};

pub(crate) const DEFAULT: &str = "ctrl+alt+c";

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct Hotkey {
    ctrl: bool,
    alt: bool,
    shift: bool,
    win: bool,
    vk: u32,
    key: String,
}

impl Hotkey {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        let parts: Vec<&str> = value
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();

        if parts.is_empty() {
            bail!("hotkey is empty; use a key such as f8 or ctrl+alt+c");
        }

        let (key, modifiers) = parts.split_last().unwrap();
        let mut hotkey = Self {
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
            vk: 0,
            key: String::new(),
        };

        for modifier in modifiers {
            match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => hotkey.ctrl = true,
                "alt" => hotkey.alt = true,
                "shift" => hotkey.shift = true,
                "win" | "super" | "meta" | "cmd" => hotkey.win = true,
                other => bail!("unknown hotkey modifier `{other}`"),
            }
        }

        (hotkey.vk, hotkey.key) = parse_key(key)?;
        Ok(hotkey)
    }

    #[cfg(windows)]
    pub(crate) fn vk(&self) -> u32 {
        self.vk
    }

    #[cfg(windows)]
    pub(crate) fn modifiers(&self) -> HOT_KEY_MODIFIERS {
        let mut modifiers = HOT_KEY_MODIFIERS(0);
        if self.ctrl {
            modifiers |= MOD_CONTROL;
        }
        if self.alt {
            modifiers |= MOD_ALT;
        }
        if self.shift {
            modifiers |= MOD_SHIFT;
        }
        if self.win {
            modifiers |= MOD_WIN;
        }
        modifiers
    }

    /// Whether the key and every modifier are currently held down. Used to
    /// detect the release of a hold-to-click hotkey, because `RegisterHotKey`
    /// only reports the initial press.
    #[cfg(windows)]
    pub(crate) fn is_down(&self) -> bool {
        fn down(vk: u32) -> bool {
            (unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000) != 0
        }

        let mut keys = vec![self.vk];
        if self.ctrl {
            keys.push(0x11);
        }
        if self.alt {
            keys.push(0x12);
        }
        if self.shift {
            keys.push(0x10);
        }
        if self.win {
            keys.push(0x5B);
        }

        keys.into_iter().all(down)
    }

    /// Human-readable form used by the GUI, e.g. `CTRL + ALT + C`.
    pub(crate) fn label(&self) -> String {
        let mut parts = Vec::with_capacity(5);
        if self.ctrl {
            parts.push("CTRL".to_string());
        }
        if self.alt {
            parts.push("ALT".to_string());
        }
        if self.shift {
            parts.push("SHIFT".to_string());
        }
        if self.win {
            parts.push("WIN".to_string());
        }
        parts.push(key_label(&self.key).to_ascii_uppercase());
        parts.join(" + ")
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for (enabled, name) in [
            (self.ctrl, "ctrl"),
            (self.alt, "alt"),
            (self.shift, "shift"),
            (self.win, "win"),
        ] {
            if enabled {
                if !first {
                    f.write_str("+")?;
                }
                f.write_str(name)?;
                first = false;
            }
        }

        if !first {
            f.write_str("+")?;
        }
        f.write_str(&self.key.to_ascii_lowercase())
    }
}

fn key_label(key: &str) -> &str {
    match key {
        "Space" => "Space",
        "Escape" => "Esc",
        "Minus" => "-",
        "Equals" => "=",
        "OpenBracket" => "[",
        "CloseBracket" => "]",
        "Backslash" => "\\",
        "Semicolon" => ";",
        "Quote" => "'",
        "Comma" => ",",
        "Period" => ".",
        "Slash" => "/",
        "Backtick" => "`",
        other => other,
    }
}

fn parse_key(key: &str) -> Result<(u32, String)> {
    let upper = key.trim().to_ascii_uppercase();

    if upper.is_empty() {
        bail!("hotkey is empty; use a key such as f8 or ctrl+alt+c");
    }

    if upper.len() == 1 {
        let byte = upper.as_bytes()[0];
        if byte.is_ascii_alphanumeric() {
            return Ok((byte as u32, upper));
        }
    }

    if let Some(number) = upper.strip_prefix('F') {
        if let Ok(number) = number.parse::<u32>() {
            if (1..=24).contains(&number) {
                return Ok((0x70 + number - 1, upper));
            }
        }
    }

    let (vk, canonical) = match upper.as_str() {
        "SPACE" => (0x20, "Space"),
        "TAB" => (0x09, "Tab"),
        "ENTER" | "RETURN" => (0x0D, "Enter"),
        "ESC" | "ESCAPE" => (0x1B, "Escape"),
        "BACKSPACE" | "BKSP" => (0x08, "Backspace"),
        "DELETE" | "DEL" => (0x2E, "Delete"),
        "INSERT" | "INS" => (0x2D, "Insert"),
        "HOME" => (0x24, "Home"),
        "END" => (0x23, "End"),
        "PAGEUP" | "PGUP" | "PRIOR" => (0x21, "PageUp"),
        "PAGEDOWN" | "PGDN" | "NEXT" => (0x22, "PageDown"),
        "LEFT" | "ARROWLEFT" => (0x25, "Left"),
        "UP" | "ARROWUP" => (0x26, "Up"),
        "RIGHT" | "ARROWRIGHT" => (0x27, "Right"),
        "DOWN" | "ARROWDOWN" => (0x28, "Down"),
        "MINUS" | "-" => (0xBD, "Minus"),
        "EQUALS" | "EQUAL" | "=" | "PLUS" | "+" => (0xBB, "Equals"),
        "OPENBRACKET" | "LBRACKET" | "LEFTBRACKET" | "OPENCURLYBRACKET" | "[" | "{" => {
            (0xDB, "OpenBracket")
        }
        "CLOSEBRACKET" | "RBRACKET" | "RIGHTBRACKET" | "CLOSECURLYBRACKET" | "]" | "}" => {
            (0xDD, "CloseBracket")
        }
        "BACKSLASH" | "PIPE" | "\\" | "|" => (0xDC, "Backslash"),
        "SEMICOLON" | "COLON" | ";" | ":" => (0xBA, "Semicolon"),
        "QUOTE" | "APOSTROPHE" | "'" => (0xDE, "Quote"),
        "COMMA" | "," => (0xBC, "Comma"),
        "PERIOD" | "DOT" | "." => (0xBE, "Period"),
        "SLASH" | "QUESTIONMARK" | "/" | "?" => (0xBF, "Slash"),
        "BACKTICK" | "BACKQUOTE" | "GRAVE" | "`" => (0xC0, "Backtick"),
        "EXCLAMATIONMARK" | "!" => (0x31, "1"),
        other => bail!("unsupported hotkey key `{other}`"),
    };

    Ok((vk, canonical.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_hotkey() {
        let hotkey = Hotkey::parse("ctrl+alt+c").unwrap();
        assert!(hotkey.ctrl);
        assert!(hotkey.alt);
        assert!(!hotkey.shift);
        assert!(!hotkey.win);
        assert_eq!(hotkey.vk, 'C' as u32);
        assert_eq!(hotkey.to_string(), "ctrl+alt+c");
        assert_eq!(hotkey.label(), "CTRL + ALT + C");
    }

    #[test]
    fn parses_single_keys_without_modifiers() {
        for (combo, vk) in [
            ("f8", 0x77),
            ("F24", 0x87),
            ("a", 'A' as u32),
            ("7", '7' as u32),
            ("space", 0x20),
            ("esc", 0x1B),
            ("left", 0x25),
            ("pagedown", 0x22),
            ("minus", 0xBD),
            ("[", 0xDB),
            ("'", 0xDE),
            ("`", 0xC0),
        ] {
            let hotkey = Hotkey::parse(combo).unwrap();
            assert_eq!(hotkey.vk, vk, "wrong virtual key for {combo}");
        }

        assert_eq!(Hotkey::parse("F8").unwrap().to_string(), "f8");
        assert_eq!(Hotkey::parse("Esc").unwrap().label(), "ESC");
        assert_eq!(Hotkey::parse("[").unwrap().label(), "[");
    }

    #[test]
    fn parses_function_keys_and_modifiers() {
        let hotkey = Hotkey::parse("ctrl+shift+f8").unwrap();
        assert!(hotkey.ctrl);
        assert!(hotkey.shift);
        assert_eq!(hotkey.vk, 0x70 + 7);
        assert_eq!(hotkey.to_string(), "ctrl+shift+f8");
    }

    #[test]
    fn canonicalizes_order_and_aliases() {
        let hotkey = Hotkey::parse("alt+control+space").unwrap();
        assert_eq!(hotkey.to_string(), "ctrl+alt+space");
        assert_eq!(hotkey.label(), "CTRL + ALT + SPACE");
    }

    #[test]
    fn rejects_empty_and_modifier_only_hotkeys() {
        assert!(Hotkey::parse("").is_err());
        assert!(Hotkey::parse("   ").is_err());
        assert!(Hotkey::parse("ctrl").is_err());
        assert!(Hotkey::parse("ctrl+alt+shift").is_err());
        assert!(Hotkey::parse("ctrl+alt+unknown").is_err());
    }
}
