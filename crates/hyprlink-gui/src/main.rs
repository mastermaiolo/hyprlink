//! HYPRLINK — the desktop face of the Android ⇄ Hyprland link.
//!
//! Runs as an `iced::daemon`: the window can close while HYPRLINK keeps
//! living in the tray. `hyprlink-gui --hidden` starts straight into the tray.

mod app;
mod graphics;
mod pages;
mod theme;
mod tray;
mod ui;
mod views;

use app::App;
use hyprlink_gui::{host, link};

fn main() -> iced::Result {
    let mut daemon = iced::daemon(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(|_: &App, _| theme::theme())
        .default_font(theme::SANS)
        .antialiasing(true);
    for font in theme::FONTS {
        daemon = daemon.font(*font);
    }
    daemon.run()
}
