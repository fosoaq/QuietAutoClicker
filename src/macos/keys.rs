//! Key names <-> macOS virtual keycodes.
//!
//! The codes are physical ANSI positions, so capture, hotkeys and injected
//! keys behave the same under every keyboard layout.

const KEYS: &[(&str, u16)] = &[
    ("A", 0x00),
    ("S", 0x01),
    ("D", 0x02),
    ("F", 0x03),
    ("H", 0x04),
    ("G", 0x05),
    ("Z", 0x06),
    ("X", 0x07),
    ("C", 0x08),
    ("V", 0x09),
    ("B", 0x0B),
    ("Q", 0x0C),
    ("W", 0x0D),
    ("E", 0x0E),
    ("R", 0x0F),
    ("Y", 0x10),
    ("T", 0x11),
    ("1", 0x12),
    ("2", 0x13),
    ("3", 0x14),
    ("4", 0x15),
    ("6", 0x16),
    ("5", 0x17),
    ("=", 0x18),
    ("9", 0x19),
    ("7", 0x1A),
    ("-", 0x1B),
    ("8", 0x1C),
    ("0", 0x1D),
    ("]", 0x1E),
    ("O", 0x1F),
    ("U", 0x20),
    ("[", 0x21),
    ("I", 0x22),
    ("P", 0x23),
    ("Enter", 0x24),
    ("L", 0x25),
    ("J", 0x26),
    ("'", 0x27),
    ("K", 0x28),
    (";", 0x29),
    ("\\", 0x2A),
    (",", 0x2B),
    ("/", 0x2C),
    ("N", 0x2D),
    ("M", 0x2E),
    (".", 0x2F),
    ("Tab", 0x30),
    ("Space", 0x31),
    ("`", 0x32),
    ("Backspace", 0x33),
    ("Esc", 0x35),
    ("F5", 0x60),
    ("F6", 0x61),
    ("F7", 0x62),
    ("F3", 0x63),
    ("F8", 0x64),
    ("F9", 0x65),
    ("F11", 0x67),
    ("F10", 0x6D),
    ("F12", 0x6F),
    ("Insert", 0x72),
    ("Home", 0x73),
    ("PageUp", 0x74),
    ("Delete", 0x75),
    ("F4", 0x76),
    ("End", 0x77),
    ("F2", 0x78),
    ("PageDown", 0x79),
    ("F1", 0x7A),
    ("Left", 0x7B),
    ("Right", 0x7C),
    ("Down", 0x7D),
    ("Up", 0x7E),
];

pub fn code_from_name(name: &str) -> Option<u16> {
    KEYS.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

pub fn name_from_code(code: u16) -> Option<&'static str> {
    KEYS.iter().find(|(_, c)| *c == code).map(|(n, _)| *n)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

pub fn format_combo(code: u16, m: Mods) -> Option<String> {
    let name = name_from_code(code)?;
    let mut parts = Vec::new();
    for (on, label) in [
        (m.ctrl, "Ctrl"),
        (m.shift, "Shift"),
        (m.alt, "Alt"),
        (m.meta, "Meta"),
    ] {
        if on {
            parts.push(label);
        }
    }
    parts.push(name);
    Some(parts.join("+"))
}

pub fn parse_combo(combo: &str) -> Option<(u16, Mods)> {
    let mut parts: Vec<&str> = combo.split('+').collect();
    let code = code_from_name(parts.pop()?)?;
    let mut m = Mods::default();
    for p in parts {
        match p {
            "Ctrl" => m.ctrl = true,
            "Shift" => m.shift = true,
            "Alt" => m.alt = true,
            "Meta" => m.meta = true,
            _ => return None,
        }
    }
    Some((code, m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_bijective() {
        for (n, c) in KEYS {
            assert_eq!(code_from_name(n), Some(*c));
            assert_eq!(name_from_code(*c), Some(*n));
        }
    }

    #[test]
    fn combo_round_trips() {
        for s in ["F6", "Ctrl+Shift+J", "Alt+Meta+Space"] {
            let (code, m) = parse_combo(s).unwrap();
            assert_eq!(format_combo(code, m).unwrap(), s);
        }
        assert_eq!(parse_combo("Ctrl"), None);
        assert_eq!(parse_combo("Nope+A"), None);
    }
}
