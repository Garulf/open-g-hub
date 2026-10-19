use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use crossbeam_channel::Sender;

use crate::Unavailable;

const RETRY: Duration = Duration::from_secs(2);
const SOCKET_NAME: &str = ".socket2.sock";
const NEEDS_HYPRLAND: &str = "focus switching needs Hyprland on Linux";

/// `Some(app)` for an `activewindow` event (`None` when no window has the
/// focus), `None` for any other event.
pub fn parse_event(line: &str) -> Option<Option<String>> {
    let data = line.strip_prefix("activewindow>>")?;
    let class = data.split(',').next().unwrap_or_default();
    Some((!class.is_empty()).then(|| class.to_lowercase()))
}

/// The event socket of a Hyprland instance. Without a signature, the most
/// recently modified instance wins.
pub fn socket_path(runtime_dir: &Path, signature: Option<&str>) -> Option<PathBuf> {
    let instances = runtime_dir.join("hypr");
    match signature {
        Some(signature) => {
            let path = instances.join(signature).join(SOCKET_NAME);
            path.exists().then_some(path)
        }
        None => std::fs::read_dir(instances)
            .ok()?
            .filter_map(|entry| {
                let path = entry.ok()?.path().join(SOCKET_NAME);
                let modified = path.metadata().ok()?.modified().ok()?;
                Some((modified, path))
            })
            .max_by_key(|(modified, _): &(SystemTime, PathBuf)| *modified)
            .map(|(_, path)| path),
    }
}

pub(crate) fn spawn(on_focus: Sender<Option<String>>) -> Result<JoinHandle<()>, Unavailable> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| Unavailable(NEEDS_HYPRLAND.to_string()))?;
    let resolve = move || {
        let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok();
        socket_path(&runtime_dir, signature.as_deref())
    };
    if resolve().is_none() {
        return Err(Unavailable(NEEDS_HYPRLAND.to_string()));
    }
    Ok(watch(resolve, RETRY, on_focus))
}

/// Reads focus events from the socket `socket` resolves to, reconnecting after
/// `retry` whenever it closes. Ends when `on_focus` has no receiver left.
pub fn watch(
    socket: impl Fn() -> Option<PathBuf> + Send + 'static,
    retry: Duration,
    on_focus: Sender<Option<String>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        loop {
            if let Some(stream) = socket().and_then(|path| UnixStream::connect(path).ok())
                && !forward_events(stream, &on_focus)
            {
                return;
            }
            thread::sleep(retry);
        }
    })
}

/// Returns false once the receiver is gone, true when the stream ended.
fn forward_events(stream: UnixStream, on_focus: &Sender<Option<String>>) -> bool {
    let mut reader = BufReader::new(stream);
    let mut raw = Vec::new();
    loop {
        raw.clear();
        match reader.read_until(b'\n', &mut raw) {
            Ok(0) | Err(_) => return true,
            Ok(_) => {}
        }
        let Ok(line) = std::str::from_utf8(&raw) else {
            continue;
        };
        if let Some(app) = parse_event(line.trim_end_matches(['\r', '\n']))
            && on_focus.send(app).is_err()
        {
            return false;
        }
    }
}
