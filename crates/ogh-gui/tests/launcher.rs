use std::fs;
use std::path::Path;
#[cfg(unix)]
use std::thread::sleep;
#[cfg(unix)]
use std::time::{Duration, Instant};

use ogh_gui::launcher::{Launcher, gui_path};

const GUI_NAME: &str = if cfg!(windows) {
    "ogh-gui.exe"
} else {
    "ogh-gui"
};

#[test]
fn gui_path_prefers_the_sibling() {
    let dir = tempfile::tempdir().unwrap();
    let current = dir.path().join("ogh-tray");
    let sibling = dir.path().join(GUI_NAME);
    fs::write(&current, "").unwrap();
    fs::write(&sibling, "").unwrap();

    assert_eq!(gui_path(&current), sibling);
}

#[test]
fn gui_path_falls_back_to_the_name() {
    let dir = tempfile::tempdir().unwrap();
    let current = dir.path().join("ogh-tray");
    fs::write(&current, "").unwrap();

    assert_eq!(gui_path(&current), Path::new(GUI_NAME));
}

#[cfg(unix)]
fn count(log: &Path, word: &str) -> usize {
    fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter(|line| *line == word)
        .count()
}

#[cfg(unix)]
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
#[test]
fn launcher_does_not_start_a_second_window() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log");
    let stop = dir.path().join("stop");
    let script = dir.path().join("fake-gui");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\necho start >> '{log}'\nwhile [ ! -e '{stop}' ]; do sleep 0.02; done\necho end >> '{log}'\n",
            log = log.display(),
            stop = stop.display(),
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

    let mut launcher = Launcher::new(script);
    launcher.open().unwrap();
    launcher.open().unwrap();
    wait_until("the first window", || count(&log, "start") >= 1);
    sleep(Duration::from_millis(300));
    launcher.open().unwrap();
    sleep(Duration::from_millis(300));
    assert_eq!(count(&log, "start"), 1);

    fs::write(&stop, "").unwrap();
    wait_until("the first window to exit", || count(&log, "end") == 1);
    fs::remove_file(&stop).unwrap();
    wait_until("a new window", || {
        launcher.open().unwrap();
        count(&log, "start") == 2
    });
    sleep(Duration::from_millis(300));
    assert_eq!(count(&log, "start"), 2);

    fs::write(&stop, "").unwrap();
    wait_until("the second window to exit", || count(&log, "end") == 2);
}

#[test]
fn open_reports_a_missing_program() {
    let dir = tempfile::tempdir().unwrap();
    let mut launcher = Launcher::new(dir.path().join("missing"));

    let error = launcher.open().unwrap_err();

    assert!(error.contains("missing"), "{error}");
}
