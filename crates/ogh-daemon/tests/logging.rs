use std::fs;

use ogh_daemon::logging::{LOG_ROTATE_BYTES, rotate_if_large};
use tempfile::TempDir;

#[test]
fn large_log_is_rotated_on_start() {
    let dir = TempDir::new().unwrap();
    let log = dir.path().join("daemon.log");
    let rotated = dir.path().join("daemon.log.1");
    fs::write(&rotated, "older").unwrap();
    fs::write(&log, vec![b'x'; 6 * 1024 * 1024]).unwrap();

    rotate_if_large(&log, LOG_ROTATE_BYTES).unwrap();

    assert!(!log.exists());
    assert_eq!(fs::metadata(&rotated).unwrap().len(), 6 * 1024 * 1024);
}

#[test]
fn small_log_is_kept() {
    let dir = TempDir::new().unwrap();
    let log = dir.path().join("daemon.log");
    fs::write(&log, vec![b'x'; 1024]).unwrap();

    rotate_if_large(&log, LOG_ROTATE_BYTES).unwrap();

    assert_eq!(fs::metadata(&log).unwrap().len(), 1024);
    assert!(!dir.path().join("daemon.log.1").exists());
}

#[test]
fn missing_log_is_not_an_error() {
    let dir = TempDir::new().unwrap();
    rotate_if_large(&dir.path().join("daemon.log"), LOG_ROTATE_BYTES).unwrap();
}
