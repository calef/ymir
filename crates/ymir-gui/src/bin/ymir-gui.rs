//! `ymir-gui` binary entry point.
//!
//! Opens the main window and runs the eframe event loop. The actual
//! application logic lives in [`ymir_gui::YmirApp`]; this file is just the
//! `main` that wires eframe to it.

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 600.0])
            .with_title("Ymir"),
        ..Default::default()
    };
    eframe::run_native(
        "Ymir",
        native_options,
        Box::new(|cc| Ok(Box::new(ymir_gui::YmirApp::new(cc)))),
    )
}
