use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use ogh_device::{DeviceError, FeatureInfo};
use ogh_ipc::{
    AutoSwitchStatus, Connection, ErrorKind, Feature, FocusStatus, Headset, Hello, IPC_VERSION,
    IpcError, Notification, ProfileInfo, Request, Response, Setting, Status,
};

use crate::autoswitch::{AudioSwitchOutcome, Command};
use crate::config::{AudioSwitchConfig, Store};
use crate::convert;
use crate::device_worker::{DeviceCommand, DeviceUpdate};
use crate::snapshot::{DeviceSnapshot, DeviceWrite};
use default_capture::DefaultCapture;
use deferred::DeferredSave;

mod default_capture;
mod deferred;

/// How long an app must keep focus before its profile applies.
pub const FOCUS_SETTLE: Duration = Duration::from_millis(250);

#[allow(clippy::large_enum_variant)]
pub enum Input {
    Ipc {
        request: Request,
        reply: Sender<Response>,
    },
    Subscribe(Sender<Notification>),
    Device(DeviceUpdate),
    Focus(Option<String>),
    FocusUnavailable(String),
    AutoSwitch(AudioSwitchOutcome),
}

struct PendingWrite {
    reply: Option<Sender<Response>>,
    writes: Vec<DeviceWrite>,
}

struct PendingFocus {
    app: Option<String>,
    deadline: Instant,
}

/// A profile chosen by hand. It holds while the settled app stays the one
/// that had focus when it was chosen.
struct ManualProfile {
    name: String,
    chosen_over: Option<String>,
}

pub struct Controller {
    store: Store,
    devices: Sender<DeviceCommand>,
    clock: Box<dyn Fn() -> Instant + Send>,
    connection: Connection,
    features: Vec<FeatureInfo>,
    snapshot: DeviceSnapshot,
    default_capture: DefaultCapture,
    /// An automatic application was skipped while writes were in flight.
    apply_wanted: bool,
    next_write_id: u64,
    pending: HashMap<u64, PendingWrite>,
    /// Saves wait for writes in flight so they hold what those writes set.
    deferred_saves: Vec<DeferredSave>,
    focus_available: bool,
    focused_app: Option<String>,
    pending_focus: Option<PendingFocus>,
    manual: Option<ManualProfile>,
    active: Option<String>,
    subscribers: Vec<Sender<Notification>>,
    last_status: Option<Status>,
    auto_switch: Option<Sender<Command>>,
    announced_headset: Headset,
    audio: AudioSwitchOutcome,
}

impl Controller {
    pub fn new(
        store: Store,
        devices: Sender<DeviceCommand>,
        clock: impl Fn() -> Instant + Send + 'static,
    ) -> Controller {
        let mut controller = Controller {
            store,
            devices,
            clock: Box::new(clock),
            connection: Connection::NotFound,
            features: Vec::new(),
            snapshot: DeviceSnapshot::default(),
            default_capture: DefaultCapture::Unchecked,
            apply_wanted: false,
            next_write_id: 1,
            pending: HashMap::new(),
            deferred_saves: Vec::new(),
            focus_available: true,
            focused_app: None,
            pending_focus: None,
            manual: None,
            active: None,
            subscribers: Vec::new(),
            last_status: None,
            auto_switch: None,
            announced_headset: Headset::Unknown,
            audio: AudioSwitchOutcome::default(),
        };
        controller.active = controller.resolve_active();
        controller.last_status = Some(controller.status());
        controller
    }

    /// Feeds headset changes and the audio settings to the auto-switch thread.
    pub fn with_auto_switch(mut self, commands: Sender<Command>) -> Controller {
        self.auto_switch = Some(commands);
        self.configure_auto_switch();
        self
    }

    pub fn status(&self) -> Status {
        Status {
            connection: self.connection.clone(),
            headset: self.snapshot.headset(),
            battery: convert::battery(&self.snapshot),
            firmware: self.snapshot.firmware.clone(),
            settings: convert::device_settings(&self.snapshot),
            active_profile: self.active.clone(),
            focus: FocusStatus {
                available: self.focus_available,
                app: self.focused_app.clone(),
            },
            auto_switch: AutoSwitchStatus {
                enabled: self
                    .store
                    .config()
                    .map(|config| config.audio_switch.enabled)
                    .unwrap_or(false),
                active: self.audio.active,
                error: self.audio.error.clone(),
            },
            config_error: self.store.config_error(),
        }
    }

