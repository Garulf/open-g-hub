#![cfg(unix)]

use std::fs::{File, FileTimes};
use std::io::Write;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::unbounded;
use ogh_focus::hyprland::{parse_event, resolve, socket_path, watch};

const WAIT: Duration = Duration::from_secs(5);

#[test]
fn parses_activewindow() {
    assert_eq!(
        parse_event("activewindow>>CS2,Counter-Strike 2"),
        Some(Some("cs2".to_string()))
    );
}

#[test]
fn title_with_commas_keeps_the_class() {
    assert_eq!(
        parse_event("activewindow>>Firefox,a, b, c"),
        Some(Some("firefox".to_string()))
    );
}

#[test]
fn empty_class_is_no_app() {
    assert_eq!(parse_event("activewindow>>,"), Some(None));
}

#[test]
fn other_events_are_ignored() {
    assert_eq!(parse_event("workspace>>2"), None);
    assert_eq!(parse_event("activewindowv2>>abc123"), None);
    assert_eq!(parse_event(""), None);
}

fn fake_socket(runtime: &Path, signature: &str, modified: SystemTime) -> PathBuf {
    let dir = runtime.join("hypr").join(signature);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(".socket2.sock");
    let file = File::create(&path).unwrap();
    file.set_times(FileTimes::new().set_modified(modified))
        .unwrap();
    path
}

#[test]
fn socket_path_uses_the_signature() {
    let runtime = tempfile::tempdir().unwrap();
    let path = fake_socket(runtime.path(), "abc", SystemTime::now());
    fake_socket(runtime.path(), "def", SystemTime::now());
    assert_eq!(socket_path(runtime.path(), Some("abc")), Some(path));
    assert_eq!(socket_path(runtime.path(), Some("missing")), None);
}

#[test]
fn socket_path_without_signature_picks_the_newest() {
    let runtime = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    fake_socket(runtime.path(), "old", now - Duration::from_secs(1000));
    let newest = fake_socket(runtime.path(), "new", now);
    fake_socket(runtime.path(), "older", now - Duration::from_secs(2000));
    assert_eq!(socket_path(runtime.path(), None), Some(newest));
}

#[test]
fn watcher_reads_events_from_a_socket() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("socket2");
    let listener = UnixListener::bind(&path).unwrap();
    let (tx, rx) = unbounded();
    let socket = path.clone();
    let _watcher = watch(move || Some(socket.clone()), Duration::from_millis(20), tx);
    let (mut conn, _) = listener.accept().unwrap();
    conn.write_all(b"activewindow>>CS2,Counter-Strike 2\nworkspace>>2\nactivewindow>>,\nactivewindow>>Discord,General\n")
        .unwrap();
    assert_eq!(rx.recv_timeout(WAIT), Ok(Some("cs2".to_string())));
    assert_eq!(rx.recv_timeout(WAIT), Ok(None));
    assert_eq!(rx.recv_timeout(WAIT), Ok(Some("discord".to_string())));
}

#[test]
fn watcher_reconnects_after_the_socket_closes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("socket2");
    let listener = UnixListener::bind(&path).unwrap();
    let (tx, rx) = unbounded();
    let socket = path.clone();
    let _watcher = watch(move || Some(socket.clone()), Duration::from_millis(20), tx);
    let (mut first, _) = listener.accept().unwrap();
    first.write_all(b"activewindow>>one,t\n").unwrap();
    drop(first);
    assert_eq!(rx.recv_timeout(WAIT), Ok(Some("one".to_string())));
    let (mut second, _) = listener.accept().unwrap();
    second.write_all(b"activewindow>>two,t\n").unwrap();
    assert_eq!(rx.recv_timeout(WAIT), Ok(Some("two".to_string())));
}

#[test]
fn stale_signature_falls_back_to_the_newest() {
    let runtime = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    fake_socket(runtime.path(), "old", now - Duration::from_secs(1000));
    let newest = fake_socket(runtime.path(), "new", now);
    assert_eq!(resolve(runtime.path(), Some("gone")), Some(newest.clone()));
    assert_eq!(resolve(runtime.path(), None), Some(newest));
    let old = runtime.path().join("hypr/old/.socket2.sock");
    assert_eq!(resolve(runtime.path(), Some("old")), Some(old));
    assert_eq!(
        resolve(tempfile::tempdir().unwrap().path(), Some("gone")),
        None
    );
}

#[test]
fn watcher_skips_non_utf8_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("socket2");
    let listener = UnixListener::bind(&path).unwrap();
    let (tx, rx) = unbounded();
    let socket = path.clone();
    let _watcher = watch(move || Some(socket.clone()), Duration::from_millis(20), tx);
    let (mut conn, _) = listener.accept().unwrap();
    conn.write_all(b"activewindow>>\xff\xfe,bad\nactivewindow>>ok,fine\n")
        .unwrap();
    assert_eq!(rx.recv_timeout(WAIT), Ok(Some("ok".to_string())));
    assert!(rx.try_recv().is_err());
}

#[test]
fn watcher_exits_after_the_receiver_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("socket2");
    let listener = UnixListener::bind(&path).unwrap();
    let (tx, rx) = unbounded();
    let socket = path.clone();
    let watcher = watch(move || Some(socket.clone()), Duration::from_millis(20), tx);
    let (mut conn, _) = listener.accept().unwrap();
    drop(rx);
    conn.write_all(b"activewindow>>one,t\n").unwrap();
    let deadline = Instant::now() + WAIT;
    while !watcher.is_finished() {
        assert!(Instant::now() < deadline, "the watcher did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
    watcher.join().unwrap();
}
