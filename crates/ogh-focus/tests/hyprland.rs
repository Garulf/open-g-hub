#![cfg(unix)]

use std::fs::{File, FileTimes};
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::unbounded;
use ogh_focus::FocusEvent;
use ogh_focus::hyprland::{
    parse_event, query_active_window, resolve, socket_path, spawn_in, watch,
};

const WAIT: Duration = Duration::from_secs(5);
const RETRY: Duration = Duration::from_millis(20);
const NO_SOCKET: &str = "focus switching needs Hyprland on Linux (no Hyprland socket found)";

fn focus(app: &str) -> FocusEvent {
    FocusEvent::Focus(Some(app.to_string()))
}

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
    assert_eq!(rx.recv_timeout(WAIT), Ok(focus("cs2")));
    assert_eq!(rx.recv_timeout(WAIT), Ok(FocusEvent::Focus(None)));
    assert_eq!(rx.recv_timeout(WAIT), Ok(focus("discord")));
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
    assert_eq!(rx.recv_timeout(WAIT), Ok(focus("one")));
    let (mut second, _) = listener.accept().unwrap();
    second.write_all(b"activewindow>>two,t\n").unwrap();
    assert_eq!(rx.recv_timeout(WAIT), Ok(focus("two")));
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
    assert_eq!(rx.recv_timeout(WAIT), Ok(focus("ok")));
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

#[test]
fn without_a_runtime_dir_focus_is_unavailable() {
    let (tx, _rx) = unbounded();
    assert!(spawn_in(None, Duration::from_millis(20), tx).is_err());
}

#[test]
fn watcher_waits_for_hyprland_to_create_its_socket() {
    let runtime = tempfile::tempdir().unwrap();
    let (tx, rx) = unbounded();
    let watcher = spawn_in(
        Some(runtime.path().to_path_buf()),
        Duration::from_millis(20),
        tx,
    );
    assert!(watcher.is_ok(), "{:?}", watcher.err());

    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").unwrap_or("late".to_string());
    let dir = runtime.path().join("hypr").join(signature);
    std::fs::create_dir_all(&dir).unwrap();
    let listener = UnixListener::bind(dir.join(".socket2.sock")).unwrap();
    let (mut conn, _) = listener.accept().unwrap();
    conn.write_all(b"activewindow>>CS2,Counter-Strike 2\n")
        .unwrap();
    let mut event = rx.recv_timeout(WAIT).unwrap();
    while matches!(event, FocusEvent::Unavailable(_)) {
        event = rx.recv_timeout(WAIT).unwrap();
    }
    assert_eq!(event, focus("cs2"));
}

fn answer_active_window(dir: &Path, reply: &'static str) -> std::thread::JoinHandle<Vec<u8>> {
    let listener = UnixListener::bind(dir.join(".socket.sock")).unwrap();
    std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut request = [0u8; 14];
        conn.read_exact(&mut request).unwrap();
        conn.write_all(reply.as_bytes()).unwrap();
        request.to_vec()
    })
}

#[test]
fn active_window_query_reads_the_class() {
    let dir = tempfile::tempdir().unwrap();
    let server = answer_active_window(dir.path(), r#"{"class":"Kitty","title":"x"}"#);
    assert_eq!(
        query_active_window(&dir.path().join(".socket.sock")).unwrap(),
        Some("kitty".to_string())
    );
    assert_eq!(server.join().unwrap(), b"j/activewindow");
}

#[test]
fn active_window_query_with_no_window_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let server = answer_active_window(dir.path(), "{}");
    assert_eq!(
        query_active_window(&dir.path().join(".socket.sock")).unwrap(),
        None
    );
    server.join().unwrap();
}

#[test]
fn active_window_query_with_an_empty_class_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let server = answer_active_window(dir.path(), r#"{"class":"","title":""}"#);
    assert_eq!(
        query_active_window(&dir.path().join(".socket.sock")).unwrap(),
        None
    );
    server.join().unwrap();
}

#[test]
fn active_window_query_without_a_socket_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(query_active_window(&dir.path().join(".socket.sock")).is_err());
}

