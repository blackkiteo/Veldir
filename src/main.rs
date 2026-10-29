#![cfg_attr(windows, windows_subsystem = "windows")]
mod app;
mod fs_ops;
mod search;
mod preview;
mod shell_clipboard;
mod image_view;
mod native_icons;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Veldir · File Manager")
            .with_inner_size([1230.0, 790.0])
            .with_min_inner_size([900.0, 570.0])
            .with_drag_and_drop(true),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native("Veldir", options, Box::new(|cc| Ok(Box::new(app::Veldir::new(cc)))))
}
