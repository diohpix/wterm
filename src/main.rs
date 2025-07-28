use eframe::egui;

mod app;
mod ime;
mod terminal;
mod utils;

use app::TerminalApp;

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 768.0])
            .with_resizable(true) // Make window resizable
            .with_title("WTerm - 터미널"), // Window title
        ..Default::default()
    };

    let _result = eframe::run_native(
        "WTerm",
        options,
        Box::new(|cc| {
            Ok(Box::new(
                TerminalApp::new(cc).expect("Failed to create terminal app"),
            ))
        }),
    );
}
