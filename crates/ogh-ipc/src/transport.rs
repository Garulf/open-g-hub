use std::ffi::OsString;
use std::fmt;
use std::io::{self, BufReader};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use interprocess::local_socket::traits::{Listener as _, Stream as _};
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, Listener, ListenerOptions, Name, RecvHalf, SendHalf,
    Stream, ToFsName, ToNsName,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::codec::{CodecError, read_line, write_line};
use crate::types::{ErrorKind, IPC_VERSION, Notification, Request, Response};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    Path(PathBuf),
    PipeName(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    XdgRuntimeDirUnset,
    AlreadyRunning,
    DaemonNotRunning,
    VersionMismatch { daemon: u32 },
    Io(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::XdgRuntimeDirUnset => write!(f, "XDG_RUNTIME_DIR is not set"),
            TransportError::AlreadyRunning => write!(f, "ogh-daemon is already running"),
            TransportError::DaemonNotRunning => write!(f, "ogh-daemon is not running"),
            TransportError::VersionMismatch { daemon } => write!(
                f,
                "ogh-daemon speaks IPC version {daemon}, this client speaks {IPC_VERSION}"
            ),
            TransportError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<io::Error> for TransportError {
    fn from(e: io::Error) -> Self {
        TransportError::Io(e.to_string())
    }
}

impl From<CodecError> for TransportError {
    fn from(e: CodecError) -> Self {
        TransportError::Io(e.to_string())
    }
}

impl Endpoint {
    pub fn from_runtime_dir(runtime_dir: Option<OsString>) -> Result<Endpoint, TransportError> {
        match runtime_dir {
            Some(dir) if !dir.is_empty() => Ok(Endpoint::Path(
                PathBuf::from(dir).join("open-g-hub").join("daemon.sock"),
            )),
            _ => Err(TransportError::XdgRuntimeDirUnset),
        }
    }

    #[cfg(unix)]
    pub fn for_current_user() -> Result<Endpoint, TransportError> {
        Endpoint::from_runtime_dir(std::env::var_os("XDG_RUNTIME_DIR"))
    }

    #[cfg(windows)]
    pub fn for_current_user() -> Result<Endpoint, TransportError> {
        let user = std::env::var("USERNAME")
            .ok()
            .filter(|user| !user.is_empty())
            .ok_or_else(|| TransportError::Io("USERNAME is not set".to_string()))?;
        Ok(Endpoint::PipeName(format!("open-g-hub-{user}")))
    }

    fn name(&self) -> io::Result<Name<'_>> {
        match self {
            Endpoint::Path(path) => path.as_path().to_fs_name::<GenericFilePath>(),
            Endpoint::PipeName(name) => name.as_str().to_ns_name::<GenericNamespaced>(),
        }
    }
}

pub struct Server {
    listener: Listener,
}

impl Server {
    pub fn bind(endpoint: &Endpoint) -> Result<Server, TransportError> {
        platform::prepare(endpoint)?;
        let listener = ListenerOptions::new()
            .name(endpoint.name()?)
            .create_sync()
            .map_err(platform::bind_error)?;
        platform::secure(endpoint)?;
        Ok(Server { listener })
    }

    pub fn accept(&self) -> Result<Connection, TransportError> {
        Ok(Connection::new(self.listener.accept()?))
    }
}

pub struct Connection {
    reader: BufReader<RecvHalf>,
    writer: Arc<Mutex<SendHalf>>,
}

impl Connection {
    fn new(stream: Stream) -> Connection {
        let (recv, send) = stream.split();
        Connection {
            reader: BufReader::new(recv),
            writer: Arc::new(Mutex::new(send)),
        }
    }

    pub fn send<T: Serialize>(&mut self, value: &T) -> io::Result<()> {
        write_shared(&self.writer, value)
    }

    /// Reads one line. `Ok(None)` means the peer closed the connection. After any `CodecError`
    /// the stream may be desynchronised: drop the connection instead of calling `recv` again.
    pub fn recv<T: DeserializeOwned>(&mut self) -> Result<Option<T>, CodecError> {
        read_line(&mut self.reader)
    }

    /// The writer shares the connection's send half and serialises whole lines across clones.
    pub fn try_clone_writer(&self) -> Result<ConnectionWriter, TransportError> {
        Ok(ConnectionWriter {
            writer: Arc::clone(&self.writer),
        })
    }
}

#[derive(Clone)]
pub struct ConnectionWriter {
    writer: Arc<Mutex<SendHalf>>,
}

impl ConnectionWriter {
    pub fn send<T: Serialize>(&mut self, value: &T) -> io::Result<()> {
        write_shared(&self.writer, value)
    }
}

fn write_shared<T: Serialize>(writer: &Mutex<SendHalf>, value: &T) -> io::Result<()> {
    let mut half = writer.lock().unwrap_or_else(PoisonError::into_inner);
    write_line(&mut *half, value)
}

pub struct Client {
    connection: Connection,
}

impl Client {
    pub fn connect(endpoint: &Endpoint) -> Result<Client, TransportError> {
        let stream = Stream::connect(endpoint.name()?).map_err(connect_error)?;
        let mut client = Client {
            connection: Connection::new(stream),
        };
        client.handshake()?;
        Ok(client)
    }

    fn handshake(&mut self) -> Result<(), TransportError> {
        let reply = self.call(&Request::Hello {
            version: IPC_VERSION,
        })?;
        match reply {
            Response::Hello(hello) if hello.version == IPC_VERSION => Ok(()),
            Response::Hello(hello) => Err(TransportError::VersionMismatch {
                daemon: hello.version,
            }),
            Response::Error(e) if e.kind == ErrorKind::VersionMismatch => {
                Err(TransportError::VersionMismatch { daemon: 0 })
            }
            Response::Error(e) => Err(TransportError::Io(e.message)),
            _ => Err(TransportError::Io("unexpected reply to hello".to_string())),
        }
    }

    pub fn call(&mut self, request: &Request) -> Result<Response, TransportError> {
        self.connection.send(request)?;
        self.connection
            .recv()?
            .ok_or_else(|| TransportError::Io("ogh-daemon closed the connection".to_string()))
    }

    pub fn subscribe(mut self) -> Result<Subscription, TransportError> {
        match self.call(&Request::Subscribe)? {
            Response::Done => Ok(Subscription {
                connection: self.connection,
            }),
            Response::Error(e) => Err(TransportError::Io(e.message)),
            _ => Err(TransportError::Io(
                "unexpected reply to subscribe".to_string(),
            )),
        }
    }
}

pub struct Subscription {
    connection: Connection,
}

impl Subscription {
    /// Blocks for the next notification; `Ok(None)` means the daemon closed the connection.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<Notification>, TransportError> {
        Ok(self.connection.recv()?)
    }
}

