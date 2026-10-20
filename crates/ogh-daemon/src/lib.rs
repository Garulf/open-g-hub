pub mod autoswitch;
pub mod config;
pub mod controller;
pub mod convert;
pub mod device_worker;
mod fs_util;
pub mod ipc_server;
pub mod logging;
pub mod paths;
pub mod snapshot;

use std::path::Path;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use ogh_device::{Session, guard};
use ogh_focus::FocusEvent;
use ogh_ipc::{Endpoint, Server};

use crate::autoswitch::Command;
use crate::config::Store;
use crate::controller::{Controller, Input};
use crate::device_worker::{DeviceCommand, DeviceUpdate, WorkerConfig};
use crate::paths::Paths;

const WORKER_STOP_WAIT: Duration = Duration::from_secs(2);

/// Runs until Ctrl-C or SIGTERM, then exits the process with status 0.
pub fn run(paths: Paths) -> Result<(), String> {
    let endpoint = Endpoint::for_current_user().map_err(|e| e.to_string())?;
    let (stop_tx, stop) = bounded(1);
    ctrlc::set_handler(move || {
        let _ = stop_tx.try_send(());
    })
    .map_err(|e| format!("cannot install the signal handler: {e}"))?;
    let server = Server::bind(&endpoint).map_err(|e| e.to_string())?;

    let store = Store::open(&paths.config_file);
    let (commands, command_rx) = unbounded();
    let (updates, update_rx) = unbounded();
    let (inputs, input_rx) = unbounded();
    let worker = device_worker::spawn(
        Session::open,
        guard::ghub_agent_running,
        WorkerConfig::default(),
        command_rx,
        updates,
    );
    bridge_device_updates(update_rx, inputs.clone());
    let audio_commands = start_auto_switch(&store, &paths, inputs.clone());
    let controller =
        Controller::new(store, commands.clone(), Instant::now).with_auto_switch(audio_commands);
    thread::spawn(move || controller.run(input_rx));
    ipc_server::serve(server, inputs.clone());
    start_focus_watcher(inputs.clone());
    tracing::info!(
        "ogh-daemon {} listening on {endpoint:?}",
        env!("CARGO_PKG_VERSION")
    );

    let _ = stop.recv();
    shut_down(&commands, worker, &endpoint)
}

fn bridge_device_updates(updates: Receiver<DeviceUpdate>, inputs: Sender<Input>) {
    thread::spawn(move || {
        for update in updates {
            if inputs.send(Input::Device(update)).is_err() {
                return;
            }
        }
    });
}

fn start_auto_switch(store: &Store, paths: &Paths, inputs: Sender<Input>) -> Sender<Command> {
    let config = store
        .config()
        .map(|config| config.audio_switch.clone())
        .unwrap_or_default();
    let (outcome_tx, outcome_rx) = unbounded();
    let commands = autoswitch::spawn(
        ogh_audio::system_router,
        config,
        paths.state_file.clone(),
        outcome_tx,
    );
    thread::spawn(move || {
        for outcome in outcome_rx {
            if let Some(error) = &outcome.error {
                tracing::warn!("audio auto-switch: {error}");
            }
            if inputs.send(Input::AutoSwitch(outcome)).is_err() {
                return;
            }
        }
    });
    commands
}

fn start_focus_watcher(inputs: Sender<Input>) {
    let (focus_tx, focus_rx) = unbounded();
    match ogh_focus::spawn(focus_tx) {
        Ok(_) => {
            thread::spawn(move || {
                for event in focus_rx {
                    let input = match event {
                        FocusEvent::Focus(app) => Input::Focus(app),
                        FocusEvent::Unavailable(reason) => Input::FocusUnavailable(reason),
                    };
                    if inputs.send(input).is_err() {
                        return;
                    }
                }
            });
        }
        Err(reason) => {
            tracing::info!("focus switching is unavailable: {reason}");
            let _ = inputs.send(Input::FocusUnavailable(reason.to_string()));
        }
    }
}

fn shut_down(commands: &Sender<DeviceCommand>, worker: JoinHandle<()>, endpoint: &Endpoint) -> ! {
    tracing::info!("shutting down");
    let _ = commands.send(DeviceCommand::Shutdown);
    let (stopped_tx, stopped) = bounded(1);
    thread::spawn(move || {
        let _ = worker.join();
        let _ = stopped_tx.send(());
    });
    if stopped.recv_timeout(WORKER_STOP_WAIT).is_err() {
        tracing::warn!("the device worker did not stop within {WORKER_STOP_WAIT:?}");
    }
    remove_socket(endpoint);
    std::process::exit(0)
}

fn remove_socket(endpoint: &Endpoint) {
    if let Endpoint::Path(path) = endpoint {
        remove_file_if_present(path);
    }
}

fn remove_file_if_present(path: &Path) {
    if let Err(e) = std::fs::remove_file(path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("cannot remove {}: {e}", path.display());
    }
}