#[test]
fn watcher_reports_the_focused_window_on_connect() {
    let dir = tempfile::tempdir().unwrap();
    let server = answer_active_window(dir.path(), r#"{"class":"firefox","title":"t"}"#);
    let listener = UnixListener::bind(dir.path().join(".socket2.sock")).unwrap();
    let (tx, rx) = unbounded();
    let socket = dir.path().join(".socket2.sock");
    let _watcher = watch(move || Some(socket.clone()), RETRY, tx);
    let (_conn, _) = listener.accept().unwrap();
    assert_eq!(rx.recv_timeout(WAIT), Ok(focus("firefox")));
    server.join().unwrap();
}

#[test]
fn watcher_connects_without_a_command_socket() {
    let dir = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(dir.path().join(".socket2.sock")).unwrap();
    let (tx, rx) = unbounded();
    let socket = dir.path().join(".socket2.sock");
    let _watcher = watch(move || Some(socket.clone()), RETRY, tx);
    let (mut conn, _) = listener.accept().unwrap();
    conn.write_all(b"activewindow>>one,t\n").unwrap();
    assert_eq!(rx.recv_timeout(WAIT), Ok(focus("one")));
}

#[test]
fn watcher_reports_unavailable_while_no_hyprland_socket_exists() {
    let runtime = tempfile::tempdir().unwrap();
    let (tx, rx) = unbounded();
    let _watcher = spawn_in(Some(runtime.path().to_path_buf()), RETRY, tx).unwrap();
    assert_eq!(
        rx.recv_timeout(WAIT),
        Ok(FocusEvent::Unavailable(NO_SOCKET.to_string()))
    );
    std::thread::sleep(RETRY * 10);
    assert!(rx.try_recv().is_err(), "a second outage report");

    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").unwrap_or("late".to_string());
    let dir = runtime.path().join("hypr").join(signature);
    std::fs::create_dir_all(&dir).unwrap();
    let listener = UnixListener::bind(dir.join(".socket2.sock")).unwrap();
    let (mut conn, _) = listener.accept().unwrap();
    conn.write_all(b"activewindow>>kitty,t\n").unwrap();
    assert_eq!(rx.recv_timeout(WAIT), Ok(FocusEvent::Focus(None)));
    assert_eq!(rx.recv_timeout(WAIT), Ok(focus("kitty")));
}

#[test]
fn watcher_reports_each_outage_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".socket2.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let (tx, rx) = unbounded();
    let socket = path.clone();
    let _watcher = watch(move || Some(socket.clone()), RETRY, tx);
    let (mut conn, _) = listener.accept().unwrap();
    conn.write_all(b"activewindow>>one,t\n").unwrap();
    assert_eq!(rx.recv_timeout(WAIT), Ok(focus("one")));
    drop(conn);
    drop(listener);
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(
        rx.recv_timeout(WAIT),
        Ok(FocusEvent::Unavailable(_))
    ));
    std::thread::sleep(RETRY * 10);
    assert!(rx.try_recv().is_err(), "a second outage report");
    let listener = UnixListener::bind(&path).unwrap();
    let (mut conn, _) = listener.accept().unwrap();
    conn.write_all(b"activewindow>>two,t\n").unwrap();
    assert_eq!(rx.recv_timeout(WAIT), Ok(FocusEvent::Focus(None)));
    assert_eq!(rx.recv_timeout(WAIT), Ok(focus("two")));
}

#[test]
fn watcher_reports_no_app_when_the_query_fails_after_an_outage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".socket2.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let (tx, rx) = unbounded();
    let socket = path.clone();
    let _watcher = watch(move || Some(socket.clone()), RETRY, tx);
    let (mut conn, _) = listener.accept().unwrap();
    conn.write_all(b"activewindow>>one,t\n").unwrap();
    assert_eq!(rx.recv_timeout(WAIT), Ok(focus("one")));
    drop(conn);
    drop(listener);
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(
        rx.recv_timeout(WAIT),
        Ok(FocusEvent::Unavailable(_))
    ));

    let listener = UnixListener::bind(&path).unwrap();
    let (_conn, _) = listener.accept().unwrap();
    assert_eq!(rx.recv_timeout(WAIT), Ok(FocusEvent::Focus(None)));
}