fn connect_error(e: io::Error) -> TransportError {
    match e.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => {
            TransportError::DaemonNotRunning
        }
        _ => TransportError::from(e),
    }
}

#[cfg(unix)]
mod platform {
    use std::fs::{self, DirBuilder, Permissions};
    use std::io;
    use std::os::unix::fs::{DirBuilderExt, FileTypeExt, PermissionsExt};
    use std::path::Path;

    use interprocess::local_socket::Stream;
    use interprocess::local_socket::traits::Stream as _;

    use super::{Endpoint, TransportError, connect_error};

    pub(super) fn prepare(endpoint: &Endpoint) -> Result<(), TransportError> {
        let Endpoint::Path(path) = endpoint else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            private_dir(dir)?;
        }
        clear_stale_socket(endpoint, path)
    }

    pub(super) fn secure(endpoint: &Endpoint) -> Result<(), TransportError> {
        if let Endpoint::Path(path) = endpoint {
            fs::set_permissions(path, Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    pub(super) fn bind_error(e: io::Error) -> TransportError {
        match e.kind() {
            io::ErrorKind::AddrInUse => TransportError::AlreadyRunning,
            _ => TransportError::from(e),
        }
    }

    fn private_dir(dir: &Path) -> io::Result<()> {
        match fs::metadata(dir) {
            Ok(meta) if meta.permissions().mode() & 0o077 != 0 => {
                fs::set_permissions(dir, Permissions::from_mode(0o700))
            }
            Ok(_) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                DirBuilder::new().recursive(true).mode(0o700).create(dir)
            }
            Err(e) => Err(e),
        }
    }

    fn clear_stale_socket(endpoint: &Endpoint, path: &Path) -> Result<(), TransportError> {
        let meta = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        if !meta.file_type().is_socket() {
            return Err(TransportError::Io(format!(
                "{} exists and is not a socket",
                path.display()
            )));
        }
        match Stream::connect(endpoint.name()?).map_err(connect_error) {
            Ok(_) => Err(TransportError::AlreadyRunning),
            Err(TransportError::DaemonNotRunning) => Ok(fs::remove_file(path)?),
            Err(e) => Err(e),
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::io;

    use super::{Endpoint, TransportError};

    pub(super) fn prepare(_endpoint: &Endpoint) -> Result<(), TransportError> {
        Ok(())
    }

    pub(super) fn secure(_endpoint: &Endpoint) -> Result<(), TransportError> {
        Ok(())
    }

    /// The first pipe instance is created exclusively, so a live server makes the second
    /// create fail with access denied.
    pub(super) fn bind_error(e: io::Error) -> TransportError {
        match e.kind() {
            io::ErrorKind::PermissionDenied | io::ErrorKind::AddrInUse => {
                TransportError::AlreadyRunning
            }
            _ => TransportError::from(e),
        }
    }
}
