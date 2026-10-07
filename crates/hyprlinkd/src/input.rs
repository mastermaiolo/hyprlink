//! Touchpad e teclado remotos via `/dev/uinput` direto — sem depender do
//! `ydotool`/`ydotoold` (o kernel já carrega `uinput` neste sistema).
//!
//! ponytail: `input.type` só cobre ASCII imprimível (layout US QWERTY fixo).
//! Acentos/emoji/CJK precisariam de outro mecanismo (compose/IBus) — fora do
//! escopo por ora.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use uinput::Device;
use uinput::event::controller::{Controller, Mouse};
use uinput::event::keyboard::{Key, Keyboard};
use uinput::event::relative::{Position, Relative, Wheel};

/// Sem `input.move` nem `up` durante este tempo com um botão premido, solta-se
/// tudo: um telemóvel que adormece/perde a rede a meio de um arrastar não pode
/// deixar o botão preso no PC.
pub const WATCHDOG: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Btn {
    Left,
    Right,
    Middle,
}

impl Btn {
    /// `left` / `right` / `middle`; qualquer outra coisa → `None` (ignora-se).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "middle" => Some(Self::Middle),
            _ => None,
        }
    }

    fn mouse(self) -> Mouse {
        match self {
            Self::Left => Mouse::Left,
            Self::Right => Mouse::Right,
            Self::Middle => Mouse::Middle,
        }
    }
}

/// O que o rato remoto sabe fazer; o dispositivo uinput implementa-o, e os
/// testes usam um falso que só regista os eventos.
pub trait Sink {
    fn button(&mut self, b: Btn, down: bool);
    fn rel(&mut self, dx: i32, dy: i32);
}

/// Rato remoto + os botões que estão premidos agora. Toda a segurança do
/// «nunca deixar o botão preso» vive aqui, independente do uinput.
pub struct Pad<S: Sink> {
    pub sink: S,
    held: Vec<Btn>,
    /// Último `down`/`move` com um botão premido (o relógio do watchdog).
    last: Option<Instant>,
}

impl<S: Sink> Pad<S> {
    pub fn new(sink: S) -> Self {
        Self {
            sink,
            held: Vec::new(),
            last: None,
        }
    }

    #[cfg(test)]
    pub fn held(&self) -> &[Btn] {
        &self.held
    }

    pub fn move_rel(&mut self, dx: i32, dy: i32, now: Instant) {
        self.sink.rel(dx, dy);
        if !self.held.is_empty() {
            self.last = Some(now);
        }
    }

    /// `input.button {button, state}`. `false` = pedido inválido (botão ou
    /// estado desconhecidos), ignorado sem emitir nada. `up` solta **todos**
    /// os botões premidos, não só o nomeado: um `up` perdido de um botão
    /// nunca fica a pairar.
    pub fn button(&mut self, button: &str, state: &str, now: Instant) -> bool {
        let Some(b) = Btn::parse(button) else {
            return false;
        };
        match state {
            "down" => {
                if !self.held.contains(&b) {
                    self.sink.button(b, true);
                    self.held.push(b);
                }
                self.last = Some(now);
                true
            }
            "up" => {
                self.release_all();
                true
            }
            _ => false,
        }
    }

    /// Solta tudo o que estiver premido; devolve quantos eram.
    pub fn release_all(&mut self) -> usize {
        let n = self.held.len();
        for b in self.held.drain(..) {
            self.sink.button(b, false);
        }
        self.last = None;
        n
    }

    /// Watchdog: com botões premidos e sem atividade há mais de `timeout`,
    /// solta tudo. `true` se soltou.
    pub fn expire(&mut self, now: Instant, timeout: Duration) -> bool {
        match self.last {
            Some(t) if !self.held.is_empty() && now.saturating_duration_since(t) > timeout => {
                self.release_all();
                true
            }
            _ => false,
        }
    }
}

/// O dispositivo uinput real.
pub struct UinputSink {
    dev: Device,
}

impl Sink for UinputSink {
    fn button(&mut self, b: Btn, down: bool) {
        let c = Controller::Mouse(b.mouse());
        let _ = if down {
            self.dev.press(&c)
        } else {
            self.dev.release(&c)
        };
    }

    fn rel(&mut self, dx: i32, dy: i32) {
        let _ = self
            .dev
            .position(&Relative::Position(Position::X), dx)
            .and_then(|_| self.dev.position(&Relative::Position(Position::Y), dy));
    }
}

