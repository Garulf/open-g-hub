#[cfg(any(target_os = "linux", windows))]
use std::ops::ControlFlow;

#[cfg(any(target_os = "linux", windows))]
use crate::launcher::Launcher;
#[cfg(any(target_os = "linux", windows))]
use crate::link::DaemonLink;
#[cfg(any(target_os = "linux", windows))]
use crate::tray_menu::TrayAction;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::run;
#[cfg(windows)]
pub use windows::run;

/// Runs the tray until the user picks Quit.
#[cfg(not(any(target_os = "linux", windows)))]
pub fn run() -> Result<(), String> {
    Err("not built yet".to_string())
}

/// Carries out one menu action; `Break` means Quit.
#[cfg(any(target_os = "linux", windows))]
fn perform(
    action: TrayAction,
    link: Option<&DaemonLink>,
    launcher: &mut Launcher,
    report: impl FnOnce(String),
) -> ControlFlow<()> {
    match action {
        TrayAction::Request(request) => {
            if let Some(link) = link {
                link.send(request);
            }
        }
        TrayAction::OpenWindow => {
            if let Err(error) = launcher.open() {
                report(error);
            }
        }
        TrayAction::Quit => return ControlFlow::Break(()),
    }
    ControlFlow::Continue(())
}
