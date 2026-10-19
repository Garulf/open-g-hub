#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    match ogh_gui::tray::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ogh-tray: {error}");
            ExitCode::FAILURE
        }
    }
}