pub struct InputDevice(Mutex<Option<Pad<UinputSink>>>);

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
            Ok(d) => Self(Mutex::new(Some(Pad::new(UinputSink { dev: d })))),
            Err(e) => {
                eprintln!(
                    "[!] /dev/uinput indisponível, touchpad/teclado remotos desativados: {e}"
                );
                Self(Mutex::new(None))
            }
        }
    }

    fn with_pad(&self, f: impl FnOnce(&mut Pad<UinputSink>)) {
        if let Some(pad) = self.0.lock().unwrap().as_mut() {
            f(pad);
            let _ = pad.sink.dev.synchronize();
        }
    }

    fn with_device(&self, f: impl FnOnce(&mut Device) -> uinput::Result<()>) {
        self.with_pad(|pad| {
            let _ = f(&mut pad.sink.dev);
        });
    }

    pub fn move_relative(&self, dx: i32, dy: i32) {
        self.with_pad(|p| p.move_rel(dx, dy, Instant::now()));
    }

    /// `input.button {button, state}`: premir/largar (arrastar, selecionar uma
    /// área). Devolve `false` se o pedido é inválido.
    pub fn button(&self, button: &str, state: &str) -> bool {
        let mut ok = false;
        self.with_pad(|p| ok = p.button(button, state, Instant::now()));
        ok
    }

    /// Solta todos os botões premidos (ligação caída, watchdog). Devolve quantos.
    pub fn release_all(&self) -> usize {
        let mut n = 0;
        self.with_pad(|p| n = p.release_all());
        n
    }

    /// Chamado a cada segundo: solta tudo se um botão ficou preso (ver `WATCHDOG`).
    pub fn watchdog_tick(&self) -> bool {
        let mut released = false;
        self.with_pad(|p| released = p.expire(Instant::now(), WATCHDOG));
        released
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
                let Some((key, shift)) = char_key(c) else {
                    continue;
                };
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
        Key::A,
        Key::B,
        Key::C,
        Key::D,
        Key::E,
        Key::F,
        Key::G,
        Key::H,
        Key::I,
        Key::J,
        Key::K,
        Key::L,
        Key::M,
        Key::N,
        Key::O,
        Key::P,
        Key::Q,
        Key::R,
        Key::S,
        Key::T,
        Key::U,
        Key::V,
        Key::W,
        Key::X,
        Key::Y,
        Key::Z,
    ];
    const DIGITS: [Key; 10] = [
        Key::_0,
        Key::_1,
        Key::_2,
        Key::_3,
        Key::_4,
        Key::_5,
        Key::_6,
        Key::_7,
        Key::_8,
        Key::_9,
    ];
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Clone, Copy)]
    enum Ev {
        Down(Btn),
        Up(Btn),
        Rel(i32, i32),
    }

    #[derive(Default)]
    struct Fake(Vec<Ev>);

    impl Sink for Fake {
        fn button(&mut self, b: Btn, down: bool) {
            self.0.push(if down { Ev::Down(b) } else { Ev::Up(b) });
        }
        fn rel(&mut self, dx: i32, dy: i32) {
            self.0.push(Ev::Rel(dx, dy));
        }
    }

    fn pad() -> Pad<Fake> {
        Pad::new(Fake::default())
    }

    fn evs(p: &Pad<Fake>) -> Vec<Ev> {
        p.sink.0.clone()
    }

    #[test]
    fn premir_mexer_largar_emite_os_eventos_certos_e_pela_ordem() {
        let (mut p, t0) = (pad(), Instant::now());
        assert!(p.button("left", "down", t0));
        p.move_rel(10, -4, t0);
        p.move_rel(3, 0, t0);
        assert!(p.button("left", "up", t0));
        assert_eq!(
            evs(&p),
            [
                Ev::Down(Btn::Left),
                Ev::Rel(10, -4),
                Ev::Rel(3, 0),
                Ev::Up(Btn::Left)
            ]
        );
        assert!(p.held().is_empty());
    }

    #[test]
    fn os_tres_botoes() {
        for (name, b) in [
            ("left", Btn::Left),
            ("right", Btn::Right),
            ("middle", Btn::Middle),
        ] {
            let mut p = pad();
            let t = Instant::now();
            assert!(p.button(name, "down", t));
            assert!(p.button(name, "up", t));
            assert_eq!(evs(&p), [Ev::Down(b), Ev::Up(b)]);
        }
    }

    #[test]
    fn down_repetido_nao_repete_o_evento_e_up_solta_todos() {
        let (mut p, t) = (pad(), Instant::now());
        p.button("left", "down", t);
        p.button("left", "down", t); // um `down` duplicado (reenvio)
        p.button("right", "down", t);
        assert_eq!(evs(&p), [Ev::Down(Btn::Left), Ev::Down(Btn::Right)]);
        // Um só `up` solta o que estiver premido, seja qual for o botão nomeado.
        p.button("middle", "up", t);
        assert_eq!(
            evs(&p)[2..],
            [Ev::Up(Btn::Left), Ev::Up(Btn::Right)],
            "soltou os dois premidos"
        );
        assert!(p.held().is_empty());
    }

    #[test]
    fn up_sem_nada_premido_nao_emite() {
        let (mut p, t) = (pad(), Instant::now());
        assert!(p.button("left", "up", t));
        assert!(evs(&p).is_empty());
    }

    #[test]
    fn estado_ou_botao_invalido_e_ignorado() {
        let (mut p, t) = (pad(), Instant::now());
        for (b, st) in [
            ("left", "press"),
            ("left", ""),
            ("left", "DOWN"),
            ("side", "down"),
            ("", "down"),
            ("left ", "down"),
        ] {
            assert!(!p.button(b, st, t), "{b:?}/{st:?}");
        }
        assert!(evs(&p).is_empty() && p.held().is_empty());
        // Um pedido inválido a meio não larga o que está premido.
        p.button("left", "down", t);
        assert!(!p.button("left", "sideways", t));
        assert_eq!(p.held(), [Btn::Left]);
    }

    #[test]
    fn soltar_na_desconexao() {
        let (mut p, t) = (pad(), Instant::now());
        p.button("left", "down", t);
        p.button("right", "down", t);
        // A ligação caiu: `release_all` (chamado pelo servidor) solta tudo.
        assert_eq!(p.release_all(), 2);
        assert_eq!(evs(&p)[2..], [Ev::Up(Btn::Left), Ev::Up(Btn::Right)]);
        assert!(p.held().is_empty());
        // Uma segunda desconexão não emite nada.
        assert_eq!(p.release_all(), 0);
        assert_eq!(evs(&p).len(), 4);
    }

    #[test]
    fn watchdog_solta_apos_30s_sem_atividade() {
        let (mut p, t0) = (pad(), Instant::now());
        p.button("left", "down", t0);
        // Ainda dentro do prazo: nada.
        assert!(!p.expire(t0 + Duration::from_secs(29), WATCHDOG));
        assert_eq!(p.held(), [Btn::Left]);
        // Passou: solta.
        assert!(p.expire(t0 + Duration::from_secs(31), WATCHDOG));
        assert_eq!(evs(&p), [Ev::Down(Btn::Left), Ev::Up(Btn::Left)]);
        assert!(p.held().is_empty());
        // E já não há nada para o watchdog fazer.
        assert!(!p.expire(t0 + Duration::from_secs(99), WATCHDOG));
    }

    #[test]
    fn mexer_adia_o_watchdog() {
        let (mut p, t0) = (pad(), Instant::now());
        p.button("left", "down", t0);
        // Um arrastar longo: um `move` aos 25 s e outro aos 50 s.
        p.move_rel(1, 1, t0 + Duration::from_secs(25));
        assert!(
            !p.expire(t0 + Duration::from_secs(40), WATCHDOG),
            "15 s desde o move"
        );
        p.move_rel(1, 1, t0 + Duration::from_secs(50));
        assert!(!p.expire(t0 + Duration::from_secs(70), WATCHDOG));
        assert!(
            p.expire(t0 + Duration::from_secs(81), WATCHDOG),
            "31 s sem mexer"
        );
    }

    #[test]
    fn mover_sem_botao_nao_arma_o_watchdog() {
        let (mut p, t0) = (pad(), Instant::now());
        p.move_rel(5, 5, t0);
        assert!(!p.expire(t0 + Duration::from_secs(3600), WATCHDOG));
        assert_eq!(evs(&p), [Ev::Rel(5, 5)]);
    }

    #[test]
    fn nomes_de_botao() {
        assert_eq!(Btn::parse("left"), Some(Btn::Left));
        assert_eq!(Btn::parse("right"), Some(Btn::Right));
        assert_eq!(Btn::parse("middle"), Some(Btn::Middle));
        assert_eq!(Btn::parse("Left"), None);
    }

    /// À mão, com o uinput real: `down` → `move` → `up` pelo dispositivo.
    /// **Cria um rato (e um teclado) virtual no PC e o Hyprland guarda uma
    /// entrada `hyprlink-remote-N` por cada execução** até reiniciar. Só emite
    /// se `HYPRLINK_TEST_FLAG` apontar para um ficheiro que um leitor externo
    /// cria depois de agarrar o dispositivo em exclusivo (EVIOCGRAB), para o
    /// compositor não receber o clique — o que exige poder ler
    /// `/dev/input/event*` (grupo `input` ou root). Sem isso falha sem emitir.
    /// `cargo test -p hyprlinkd -- --ignored manual_arrastar_uinput`
    #[test]
    #[ignore]
    fn manual_arrastar_uinput() {
        let flag = std::env::var("HYPRLINK_TEST_FLAG").expect("HYPRLINK_TEST_FLAG");
        let dev = InputDevice::open();
        for _ in 0..100 {
            if std::path::Path::new(&flag).exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            std::path::Path::new(&flag).exists(),
            "ninguém agarrou o dispositivo"
        );
        assert!(dev.button("left", "down"));
        dev.move_relative(7, 3);
        assert!(!dev.button("left", "sideways"), "inválido ignorado");
        assert!(dev.button("left", "up"));
        // Deixa um botão premido e larga-o como na desconexão.
        assert!(dev.button("right", "down"));
        assert_eq!(dev.release_all(), 1);
        std::thread::sleep(Duration::from_millis(500));
    }
}
