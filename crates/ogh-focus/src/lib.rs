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

/// What the focus watcher reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusEvent {
    /// The lowercased name of the focused app, or `None` when focus goes to
    /// nothing.
    Focus(Option<String>),
    /// Focus cannot be followed right now; `Focus` events resume once it can.
    Unavailable(String),
}

/// Starts watching the focus and reports it as `FocusEvent`s.
pub fn spawn(events: Sender<FocusEvent>) -> Result<JoinHandle<()>, Unavailable> {
    spawn_platform(events)
}

#[cfg(windows)]
fn spawn_platform(events: Sender<FocusEvent>) -> Result<JoinHandle<()>, Unavailable> {
    windows::spawn(events)
}

#[cfg(target_os = "linux")]
fn spawn_platform(events: Sender<FocusEvent>) -> Result<JoinHandle<()>, Unavailable> {
    hyprland::spawn(events)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn spawn_platform(_events: Sender<FocusEvent>) -> Result<JoinHandle<()>, Unavailable> {
    Err(Unavailable(
        "focus switching is not supported on this platform".to_string(),
    ))
}
