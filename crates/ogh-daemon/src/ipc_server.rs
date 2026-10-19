use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, bounded, select, unbounded};
use ogh_ipc::transport::{Connection, ConnectionWriter};
use ogh_ipc::{
    CodecError, ErrorKind, Hello, IPC_VERSION, IpcError, Notification, Request, Response, Server,
};

use crate::controller::Input;

const ACCEPT_RETRY: Duration = Duration::from_millis(100);

/// Runs the accept loop on its own thread, with one more thread per connection.
pub fn serve(server: Server, inputs: Sender<Input>) -> JoinHandle<()> {
    thread::spawn(move || {
        loop {
            match server.accept() {
                Ok(connection) => {
                    let inputs = inputs.clone();
                    thread::spawn(move || handle_connection(connection, inputs));
                }
                Err(error) => {
                    tracing::warn!("cannot accept a connection: {error}");
                    thread::sleep(ACCEPT_RETRY);
                }
            }
        }
    })
}

enum Next {
    Request(Request),
    /// A whole line was consumed but did not parse; the stream is still in sync.
    Skip,
    Close,
}

fn handle_connection(mut connection: Connection, inputs: Sender<Input>) {
    if !handshake(&mut connection) {
        return;
    }
    loop {
        let request = match next_request(&mut connection) {
            Next::Request(request) => request,
            Next::Skip => continue,
            Next::Close => return,
        };
        let sent = match request {
            Request::Hello { version } => answer_hello(&mut connection, version),
            Request::Subscribe => return subscribe(connection, &inputs),
            request => forward(&mut connection, &inputs, request),
        };
        if !sent {
            return;
        }
    }
}

/// `true` once the client sent a `Hello` with the version this daemon speaks.
fn handshake(connection: &mut Connection) -> bool {
    loop {
        match next_request(connection) {
            Next::Request(Request::Hello { version }) => {
                return answer_hello(connection, version);
            }
            Next::Request(_) => {
                let _ = connection.send(&error(
                    ErrorKind::BadRequest,
                    "the first message must be hello",
                ));
                return false;
            }
            Next::Skip => {}
            Next::Close => return false,
        }
    }
}

fn answer_hello(connection: &mut Connection, version: u32) -> bool {
    if version != IPC_VERSION {
        let message = format!("ogh-daemon speaks IPC version {IPC_VERSION}, not {version}");
        let _ = connection.send(&error(ErrorKind::VersionMismatch, &message));
        return false;
    }
    connection
        .send(&Response::Hello(Hello {
            version: IPC_VERSION,
            daemon_version: env!("CARGO_PKG_VERSION").to_string(),
        }))
        .is_ok()
}

fn next_request(connection: &mut Connection) -> Next {
    match connection.recv::<Request>() {
        Ok(Some(request)) => Next::Request(request),
        Ok(None) => Next::Close,
        Err(CodecError::Json(message)) => {
            let reply = error(ErrorKind::BadRequest, &message);
            if connection.send(&reply).is_ok() {
                Next::Skip
            } else {
                Next::Close
            }
        }
        Err(CodecError::TooLong) => {
            tracing::warn!("closing a connection that sent an oversized message");
            let _ = connection.send(&error(ErrorKind::BadRequest, "message too long"));
            Next::Close
        }
        Err(CodecError::Io(e)) => {
            tracing::warn!("cannot read from a connection: {e}");
            Next::Close
        }
    }
}

/// `false` when the connection must end.
fn forward(connection: &mut Connection, inputs: &Sender<Input>, request: Request) -> bool {
    let (reply, response) = bounded(1);
    let reply = match inputs.send(Input::Ipc { request, reply }) {
        Ok(()) => response.recv().ok(),
        Err(_) => None,
    };
    match reply {
        Some(reply) => connection.send(&reply).is_ok(),
        None => {
            let _ = connection.send(&shutting_down());
            false
        }
    }
}

/// After the reply the connection only carries notifications. Dropping `closed` when the
/// client goes away stops the forwarder, which drops the receiver and gets the controller to
/// prune this subscriber on its next send.
fn subscribe(mut connection: Connection, inputs: &Sender<Input>) {
    let (notify, notifications) = unbounded();
    if inputs.send(Input::Subscribe(notify)).is_err() {
        let _ = connection.send(&shutting_down());
        return;
    }
    let Ok(writer) = connection.try_clone_writer() else {
        return;
    };
    if connection.send(&Response::Done).is_err() {
        return;
    }
    let (closed_tx, closed) = bounded::<()>(0);
    thread::spawn(move || forward_notifications(writer, notifications, closed));
    while !matches!(next_ignoring_requests(&mut connection), Next::Close) {}
    drop(closed_tx);
}

fn next_ignoring_requests(connection: &mut Connection) -> Next {
    match connection.recv::<Request>() {
        Ok(Some(_)) | Err(CodecError::Json(_)) => Next::Skip,
        Ok(None) | Err(CodecError::TooLong) => Next::Close,
        Err(CodecError::Io(e)) => {
            tracing::warn!("cannot read from a subscribed connection: {e}");
            Next::Close
        }
    }
}

fn forward_notifications(
    mut writer: ConnectionWriter,
    notifications: Receiver<Notification>,
    closed: Receiver<()>,
) {
    loop {
        select! {
            recv(notifications) -> notification => match notification {
                Ok(notification) if writer.send(&notification).is_ok() => {}
                _ => return,
            },
            recv(closed) -> _ => return,
        }
    }
}

fn error(kind: ErrorKind, message: &str) -> Response {
    Response::Error(IpcError {
        kind,
        message: message.to_string(),
    })
}

fn shutting_down() -> Response {
    error(ErrorKind::Device, "ogh-daemon is shutting down")
}
