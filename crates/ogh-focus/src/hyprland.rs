use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use crossbeam_channel::Sender;

use crate::{FocusEvent, Unavailable};

const RETRY: Duration = Duration::from_secs(2);
const SOCKET_NAME: &str = ".socket2.sock";
const COMMAND_SOCKET_NAME: &str = ".socket.sock";
const NEEDS_HYPRLAND: &str = "focus switching needs Hyprland on Linux";
const NO_SOCKET: &str = "focus switching needs Hyprland on Linux (no Hyprland socket found)";
const CANNOT_CONNECT: &str = "focus switching cannot connect to the Hyprland event socket";
const QUERY_TIMEOUT: Duration = Duration::from_secs(2);

/// `Some(app)` for an `activewindow` event (`None` when no window has the
/// focus), `None` for any other event.
pub fn parse_event(line: &str) -> Option<Option<String>> {
    let data = line.strip_prefix("activewindow>>")?;
    let class = data.split(',').next().unwrap_or_default();
    Some((!class.is_empty()).then(|| class.to_lowercase()))
}

/// The class of the focused window, lowercased, asked of the command socket
/// `command_socket`. `None` when no window has the focus.
pub fn query_active_window(command_socket: &Path) -> io::Result<Option<String>> {
    let mut stream = UnixStream::connect(command_socket)?;
    stream.set_read_timeout(Some(QUERY_TIMEOUT))?;
    stream.set_write_timeout(Some(QUERY_TIMEOUT))?;
    stream.write_all(b"j/activewindow")?;
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply)?;
    let window: serde_json::Value = serde_json::from_slice(&reply)?;
    let class = window.get("class").and_then(|class| class.as_str());
    Ok(class
        .filter(|class| !class.is_empty())
        .map(|class| class.to_lowercase()))
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

/// Like `socket_path`, but a signature whose socket is gone (a stale
/// environment after a Hyprland restart) falls back to the newest instance.
pub fn resolve(runtime_dir: &Path, signature: Option<&str>) -> Option<PathBuf> {
    signature
        .and_then(|signature| socket_path(runtime_dir, Some(signature)))
        .or_else(|| socket_path(runtime_dir, None))
}

pub(crate) fn spawn(events: Sender<FocusEvent>) -> Result<JoinHandle<()>, Unavailable> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    spawn_in(runtime_dir, RETRY, events)
}

/// Watches the Hyprland instance under `runtime_dir`, the user's
/// `XDG_RUNTIME_DIR`. Without one there is no Hyprland to find; with one, a
/// socket that does not exist yet is waited for, since Hyprland may start
/// after the daemon.
pub fn spawn_in(
    runtime_dir: Option<PathBuf>,
    retry: Duration,
    events: Sender<FocusEvent>,
) -> Result<JoinHandle<()>, Unavailable> {
    let runtime_dir = runtime_dir.ok_or_else(|| Unavailable(NEEDS_HYPRLAND.to_string()))?;
    let resolve = move || {
        let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok();
        resolve(&runtime_dir, signature.as_deref())
    };
    Ok(watch(resolve, retry, events))
}

/// Reads focus events from the socket `socket` resolves to, reconnecting after
/// `retry` whenever it closes. Each outage is reported once as `Unavailable`;
/// after a connect the focused window is reported before any event. Ends when
/// `events` has no receiver left.
pub fn watch(
    socket: impl Fn() -> Option<PathBuf> + Send + 'static,
    retry: Duration,
    events: Sender<FocusEvent>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut reported = false;
        loop {
            match connect(&socket) {
                Ok((stream, path)) => {
                    let after_outage = std::mem::replace(&mut reported, false);
                    if !report_active_window(&path, after_outage, &events)
                        || !forward_events(stream, &events)
                    {
                        return;
                    }
                }
                Err(reason) if !reported => {
                    reported = true;
                    if events
                        .send(FocusEvent::Unavailable(reason.to_string()))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(_) => {}
            }
            thread::sleep(retry);
        }
    })
}

fn connect(socket: &impl Fn() -> Option<PathBuf>) -> Result<(UnixStream, PathBuf), &'static str> {
    let path = socket().ok_or(NO_SOCKET)?;
    let stream = UnixStream::connect(&path).map_err(|_| CANNOT_CONNECT)?;
    Ok((stream, path))
}

/// Returns false once the receiver is gone. A failed query reports no app after an outage,
/// so focus stops reading as unavailable; otherwise the next focus event still arrives.
fn report_active_window(
    event_socket: &Path,
    after_outage: bool,
    events: &Sender<FocusEvent>,
) -> bool {
    let command_socket = event_socket.with_file_name(COMMAND_SOCKET_NAME);
    match query_active_window(&command_socket) {
        Ok(app) => events.send(FocusEvent::Focus(app)).is_ok(),
        Err(e) => {
            tracing::debug!("cannot query the active window: {e}");
            !after_outage || events.send(FocusEvent::Focus(None)).is_ok()
        }
    }
}

/// Returns false once the receiver is gone, true when the stream ended.
fn forward_events(stream: UnixStream, events: &Sender<FocusEvent>) -> bool {
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
            && events.send(FocusEvent::Focus(app)).is_err()
        {
            return false;
        }
    }
}
