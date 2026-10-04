//! Tokens de cor da GUI — alinhados com o app Android e com
//! `code-spec-iced.md` §1 (design handoff da Ronda 6). Centralizados aqui
//! porque `gui.rs` repetia os mesmos `Color::from_rgba(...)` de cartão verde
//! em quatro telas diferentes.

use iced::Color;

// acento por estado — UM único acento ativo em toda a UI a cada momento
pub const GREEN: Color = Color::from_rgb(0x3D as f32 / 255.0, 0xFF as f32 / 255.0, 0x9E as f32 / 255.0);
pub const AMBER: Color = Color::from_rgb(0xFF as f32 / 255.0, 0xB0 as f32 / 255.0, 0x20 as f32 / 255.0);
pub const RED: Color = Color::from_rgb(0xFF as f32 / 255.0, 0x47 as f32 / 255.0, 0x57 as f32 / 255.0);
pub const MUTED: Color = Color::from_rgb(0x5E as f32 / 255.0, 0x5E as f32 / 255.0, 0x5E as f32 / 255.0);

// texto
pub const TEXT: Color = Color::from_rgb(0xFF as f32 / 255.0, 0xFF as f32 / 255.0, 0xFF as f32 / 255.0);
pub const TEXT_1: Color = Color::from_rgb(0xE4 as f32 / 255.0, 0xE4 as f32 / 255.0, 0xE4 as f32 / 255.0);
pub const TEXT_2: Color = Color::from_rgb(0xB4 as f32 / 255.0, 0xB4 as f32 / 255.0, 0xB4 as f32 / 255.0);
pub const TEXT_3: Color = Color::from_rgb(0x8A as f32 / 255.0, 0x8A as f32 / 255.0, 0x8A as f32 / 255.0);
pub const TEXT_4: Color = Color::from_rgb(0x6E as f32 / 255.0, 0x6E as f32 / 255.0, 0x6E as f32 / 255.0);
pub const TEXT_5: Color = Color::from_rgb(0x4E as f32 / 255.0, 0x4E as f32 / 255.0, 0x4E as f32 / 255.0);
pub const TEXT_6: Color = Color::from_rgb(0x3A as f32 / 255.0, 0x3A as f32 / 255.0, 0x3A as f32 / 255.0);

// superfícies
pub const WINDOW_BG: Color = Color { r: 0.031, g: 0.035, b: 0.047, a: 0.82 };
pub const TERMINAL: Color = Color::from_rgb(0x07 as f32 / 255.0, 0x07 as f32 / 255.0, 0x07 as f32 / 255.0);
pub const GLASS: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 0.045 };
pub const GLASS_BRD: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 0.08 };
pub const WINDOW_BRD: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 0.10 };

// pares fundo/borda por estado (cartões)
pub const GREEN_BG: Color = Color::from_rgb(0x07 as f32 / 255.0, 0x15 as f32 / 255.0, 0x10 as f32 / 255.0);
pub const GREEN_BRD: Color = Color::from_rgb(0x1E as f32 / 255.0, 0x3E as f32 / 255.0, 0x30 as f32 / 255.0);
pub const AMBER_BG: Color = Color::from_rgb(0x15 as f32 / 255.0, 0x0F as f32 / 255.0, 0x03 as f32 / 255.0);
pub const AMBER_BRD: Color = Color::from_rgb(0x3A as f32 / 255.0, 0x2C as f32 / 255.0, 0x12 as f32 / 255.0);
pub const RED_BRD: Color = Color::from_rgb(0x2A as f32 / 255.0, 0x14 as f32 / 255.0, 0x18 as f32 / 255.0);

// bordas neutras
pub const BRD_1: Color = Color::from_rgb(0x1C as f32 / 255.0, 0x1C as f32 / 255.0, 0x1C as f32 / 255.0);
pub const DIVIDER: Color = Color::from_rgb(0x14 as f32 / 255.0, 0x14 as f32 / 255.0, 0x14 as f32 / 255.0);
