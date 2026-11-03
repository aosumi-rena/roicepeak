//! Logical shortcut keys and chord display.
use eframe::egui;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Key {
    Letter(char),
    Symbol(char),
    Digit(u8),
    Function(u8),
    Space,
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Insert,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
}

impl Key {
    pub(super) fn valid(self) -> bool {
        match self {
            Self::Letter(c) => c.is_ascii_uppercase(),
            Self::Symbol(c) => ":,\\/|?![]{}\x60-.+=;\x27".contains(c),
            Self::Digit(n) => n <= 9,
            Self::Function(n) => (1..=24).contains(&n),
            _ => true,
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Letter(c) | Self::Symbol(c) => return c.to_string(),
            Self::Digit(n) => return n.to_string(),
            Self::Function(n) => return format!("F{n}"),
            Self::Space => "Space",
            Self::Enter => "Enter",
            Self::Escape => "Escape",
            Self::Tab => "Tab",
            Self::Backspace => "Backspace",
            Self::Delete => "Delete",
            Self::Insert => "Insert",
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Home => "Home",
            Self::End => "End",
            Self::PageUp => "Page Up",
            Self::PageDown => "Page Down",
        }
        .into()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Modifiers {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
    pub super_key: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        control: false,
        shift: false,
        alt: false,
        super_key: false,
    };
    pub const CONTROL: Self = Self {
        control: true,
        ..Self::NONE
    };
    pub const CONTROL_SHIFT: Self = Self {
        control: true,
        shift: true,
        ..Self::NONE
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chord {
    pub key: Key,
    pub modifiers: Modifiers,
}

impl Chord {
    pub const fn new(key: Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }

    pub fn label(self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.control {
            parts.push("Ctrl".to_owned());
        }
        if self.modifiers.alt {
            parts.push("Alt".to_owned());
        }
        if self.modifiers.shift {
            parts.push("Shift".to_owned());
        }
        if self.modifiers.super_key {
            parts.push("Super".to_owned());
        }
        parts.push(self.key.label());
        parts.join("+")
    }

    /// Logical keys; `command` is egui's alias for Ctrl on Windows, so it is not
    /// an extra independent modifier. mac_cmd remains explicit Super.
    pub fn from_egui(key: egui::Key, modifiers: egui::Modifiers) -> Option<Self> {
        let key = match key {
            egui::Key::Colon => Key::Symbol(':'),
            egui::Key::Comma => Key::Symbol(','),
            egui::Key::Backslash => Key::Symbol('\\'),
            egui::Key::Slash => Key::Symbol('/'),
            egui::Key::Pipe => Key::Symbol('|'),
            egui::Key::Questionmark => Key::Symbol('?'),
            egui::Key::Exclamationmark => Key::Symbol('!'),
            egui::Key::OpenBracket => Key::Symbol('['),
            egui::Key::CloseBracket => Key::Symbol(']'),
            egui::Key::OpenCurlyBracket => Key::Symbol('{'),
            egui::Key::CloseCurlyBracket => Key::Symbol('}'),
            egui::Key::Backtick => Key::Symbol('`'),
            egui::Key::Minus => Key::Symbol('-'),
            egui::Key::Period => Key::Symbol('.'),
            egui::Key::Plus => Key::Symbol('+'),
            egui::Key::Equals => Key::Symbol('='),
            egui::Key::Semicolon => Key::Symbol(';'),
            egui::Key::Quote => Key::Symbol('\''),
            egui::Key::A => Key::Letter('A'),
            egui::Key::B => Key::Letter('B'),
            egui::Key::C => Key::Letter('C'),
            egui::Key::D => Key::Letter('D'),
            egui::Key::E => Key::Letter('E'),
            egui::Key::F => Key::Letter('F'),
            egui::Key::G => Key::Letter('G'),
            egui::Key::H => Key::Letter('H'),
            egui::Key::I => Key::Letter('I'),
            egui::Key::J => Key::Letter('J'),
            egui::Key::K => Key::Letter('K'),
            egui::Key::L => Key::Letter('L'),
            egui::Key::M => Key::Letter('M'),
            egui::Key::N => Key::Letter('N'),
            egui::Key::O => Key::Letter('O'),
            egui::Key::P => Key::Letter('P'),
            egui::Key::Q => Key::Letter('Q'),
            egui::Key::R => Key::Letter('R'),
            egui::Key::S => Key::Letter('S'),
            egui::Key::T => Key::Letter('T'),
            egui::Key::U => Key::Letter('U'),
            egui::Key::V => Key::Letter('V'),
            egui::Key::W => Key::Letter('W'),
            egui::Key::X => Key::Letter('X'),
            egui::Key::Y => Key::Letter('Y'),
            egui::Key::Z => Key::Letter('Z'),
            egui::Key::Num0 => Key::Digit(0),
            egui::Key::Num1 => Key::Digit(1),
            egui::Key::Num2 => Key::Digit(2),
            egui::Key::Num3 => Key::Digit(3),
            egui::Key::Num4 => Key::Digit(4),
            egui::Key::Num5 => Key::Digit(5),
            egui::Key::Num6 => Key::Digit(6),
            egui::Key::Num7 => Key::Digit(7),
            egui::Key::Num8 => Key::Digit(8),
            egui::Key::Num9 => Key::Digit(9),
            egui::Key::F1 => Key::Function(1),
            egui::Key::F2 => Key::Function(2),
            egui::Key::F3 => Key::Function(3),
            egui::Key::F4 => Key::Function(4),
            egui::Key::F5 => Key::Function(5),
            egui::Key::F6 => Key::Function(6),
            egui::Key::F7 => Key::Function(7),
            egui::Key::F8 => Key::Function(8),
            egui::Key::F9 => Key::Function(9),
            egui::Key::F10 => Key::Function(10),
            egui::Key::F11 => Key::Function(11),
            egui::Key::F12 => Key::Function(12),
            egui::Key::F13 => Key::Function(13),
            egui::Key::F14 => Key::Function(14),
            egui::Key::F15 => Key::Function(15),
            egui::Key::F16 => Key::Function(16),
            egui::Key::F17 => Key::Function(17),
            egui::Key::F18 => Key::Function(18),
            egui::Key::F19 => Key::Function(19),
            egui::Key::F20 => Key::Function(20),
            egui::Key::F21 => Key::Function(21),
            egui::Key::F22 => Key::Function(22),
            egui::Key::F23 => Key::Function(23),
            egui::Key::F24 => Key::Function(24),
            egui::Key::Space => Key::Space,
            egui::Key::Enter => Key::Enter,
            egui::Key::Escape => Key::Escape,
            egui::Key::Tab => Key::Tab,
            egui::Key::Backspace => Key::Backspace,
            egui::Key::Delete => Key::Delete,
            egui::Key::Insert => Key::Insert,
            egui::Key::ArrowUp => Key::Up,
            egui::Key::ArrowDown => Key::Down,
            egui::Key::ArrowLeft => Key::Left,
            egui::Key::ArrowRight => Key::Right,
            egui::Key::Home => Key::Home,
            egui::Key::End => Key::End,
            egui::Key::PageUp => Key::PageUp,
            egui::Key::PageDown => Key::PageDown,
            _ => return None,
        };
        Some(Self::new(
            key,
            Modifiers {
                control: modifiers.ctrl,
                shift: modifiers.shift,
                alt: modifiers.alt,
                super_key: modifiers.mac_cmd,
            },
        ))
    }
}
