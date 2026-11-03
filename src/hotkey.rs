use anyhow::{Result, bail};

#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Hotkey {
    ctrl: bool,
    alt: bool,
    shift: bool,
    win: bool,
    vk: u32,
}

impl Hotkey {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        let parts: Vec<&str> = value
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();

        if parts.len() < 2 {
            bail!("hotkey must contain at least one modifier and one key, e.g. ctrl+alt+c");
        }

        let (key, modifiers) = parts.split_last().unwrap();
        let mut hotkey = Self {
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
            vk: 0,
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

        if !(hotkey.ctrl || hotkey.alt || hotkey.shift || hotkey.win) {
            bail!("hotkey needs at least one modifier (ctrl, alt, shift or win)");
        }

        hotkey.vk = parse_key(key)?;
        Ok(hotkey)
    }

    #[cfg(windows)]
    pub(crate) fn vk(self) -> u32 {
        self.vk
    }

    #[cfg(windows)]
    pub(crate) fn modifiers(self) -> HOT_KEY_MODIFIERS {
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
}

fn parse_key(key: &str) -> Result<u32> {
    let upper = key.to_ascii_uppercase();

    if upper.len() == 1 {
        let byte = upper.as_bytes()[0];
        if byte.is_ascii_alphanumeric() {
            return Ok(byte as u32);
        }
    }

    if let Some(number) = upper.strip_prefix('F') {
        if let Ok(number) = number.parse::<u32>() {
            if (1..=24).contains(&number) {
                return Ok(0x70 + number - 1);
            }
        }
    }

    match upper.as_str() {
        "SPACE" => Ok(0x20),
        "TAB" => Ok(0x09),
        "ENTER" | "RETURN" => Ok(0x0D),
        "ESC" | "ESCAPE" => Ok(0x1B),
        "BACKSPACE" => Ok(0x08),
        other => bail!("unsupported hotkey key `{other}`"),
    }
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
    }

    #[test]
    fn parses_function_keys_and_modifiers() {
        let hotkey = Hotkey::parse("ctrl+shift+f8").unwrap();
        assert!(hotkey.ctrl);
        assert!(hotkey.shift);
        assert_eq!(hotkey.vk, 0x70 + 7);
    }

    #[test]
    fn rejects_incomplete_hotkeys() {
        assert!(Hotkey::parse("c").is_err());
        assert!(Hotkey::parse("ctrl").is_err());
        assert!(Hotkey::parse("ctrl+alt+unknown").is_err());
    }
}
