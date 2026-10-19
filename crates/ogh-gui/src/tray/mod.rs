#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::run;

/// Runs the tray until the user picks Quit.
#[cfg(not(target_os = "linux"))]
pub fn run() -> Result<(), String> {
    Err("not built yet".to_string())
}
