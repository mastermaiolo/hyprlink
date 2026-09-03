//! Touchpad e teclado remotos via `/dev/uinput` direto — sem depender do
//! `ydotool`/`ydotoold` (o kernel já carrega `uinput` neste sistema).
//!
//! ponytail: `input.type` só cobre ASCII imprimível (layout US QWERTY fixo).
//! Acentos/emoji/CJK precisariam de outro mecanismo (compose/IBus) — fora do
//! escopo por ora.

use std::sync::Mutex;

use uinput::event::controller::{Controller, Mouse};
use uinput::event::keyboard::{Key, Keyboard};
use uinput::event::relative::{Position, Relative, Wheel};
use uinput::Device;

pub struct InputDevice(Mutex<Option<Device>>);

impl InputDevice {
    pub fn open() -> Self {
        let device = uinput::default()
            .and_then(|b| b.name("hyprlink-remote"))
            .and_then(|b| b.event(Controller::All))
            .and_then(|b| b.event(Relative::Position(Position::X)))
            .and_then(|b| b.event(Relative::Position(Position::Y)))
            .and_then(|b| b.event(Relative::Wheel(Wheel::Vertical)))
            .and_then(|b| b.event(Relative::Wheel(Wheel::Horizontal)))
            .and_then(|b| b.event(Keyboard::All))
            .and_then(|b| b.create());
        match device {
            Ok(d) => Self(Mutex::new(Some(d))),
            Err(e) => {
                eprintln!("[!] /dev/uinput indisponível, touchpad/teclado remotos desativados: {e}");
                Self(Mutex::new(None))
            }
        }
    }

    fn with_device(&self, f: impl FnOnce(&mut Device) -> uinput::Result<()>) {
        if let Some(device) = self.0.lock().unwrap().as_mut() {
            let _ = f(device).and_then(|_| device.synchronize());
        }
    }

    pub fn move_relative(&self, dx: i32, dy: i32) {
        self.with_device(|d| {
            d.position(&Relative::Position(Position::X), dx)?;
            d.position(&Relative::Position(Position::Y), dy)
        });
    }

    pub fn scroll(&self, dx: i32, dy: i32) {
        self.with_device(|d| {
            if dy != 0 {
                d.position(&Relative::Wheel(Wheel::Vertical), -dy)?;
            }
            if dx != 0 {
                d.position(&Relative::Wheel(Wheel::Horizontal), dx)?;
            }
            Ok(())
        });
    }

    pub fn click(&self, button: &str) {
        let btn = match button {
            "right" => Mouse::Right,
            "middle" => Mouse::Middle,
            _ => Mouse::Left,
        };
        self.with_device(|d| d.click(&Controller::Mouse(btn)));
    }

    pub fn key(&self, name: &str) {
        if let Some(key) = special_key(name) {
            self.with_device(|d| d.click(&Keyboard::Key(key)));
        }
    }

    pub fn type_text(&self, text: &str) {
        self.with_device(|d| {
            for c in text.chars() {
                let Some((key, shift)) = char_key(c) else { continue };
                if shift {
                    d.press(&Keyboard::Key(Key::LeftShift))?;
                }
                d.click(&Keyboard::Key(key))?;
                if shift {
                    d.release(&Keyboard::Key(Key::LeftShift))?;
                }
            }
            Ok(())
        });
    }
}

fn special_key(name: &str) -> Option<Key> {
    Some(match name.to_ascii_lowercase().as_str() {
        "return" | "enter" => Key::Enter,
        "backspace" => Key::BackSpace,
        "escape" | "esc" => Key::Esc,
        "tab" => Key::Tab,
        "delete" | "del" => Key::Delete,
        "space" | "esp" => Key::Space,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "insert" => Key::Insert,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        _ => return None,
    })
}

/// `(tecla, precisa de shift)` pra um caractere ASCII imprimível, layout US.
fn char_key(c: char) -> Option<(Key, bool)> {
    const LETTERS: [Key; 26] = [
        Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I, Key::J, Key::K, Key::L, Key::M,
        Key::N, Key::O, Key::P, Key::Q, Key::R, Key::S, Key::T, Key::U, Key::V, Key::W, Key::X, Key::Y, Key::Z,
    ];
    const DIGITS: [Key; 10] =
        [Key::_0, Key::_1, Key::_2, Key::_3, Key::_4, Key::_5, Key::_6, Key::_7, Key::_8, Key::_9];
    const DIGIT_SYMBOLS: [char; 10] = ['!', '@', '#', '$', '%', '^', '&', '*', '(', ')'];

    if c.is_ascii_lowercase() {
        return Some((LETTERS[(c as u8 - b'a') as usize], false));
    }
    if c.is_ascii_uppercase() {
        return Some((LETTERS[(c as u8 - b'A') as usize], true));
    }
    if c.is_ascii_digit() {
        return Some((DIGITS[(c as u8 - b'0') as usize], false));
    }
    if let Some(i) = DIGIT_SYMBOLS.iter().position(|&s| s == c) {
        return Some((DIGITS[i], true));
    }
    Some(match c {
        ' ' => (Key::Space, false),
        '\n' => (Key::Enter, false),
        '\t' => (Key::Tab, false),
        '-' => (Key::Minus, false),
        '_' => (Key::Minus, true),
        '=' => (Key::Equal, false),
        '+' => (Key::Equal, true),
        '[' => (Key::LeftBrace, false),
        '{' => (Key::LeftBrace, true),
        ']' => (Key::RightBrace, false),
        '}' => (Key::RightBrace, true),
        ';' => (Key::SemiColon, false),
        ':' => (Key::SemiColon, true),
        '\'' => (Key::Apostrophe, false),
        '"' => (Key::Apostrophe, true),
        '`' => (Key::Grave, false),
        '~' => (Key::Grave, true),
        '\\' => (Key::BackSlash, false),
        '|' => (Key::BackSlash, true),
        ',' => (Key::Comma, false),
        '<' => (Key::Comma, true),
        '.' => (Key::Dot, false),
        '>' => (Key::Dot, true),
        '/' => (Key::Slash, false),
        '?' => (Key::Slash, true),
        _ => return None, // fora do ASCII coberto — ver ponytail no topo do arquivo
    })
}
