use std::path::Path;

use ogh_cli::autostart::{autostart_command, status_text};

#[test]
fn autostart_command_quotes_paths_with_spaces() {
    let command = autostart_command(Path::new(r"C:\Program Files\open-g-hub\ogh-daemon.exe"));
    assert_eq!(command, r#""C:\Program Files\open-g-hub\ogh-daemon.exe""#);
}

#[test]
fn autostart_command_quotes_paths_without_spaces() {
    let command = autostart_command(Path::new(r"C:\tools\ogh-daemon.exe"));
    assert_eq!(command, r#""C:\tools\ogh-daemon.exe""#);
}

#[test]
fn tray_autostart_command_quotes_the_path() {
    let command = autostart_command(Path::new(r"C:\Program Files\open-g-hub\ogh-tray.exe"));
    assert_eq!(command, r#""C:\Program Files\open-g-hub\ogh-tray.exe""#);
}

#[test]
fn status_text_reports_both_entries() {
    assert_eq!(status_text(true, true), "ogh-daemon: on\nogh-tray: on");
    assert_eq!(status_text(true, false), "ogh-daemon: on\nogh-tray: off");
    assert_eq!(status_text(false, true), "ogh-daemon: off\nogh-tray: on");
    assert_eq!(status_text(false, false), "ogh-daemon: off\nogh-tray: off");
}

#[cfg(not(windows))]
#[test]
fn autostart_points_to_the_service_off_windows() {
    use ogh_cli::autostart::{AutostartCommand, run};

    for command in [
        AutostartCommand::On,
        AutostartCommand::Off,
        AutostartCommand::Status,
    ] {
        let mut out = Vec::new();
        run(command, &mut out).expect("a local no-op");
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "use the systemd user unit (or the NixOS/Home Manager module)\n"
        );
    }
}
