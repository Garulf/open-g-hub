#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

use ogh_gui::startup::report_startup_error;

fn main() -> ExitCode {
    match ogh_gui::tray::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report_startup_error("ogh-tray", &error);
            ExitCode::FAILURE
        }
    }
}