    /// Returns when `inputs` disconnects.
    pub fn run(mut self, inputs: Receiver<Input>) {
        loop {
            let received = match self.focus_wait() {
                Some(wait) => inputs.recv_timeout(wait),
                None => inputs.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match received {
                Ok(input) => self.handle(input),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            self.settle_due_focus();
            self.announce_headset();
            self.notify();
        }
    }

    /// The remaining settle time by the controller's clock, capped so a
    /// clock that moves without a wakeup is still checked within one settle
    /// period of real time.
    fn focus_wait(&self) -> Option<Duration> {
        let pending = self.pending_focus.as_ref()?;
        let remaining = pending.deadline.saturating_duration_since((self.clock)());
        Some(remaining.min(FOCUS_SETTLE))
    }

    fn handle(&mut self, input: Input) {
        match input {
            Input::Ipc { request, reply } => self.handle_request(request, reply),
            Input::Subscribe(subscriber) => self.subscribe(subscriber),
            Input::Device(update) => self.handle_device(update),
            Input::Focus(app) => {
                self.focus_available = true;
                self.pending_focus = Some(PendingFocus {
                    app,
                    deadline: (self.clock)() + FOCUS_SETTLE,
                });
            }
            Input::AutoSwitch(outcome) => self.audio = outcome,
            Input::FocusUnavailable(_) => {
                self.focus_available = false;
                self.pending_focus = None;
                self.settle_focus(None);
            }
        }
    }

    fn announce_headset(&mut self) {
        let headset = self.snapshot.headset();
        if headset != self.announced_headset {
            self.announced_headset = headset;
            self.send_to_auto_switch(Command::Headset(headset));
        }
    }

    fn configure_auto_switch(&self) {
        let config = self
            .store
            .config()
            .map(|config| config.audio_switch.clone())
            .unwrap_or_else(|_| AudioSwitchConfig::default());
        self.send_to_auto_switch(Command::Configure(config));
    }

    fn send_to_auto_switch(&self, command: Command) {
        if let Some(commands) = &self.auto_switch {
            let _ = commands.send(command);
        }
    }

    fn subscribe(&mut self, subscriber: Sender<Notification>) {
        if subscriber.send(Notification::Status(self.status())).is_ok() {
            self.subscribers.push(subscriber);
        }
    }

    fn notify(&mut self) {
        let status = self.status();
        if self.last_status.as_ref() == Some(&status) {
            return;
        }
        self.subscribers.retain(|subscriber| {
            subscriber
                .send(Notification::Status(status.clone()))
                .is_ok()
        });
        self.last_status = Some(status);
    }

    fn is_connected(&self) -> bool {
        self.connection == Connection::Connected
    }

    fn handle_device(&mut self, update: DeviceUpdate) {
        match update {
            DeviceUpdate::Connected { features, snapshot } => {
                self.connection = Connection::Connected;
                self.features = features;
                self.snapshot = snapshot;
                self.create_default_profile();
                self.active = self.resolve_active();
                self.apply_wanted = false;
                self.apply_active();
            }
            DeviceUpdate::Event(event) if self.is_connected() => {
                self.snapshot.apply_event(&event);
            }
            DeviceUpdate::Refreshed(snapshot) if self.is_connected() => {
                self.snapshot = snapshot;
                self.create_default_profile();
                self.apply_active();
            }
            DeviceUpdate::WriteDone { id, result } => self.finish_write(id, result),
            DeviceUpdate::Closed(connection) => self.close(connection),
            DeviceUpdate::Event(_) | DeviceUpdate::Refreshed(_) => {}
        }
    }

    fn close(&mut self, connection: Connection) {
        self.connection = connection;
        self.features.clear();
        self.snapshot = DeviceSnapshot::default();
        self.apply_wanted = false;
        let error = device_error(&DeviceError::Disconnected);
        for (_, pending) in self.pending.drain() {
            if let Some(reply) = pending.reply {
                let _ = reply.send(Response::Error(error.clone()));
            }
        }
        self.run_deferred_saves();
    }

    fn send_writes(&mut self, writes: Vec<DeviceWrite>, reply: Option<Sender<Response>>) {
        let id = self.next_write_id;
        self.next_write_id += 1;
        let command = DeviceCommand::Write {
            id,
            writes: writes.clone(),
        };
        if self.devices.send(command).is_err() {
            if let Some(reply) = reply {
                let _ = reply.send(Response::Error(IpcError {
                    kind: ErrorKind::NotConnected,
                    message: "device worker stopped".to_string(),
                }));
            }
            return;
        }
        self.pending.insert(id, PendingWrite { reply, writes });
    }

    fn closed_error(&self) -> IpcError {
        let (kind, error) = match &self.connection {
            Connection::GhubRunning => (ErrorKind::GhubRunning, DeviceError::GHubRunning),
            Connection::AccessDenied { message } => (
                ErrorKind::NotConnected,
                DeviceError::AccessDenied(message.clone()),
            ),
            Connection::Error { message } => {
                (ErrorKind::NotConnected, DeviceError::Io(message.clone()))
            }
            _ => (ErrorKind::NotConnected, DeviceError::NotFound),
        };
        IpcError {
            kind,
            message: error.to_string(),
        }
    }

    fn not_connected(&self) -> IpcError {
        IpcError {
            kind: ErrorKind::NotConnected,
            ..self.closed_error()
        }
    }

    fn settle_due_focus(&mut self) {
        let now = (self.clock)();
        if let Some(focus) = self.pending_focus.take_if(|focus| now >= focus.deadline) {
            self.settle_focus(focus.app);
        }
    }

    fn settle_focus(&mut self, app: Option<String>) {
        if app == self.focused_app {
            return;
        }
        if self
            .manual
            .as_ref()
            .is_some_and(|manual| manual.chosen_over != app)
        {
            self.manual = None;
        }
        self.focused_app = app;
        self.refresh_active();
    }

    fn resolve_active(&self) -> Option<String> {
        let config = self.store.config().ok()?;
        if let Some(manual) = &self.manual
            && config.profiles.contains_key(&manual.name)
        {
            return Some(manual.name.clone());
        }
        let linked = self
            .focused_app
            .as_deref()
            .and_then(|app| self.store.profile_for_app(app));
        Some(linked.unwrap_or(&config.default_profile).to_string())
    }

    /// Recomputes the active profile after a store or focus change and
    /// applies it when its name changed.
    fn refresh_active(&mut self) {
        let stale = self.manual.as_ref().is_some_and(|manual| {
            self.store
                .config()
                .map_or(true, |config| !config.profiles.contains_key(&manual.name))
        });
        if stale {
            self.manual = None;
        }
        let next = self.resolve_active();
        if next != self.active {
            self.active = next;
            self.apply_active();
        }
    }

    fn profile_writes(&self, name: &str) -> Result<Vec<DeviceWrite>, IpcError> {
        let profile = self
            .store
            .config()?
            .profiles
            .get(name)
            .ok_or_else(|| no_such_profile(name))?;
        convert::diff(&self.snapshot, &profile.settings)
    }

    fn apply_active(&mut self) {
        if !self.is_connected() {
            return;
        }
        if !self.pending.is_empty() {
            self.apply_wanted = true;
            return;
        }
        let Some(name) = self.active.clone() else {
            return;
        };
        match self.profile_writes(&name) {
            Ok(writes) if !writes.is_empty() => self.send_writes(writes, None),
            Ok(_) => {}
            Err(error) => tracing::warn!("cannot apply profile {name}: {}", error.message),
        }
    }

    fn handle_request(&mut self, request: Request, reply: Sender<Response>) {
        match request {
            Request::Set { setting } => self.set(&setting, reply),
            Request::ApplyProfile { name } => self.apply_profile(&name, reply),
            Request::Hello { .. } => respond(&reply, hello()),
            Request::Status => respond(&reply, Response::Status(self.status())),
            Request::Features => respond(&reply, self.features_response()),
            Request::ListProfiles => respond(&reply, self.list_profiles()),
            Request::SaveProfile { name } => self.save_profile_when_idle(name, reply),
            Request::DeleteProfile { name } => respond(
                &reply,
                self.mutate_store(|store| store.delete_profile(&name)),
            ),
            Request::SetDefaultProfile { name } => {
                respond(&reply, self.mutate_store(|store| store.set_default(&name)))
            }
            Request::LinkApp { profile, app } => respond(
                &reply,
                self.mutate_store(|store| store.link_app(&profile, &app)),
            ),
            Request::UnlinkApp { app } => {
                respond(&reply, self.mutate_store(|store| store.unlink_app(&app)))
            }
            Request::SetAutoSwitch { enabled } => respond(
                &reply,
                self.mutate_store(|store| store.set_audio_switch(enabled)),
            ),
            Request::Subscribe => respond(
                &reply,
                Response::Error(IpcError {
                    kind: ErrorKind::BadRequest,
                    message: "subscribe is answered by the connection, not the controller"
                        .to_string(),
                }),
            ),
        }
    }

    fn set(&mut self, setting: &Setting, reply: Sender<Response>) {
        if !self.is_connected() {
            let _ = reply.send(Response::Error(self.closed_error()));
            return;
        }
        match convert::write_for(setting, &self.snapshot) {
            Ok(write) => self.send_writes(vec![write], Some(reply)),
            Err(error) => {
                let _ = reply.send(Response::Error(error));
            }
        }
    }

    /// Writes still in flight when a profile is chosen were diffed against
    /// the older snapshot, so the profile is diffed again once they finish.
    fn apply_profile(&mut self, name: &str, reply: Sender<Response>) {
        let chosen = self.choose_profile(name);
        if chosen.is_ok() && !self.pending.is_empty() {
            self.apply_wanted = true;
        }
        match chosen {
            Ok(writes) if writes.is_empty() => {
                let _ = reply.send(Response::Done);
            }
            Ok(writes) => self.send_writes(writes, Some(reply)),
            Err(error) => {
                let _ = reply.send(Response::Error(error));
            }
        }
    }

    fn choose_profile(&mut self, name: &str) -> Result<Vec<DeviceWrite>, IpcError> {
        if !self.store.config()?.profiles.contains_key(name) {
            return Err(no_such_profile(name));
        }
        if !self.is_connected() {
            return Err(self.closed_error());
        }
        let writes = self.profile_writes(name)?;
        self.manual = Some(ManualProfile {
            name: name.to_string(),
            chosen_over: self.focused_app.clone(),
        });
        self.active = self.resolve_active();
        Ok(writes)
    }

    fn features_response(&self) -> Response {
        if !self.is_connected() {
            return Response::Error(self.not_connected());
        }
        Response::Features(
            self.features
                .iter()
                .map(|feature| Feature {
                    index: feature.index,
                    id: feature.id.0,
                    name: feature.id.name().map(str::to_string),
                    version: feature.version,
                })
                .collect(),
        )
    }

    fn list_profiles(&self) -> Response {
        let config = match self.store.config() {
            Ok(config) => config,
            Err(error) => return Response::Error(error),
        };
        Response::Profiles(
            config
                .profiles
                .iter()
                .map(|(name, profile)| ProfileInfo {
                    name: name.clone(),
                    apps: profile.apps.clone(),
                    is_default: *name == config.default_profile,
                    settings: profile.settings.clone(),
                })
                .collect(),
        )
    }

    fn save_profile(&mut self, name: &str) -> Response {
        if !self.is_connected() {
            return Response::Error(self.not_connected());
        }
        let settings = convert::profile_settings(&self.snapshot);
        self.mutate_store(|store| store.save_profile(name, settings))
    }

    /// The store re-reads the file before each change, so hand-edited audio
    /// switch names are passed on here too.
    fn mutate_store(
        &mut self,
        change: impl FnOnce(&mut Store) -> Result<(), IpcError>,
    ) -> Response {
        match change(&mut self.store) {
            Ok(()) => {
                self.refresh_active();
                self.configure_auto_switch();
                Response::Done
            }
            Err(error) => Response::Error(error),
        }
    }
}

fn respond(reply: &Sender<Response>, response: Response) {
    let _ = reply.send(response);
}

fn hello() -> Response {
    Response::Hello(Hello {
        version: IPC_VERSION,
        daemon_version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

fn no_such_profile(name: &str) -> IpcError {
    IpcError {
        kind: ErrorKind::NoSuchProfile,
        message: format!("no profile named {name}"),
    }
}

fn device_error(error: &DeviceError) -> IpcError {
    let kind = match error {
        DeviceError::NotFound | DeviceError::Disconnected => ErrorKind::NotConnected,
        DeviceError::GHubRunning => ErrorKind::GhubRunning,
        DeviceError::HeadsetDisconnected => ErrorKind::HeadsetOff,
        DeviceError::InvalidValue(_) => ErrorKind::InvalidValue,
        _ => ErrorKind::Device,
    };
    IpcError {
        kind,
        message: error.to_string(),
    }
}
