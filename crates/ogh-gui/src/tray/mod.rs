#[cfg(target_os = "linux")]
mod linux;
#[cfg(all(test, any(target_os = "linux", windows)))]
mod test_fixtures;
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
    action: crate::tray_menu::TrayAction,
    link: Option<&crate::link::DaemonLink>,
    launcher: &mut crate::launcher::Launcher,
    report: impl FnOnce(String),
) -> std::ops::ControlFlow<()> {
    use std::ops::ControlFlow;

    use crate::tray_menu::TrayAction;

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
