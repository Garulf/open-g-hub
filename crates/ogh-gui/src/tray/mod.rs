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

/// Carries out one menu action; `Break` means Quit. Opening the window sets or clears the
/// last error.
#[cfg(any(target_os = "linux", windows))]
fn perform(
    action: crate::tray_menu::TrayAction,
    link: Option<&crate::link::DaemonLink>,
    launcher: &mut crate::launcher::Launcher,
    set_error: impl FnOnce(Option<String>),
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
            set_error(launcher.open().err());
        }
        TrayAction::Quit => return ControlFlow::Break(()),
    }
    ControlFlow::Continue(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::ops::ControlFlow;

    use super::perform;
    use crate::launcher::Launcher;
    use crate::tray_menu::TrayAction;

    #[test]
    fn a_successful_open_clears_the_last_error() {
        let mut launcher = Launcher::new("true".into());
        let mut last_error = Some("could not start ogh-gui".to_string());

        let flow = perform(TrayAction::OpenWindow, None, &mut launcher, |error| {
            last_error = error
        });

        assert_eq!(flow, ControlFlow::Continue(()));
        assert_eq!(last_error, None);
    }

    #[test]
    fn a_failed_open_sets_the_last_error() {
        let mut launcher = Launcher::new("/nonexistent/ogh-gui".into());
        let mut last_error = None;

        let _ = perform(TrayAction::OpenWindow, None, &mut launcher, |error| {
            last_error = error
        });

        assert!(last_error.is_some_and(|error| error.contains("/nonexistent/ogh-gui")));
    }
}
