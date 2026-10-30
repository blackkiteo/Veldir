#![cfg_attr(windows, windows_subsystem = "windows")]
mod app;
mod filters;
mod folder_size;
mod folder_watch;
mod fs_ops;
mod image_view;
mod native_icons;
mod pdf_view;
mod preview;
mod search;
mod shell_clipboard;
mod transfer;
mod ui_state;

use eframe::egui;

fn main() -> eframe::Result {
    if std::env::args().any(|a| a == "--version") {
        println!(
            "Veldir {} ({})",
            env!("CARGO_PKG_VERSION"),
            env!("VELDIR_BUILD_COMMIT")
        );
        return Ok(());
    }
    let portable = std::env::args().any(|a| a == "--portable");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Veldir · File Manager")
            .with_inner_size([1230.0, 790.0])
            .with_min_inner_size([900.0, 570.0])
            .with_drag_and_drop(true),
        persistence_path: if portable {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("VeldirData")))
        } else {
            None
        },
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Veldir",
        options,
        Box::new(|cc| Ok(Box::new(app::Veldir::new(cc)))),
    )
}
