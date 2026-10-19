//! Reports which application has the focus: the foreground process on Windows,
//! the active window class on Hyprland.

#[cfg(unix)]
pub mod hyprland;
#[cfg(windows)]
mod windows;

use std::fmt;
use std::thread::JoinHandle;

use crossbeam_channel::Sender;

/// Why focus switching cannot work on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unavailable(pub String);

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Starts watching the focus. Each focus change sends the lowercased app name,
/// or `None` when focus goes to nothing.
pub fn spawn(on_focus: Sender<Option<String>>) -> Result<JoinHandle<()>, Unavailable> {
    spawn_platform(on_focus)
}

#[cfg(windows)]
fn spawn_platform(on_focus: Sender<Option<String>>) -> Result<JoinHandle<()>, Unavailable> {
    windows::spawn(on_focus)
}

#[cfg(target_os = "linux")]
fn spawn_platform(on_focus: Sender<Option<String>>) -> Result<JoinHandle<()>, Unavailable> {
    hyprland::spawn(on_focus)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn spawn_platform(_on_focus: Sender<Option<String>>) -> Result<JoinHandle<()>, Unavailable> {
    Err(Unavailable(
        "focus switching is not supported on this platform".to_string(),
    ))
}
