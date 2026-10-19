use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;

use crossbeam_channel::{Sender, unbounded};
use ogh_audio::{AudioError, AudioRouter, Defaults, Direction, Role, Slot};
use ogh_ipc::Headset;
use serde::{Deserialize, Serialize};

use crate::config::AudioSwitchConfig;

/// What the last audio action left behind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioSwitchOutcome {
    /// Defaults are currently moved to the headset and remembered.
    pub active: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Headset(Headset),
    Configure(AudioSwitchConfig),
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct StateFile {
    remembered: Defaults,
}

pub struct AutoSwitch {
    router: Box<dyn AudioRouter>,
    config: AudioSwitchConfig,
    state_file: PathBuf,
    remembered: Defaults,
    previous: Headset,
    error: Option<String>,
}

impl AutoSwitch {
    pub fn new(
        router: Box<dyn AudioRouter>,
        config: AudioSwitchConfig,
        state_file: PathBuf,
    ) -> AutoSwitch {
        let remembered = load_remembered(&state_file);
        AutoSwitch {
            router,
            config,
            state_file,
            remembered,
            previous: Headset::Unknown,
            error: None,
        }
    }

    pub fn on_headset(&mut self, headset: Headset) -> AudioSwitchOutcome {
        match headset {
            Headset::InUse => {
                let undocked = self.previous != Headset::InUse;
                self.previous = Headset::InUse;
                if undocked && self.config.enabled {
                    self.switch();
                }
            }
            Headset::Docked | Headset::Off => {
                self.previous = headset;
                self.restore();
            }
            Headset::Unknown => {}
        }
        self.outcome()
    }

    pub fn configure(&mut self, config: AudioSwitchConfig) -> AudioSwitchOutcome {
        let was_enabled = self.config.enabled;
        self.config = config;
        if was_enabled && !self.config.enabled {
            self.restore();
        } else if !was_enabled && self.config.enabled && self.previous == Headset::InUse {
            self.switch();
        }
        self.outcome()
    }

    fn outcome(&self) -> AudioSwitchOutcome {
        AudioSwitchOutcome {
            active: !self.remembered.is_empty(),
            error: self.error.clone(),
        }
    }

    fn switch(&mut self) {
        self.error = self.try_switch().err();
    }

    fn try_switch(&mut self) -> Result<(), String> {
        let slots = self.router.slots();
        let targets = self.resolve_targets(slots)?;
        if self.remembered.is_empty() {
            let current = self.router.current().map_err(|e| e.to_string())?;
            self.remembered = slots
                .iter()
                .filter_map(|slot| Some((*slot, current.get(slot)?.clone())))
                .collect();
            self.persist();
        }
        let failures: Vec<String> = slots
            .iter()
            .filter_map(|slot| {
                let endpoint = &targets[slot];
                let failure = self.router.set(*slot, endpoint).err()?;
                Some(format!("could not set {slot}: {failure}"))
            })
            .collect();
        failures_to_result(failures)
    }

    /// The endpoint id for every slot this router sets. Nothing is returned
    /// unless every configured name resolves.
    fn resolve_targets(&self, slots: &[Slot]) -> Result<BTreeMap<Slot, String>, String> {
        let mut found: BTreeMap<(Direction, &str), String> = BTreeMap::new();
        let mut looked_up: BTreeSet<(Direction, &str)> = BTreeSet::new();
        let mut missing: Vec<&str> = Vec::new();
        for slot in slots {
            let name = self.target_name(*slot);
            let key = (slot.direction, name);
            if !looked_up.insert(key) {
                continue;
            }
            match self
                .router
                .find(slot.direction, name)
                .map_err(|e| e.to_string())?
            {
                Some(endpoint) => {
                    found.insert(key, endpoint);
                }
                None => missing.push(name),
            }
        }
        if !missing.is_empty() {
            return Err(format!("audio device not found: {}", missing.join(", ")));
        }
        Ok(slots
            .iter()
            .map(|slot| {
                (
                    *slot,
                    found[&(slot.direction, self.target_name(*slot))].clone(),
                )
            })
            .collect())
    }

    fn target_name(&self, slot: Slot) -> &str {
        match (slot.direction, slot.role) {
            (Direction::Capture, _) => &self.config.mic_input,
            (Direction::Render, Role::Communications) => &self.config.voice_output,
            (Direction::Render, _) => &self.config.game_output,
        }
    }

    fn restore(&mut self) {
        if self.remembered.is_empty() {
            return;
        }
        let slots = self.router.slots();
        let failures: Vec<String> = std::mem::take(&mut self.remembered)
            .into_iter()
            .filter(|(slot, _)| slots.contains(slot))
            .filter_map(|(slot, endpoint)| {
                let failure = self.router.set(slot, &endpoint).err()?;
                Some(format!("could not restore {slot}: {failure}"))
            })
            .collect();
        self.persist();
        self.error = failures_to_result(failures).err();
    }

    fn persist(&self) {
        if let Err(e) = save_remembered(&self.state_file, &self.remembered) {
            tracing::warn!("cannot save {}: {e}", self.state_file.display());
        }
    }
}

fn failures_to_result(failures: Vec<String>) -> Result<(), String> {
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn load_remembered(path: &Path) -> Defaults {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Defaults::new(),
        Err(e) => {
            tracing::warn!("cannot read {}: {e}", path.display());
            return Defaults::new();
        }
    };
    match toml::from_str::<StateFile>(&text) {
        Ok(state) => state.remembered,
        Err(e) => {
            tracing::warn!("ignoring {}: {e}", path.display());
            Defaults::new()
        }
    }
}

fn save_remembered(path: &Path, remembered: &Defaults) -> io::Result<()> {
    let state = StateFile {
        remembered: remembered.clone(),
    };
    let text = toml::to_string_pretty(&state).map_err(io::Error::other)?;
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent)?;
    }
    let mut tmp_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "state path has no file name"))?
        .to_os_string();
    tmp_name.push(OsString::from(".tmp"));
    let tmp = path.with_file_name(tmp_name);
    let mut file = File::create(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
    #[cfg(unix)]
    File::open(parent.unwrap_or(Path::new(".")))?.sync_all()?;
    Ok(())
}

/// Runs an `AutoSwitch` on its own thread so the OS audio calls stay off the
/// controller thread. The thread ends when the returned sender is dropped or
/// `outcomes` has no receiver.
pub fn spawn(
    router: impl FnOnce() -> Result<Box<dyn AudioRouter>, AudioError> + Send + 'static,
    config: AudioSwitchConfig,
    state_file: PathBuf,
    outcomes: Sender<AudioSwitchOutcome>,
) -> Sender<Command> {
    let (commands, command_rx) = unbounded();
    thread::spawn(move || {
        ogh_audio::init_thread();
        let mut auto_switch = match router() {
            Ok(router) => AutoSwitch::new(router, config, state_file),
            Err(e) => {
                let outcome = AudioSwitchOutcome {
                    active: false,
                    error: Some(format!("audio switching unavailable: {e}")),
                };
                for _ in command_rx {
                    if outcomes.send(outcome.clone()).is_err() {
                        return;
                    }
                }
                return;
            }
        };
        for command in command_rx {
            let outcome = match command {
                Command::Headset(headset) => auto_switch.on_headset(headset),
                Command::Configure(config) => auto_switch.configure(config),
            };
            if outcomes.send(outcome).is_err() {
                return;
            }
        }
    });
    commands
}
