#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

use eframe::egui::{IconData, ViewportBuilder};
use ogh_gui::GuiApp;
use ogh_gui::icon::{self, IconSize};
use ogh_gui::startup::report_startup_error;

fn main() -> ExitCode {
    let (rgba, width, height) = icon::rgba(IconSize::Large);
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("open-g-hub")
            .with_inner_size([820.0, 560.0])
            .with_icon(IconData {
                rgba,
                width,
                height,
            }),
        ..Default::default()
    };
    let started = eframe::run_native(
        "open-g-hub",
        options,
        Box::new(|creation| Ok(Box::new(GuiApp::new(&creation.egui_ctx)))),
    );
    match started {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report_startup_error("ogh-gui", &error.to_string());
            ExitCode::FAILURE
        }
    }
}
