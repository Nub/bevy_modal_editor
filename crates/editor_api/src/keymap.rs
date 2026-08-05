//! Key sequences and conflict detection (RFC §4, keymap design doc).
//!
//! Bindings are data: parsed from strings like `"ctrl+z"`, `"g g"`, `"space p p"`.
//! A binding is a sequence of chords; a chord is modifiers + one key. Conflict rules
//! (M1 acceptance A2): within one context, duplicate sequences and prefix-shadowing
//! are hard errors; across contexts anything goes (contexts are layered).

use bevy::input::keyboard::KeyCode;
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub cmd: bool, // super / windows key
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Chord {
    pub modifiers: Modifiers,
    pub key: KeyCode,
}

/// A parsed key sequence, e.g. `g g` or `ctrl+shift+p`.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Binding(pub Vec<Chord>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub input: String,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid binding {:?}: {}", self.input, self.message)
    }
}
impl std::error::Error for ParseError {}

fn parse_key(token: &str) -> Option<KeyCode> {
    use KeyCode::*;
    Some(match token {
        // letters
        "a" => KeyA,
        "b" => KeyB,
        "c" => KeyC,
        "d" => KeyD,
        "e" => KeyE,
        "f" => KeyF,
        "g" => KeyG,
        "h" => KeyH,
        "i" => KeyI,
        "j" => KeyJ,
        "k" => KeyK,
        "l" => KeyL,
        "m" => KeyM,
        "n" => KeyN,
        "o" => KeyO,
        "p" => KeyP,
        "q" => KeyQ,
        "r" => KeyR,
        "s" => KeyS,
        "t" => KeyT,
        "u" => KeyU,
        "v" => KeyV,
        "w" => KeyW,
        "x" => KeyX,
        "y" => KeyY,
        "z" => KeyZ,
        // digits
        "0" => Digit0,
        "1" => Digit1,
        "2" => Digit2,
        "3" => Digit3,
        "4" => Digit4,
        "5" => Digit5,
        "6" => Digit6,
        "7" => Digit7,
        "8" => Digit8,
        "9" => Digit9,
        // named
        "space" => Space,
        "esc" | "escape" => Escape,
        "enter" | "return" => Enter,
        "tab" => Tab,
        "backspace" => Backspace,
        "delete" | "del" => Delete,
        "up" => ArrowUp,
        "down" => ArrowDown,
        "left" => ArrowLeft,
        "right" => ArrowRight,
        "home" => Home,
        "end" => End,
        "pageup" => PageUp,
        "pagedown" => PageDown,
        // punctuation (vim-critical)
        "comma" | "," => Comma,
        "period" | "." => Period,
        "slash" | "/" => Slash,
        "backslash" => Backslash,
        "semicolon" | ";" => Semicolon,
        "quote" | "'" => Quote,
        "backtick" | "`" | "grave" => Backquote,
        "minus" | "-" => Minus,
        "equals" | "=" => Equal,
        "bracketleft" | "[" => BracketLeft,
        "bracketright" | "]" => BracketRight,
        // function keys
        "f1" => F1,
        "f2" => F2,
        "f3" => F3,
        "f4" => F4,
        "f5" => F5,
        "f6" => F6,
        "f7" => F7,
        "f8" => F8,
        "f9" => F9,
        "f10" => F10,
        "f11" => F11,
        "f12" => F12,
        _ => return None,
    })
}

fn key_name(key: KeyCode) -> String {
    let s = format!("{key:?}");
    // KeyA -> a, Digit4 -> 4, ArrowUp -> up, others lowercased
    if let Some(rest) = s.strip_prefix("Key") {
        rest.to_lowercase()
    } else if let Some(rest) = s.strip_prefix("Digit") {
        rest.to_string()
    } else if let Some(rest) = s.strip_prefix("Arrow") {
        rest.to_lowercase()
    } else {
        s.to_lowercase()
    }
}

impl FromStr for Chord {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = |message: &str| ParseError {
            input: s.to_string(),
            message: message.into(),
        };
        let mut modifiers = Modifiers::default();
        let mut key = None;
        for part in s.split('+') {
            let part_lower = part.to_lowercase();
            match part_lower.as_str() {
                "ctrl" | "control" => modifiers.ctrl = true,
                "shift" => modifiers.shift = true,
                "alt" | "option" => modifiers.alt = true,
                "cmd" | "super" | "meta" => modifiers.cmd = true,
                token => {
                    if key.is_some() {
                        return Err(err("multiple non-modifier keys in one chord"));
                    }
                    key = Some(
                        parse_key(token).ok_or_else(|| err(&format!("unknown key {token:?}")))?,
                    );
                }
            }
        }
        Ok(Chord {
            modifiers,
            key: key.ok_or_else(|| err("chord has no key"))?,
        })
    }
}

impl FromStr for Binding {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(ParseError {
                input: s.into(),
                message: "empty binding".into(),
            });
        }
        let chords = trimmed
            .split_whitespace()
            .map(Chord::from_str)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Binding(chords))
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.ctrl {
            write!(f, "ctrl+")?;
        }
        if self.modifiers.shift {
            write!(f, "shift+")?;
        }
        if self.modifiers.alt {
            write!(f, "alt+")?;
        }
        if self.modifiers.cmd {
            write!(f, "cmd+")?;
        }
        f.write_str(&key_name(self.key))
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, chord) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{chord}")?;
        }
        Ok(())
    }
}

/// A conflict between two bindings in the same context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conflict {
    /// Identical sequences bound to different actions.
    Duplicate {
        binding: String,
        first: String,
        second: String,
    },
    /// One binding is a strict prefix of another — the shorter would shadow the longer.
    PrefixShadow {
        prefix: String,
        prefix_owner: String,
        shadowed: String,
        shadowed_owner: String,
    },
}

impl fmt::Display for Conflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Conflict::Duplicate {
                binding,
                first,
                second,
            } => write!(
                f,
                "duplicate binding {binding:?}: bound by both {first:?} and {second:?}"
            ),
            Conflict::PrefixShadow {
                prefix,
                prefix_owner,
                shadowed,
                shadowed_owner,
            } => write!(
                f,
                "binding {prefix:?} ({prefix_owner:?}) shadows {shadowed:?} ({shadowed_owner:?})"
            ),
        }
    }
}

/// Check one context's bindings (`(binding, owner-label)`) for conflicts.
pub fn find_conflicts(entries: &[(Binding, String)]) -> Vec<Conflict> {
    let mut conflicts = Vec::new();
    for (i, (a, a_owner)) in entries.iter().enumerate() {
        for (b, b_owner) in entries.iter().skip(i + 1) {
            if a.0 == b.0 {
                conflicts.push(Conflict::Duplicate {
                    binding: a.to_string(),
                    first: a_owner.clone(),
                    second: b_owner.clone(),
                });
            } else if b.0.starts_with(&a.0) {
                conflicts.push(Conflict::PrefixShadow {
                    prefix: a.to_string(),
                    prefix_owner: a_owner.clone(),
                    shadowed: b.to_string(),
                    shadowed_owner: b_owner.clone(),
                });
            } else if a.0.starts_with(&b.0) {
                conflicts.push(Conflict::PrefixShadow {
                    prefix: b.to_string(),
                    prefix_owner: b_owner.clone(),
                    shadowed: a.to_string(),
                    shadowed_owner: a_owner.clone(),
                });
            }
        }
    }
    conflicts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(s: &str) -> Binding {
        s.parse().unwrap()
    }

    // A1: parse + round-trip
    #[test]
    fn parses_and_round_trips() {
        for s in [
            "ctrl+z",
            "g g",
            "space p p",
            "shift+4",
            "ctrl+shift+p",
            "f12",
            ".",
        ] {
            let binding = b(s);
            let shown = binding.to_string();
            let reparsed: Binding = shown.parse().unwrap();
            assert_eq!(
                binding, reparsed,
                "round-trip failed for {s:?} -> {shown:?}"
            );
        }
        assert_eq!(b("ctrl+z").0.len(), 1);
        assert_eq!(b("g g").0.len(), 2);
        assert_eq!(b("space p p").0.len(), 3);
        assert!(b("shift+4").0[0].modifiers.shift);
    }

    // A1: garbage rejected with useful errors
    #[test]
    fn rejects_garbage() {
        for s in ["", "ctrl+", "wobble", "a+b", "ctrl+wibble"] {
            let e = s.parse::<Binding>().unwrap_err();
            assert!(!e.message.is_empty(), "no message for {s:?}");
        }
    }

    // A2: conflicts
    #[test]
    fn detects_duplicates_and_prefix_shadowing() {
        let entries = vec![
            (b("g g"), "hierarchy.top".to_string()),
            (b("g g"), "other.action".to_string()),
            (b("g"), "greedy".to_string()),
            (b("ctrl+z"), "undo".to_string()),
        ];
        let conflicts = find_conflicts(&entries);
        assert!(
            conflicts
                .iter()
                .any(|c| matches!(c, Conflict::Duplicate { first, second, .. }
            if first == "hierarchy.top" && second == "other.action"))
        );
        // "g" shadows both "g g" entries
        assert_eq!(
            conflicts
                .iter()
                .filter(|c| matches!(c, Conflict::PrefixShadow { .. }))
                .count(),
            2
        );
        // ctrl+z conflicts with nothing
        assert!(!conflicts.iter().any(|c| c.to_string().contains("ctrl+z")));
    }

    #[test]
    fn no_conflicts_across_distinct_sequences() {
        let entries = vec![
            (b("w"), "move".to_string()),
            (b("e"), "rotate".to_string()),
            (b("r"), "scale".to_string()),
        ];
        assert!(find_conflicts(&entries).is_empty());
    }
}
