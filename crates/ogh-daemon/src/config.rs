use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::fs_util::write_atomic;
use ogh_ipc::{ErrorKind, IpcError, ProfileSettings};
use serde::{Deserialize, Serialize};

const DEFAULT_PROFILE: &str = "default";
const MAX_PROFILE_NAME_CHARS: usize = 64;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(flatten)]
    pub settings: ProfileSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSwitchConfig {
    pub enabled: bool,
    pub game_output: String,
    pub voice_output: String,
    pub mic_input: String,
}

impl Default for AudioSwitchConfig {
    fn default() -> Self {
        AudioSwitchConfig {
            enabled: false,
            game_output: "A50 X Game".to_string(),
            voice_output: "A50 X Voice".to_string(),
            mic_input: "A50 X Mic".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub default_profile: String,
    pub profiles: BTreeMap<String, Profile>,
    pub audio_switch: AudioSwitchConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            default_profile: DEFAULT_PROFILE.to_string(),
            profiles: BTreeMap::from([(DEFAULT_PROFILE.to_string(), Profile::default())]),
            audio_switch: AudioSwitchConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Loaded {
    Ok(Config),
    Missing,
    Unreadable(String),
}

pub fn load(path: &Path) -> Loaded {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if is_absent(&e) => return Loaded::Missing,
        Err(e) => return Loaded::Unreadable(format!("{}: {e}", path.display())),
    };
    match toml::from_str::<Config>(&text) {
        Ok(mut config) => {
            config
                .profiles
                .entry(config.default_profile.clone())
                .or_default();
            Loaded::Ok(config)
        }
        Err(e) => Loaded::Unreadable(format!("{}: {e}", path.display())),
    }
}

/// A parent that is a file also means there is no config file.
fn is_absent(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

pub fn save(path: &Path, config: &Config) -> io::Result<()> {
    let text = toml::to_string_pretty(config).map_err(io::Error::other)?;
    write_atomic(path, &text)
}

fn error(kind: ErrorKind, message: impl Into<String>) -> IpcError {
    IpcError {
        kind,
        message: message.into(),
    }
}

fn validate_profile_name(name: &str) -> Result<(), IpcError> {
    let chars = name.chars().count();
    let allowed = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-'));
    if !(1..=MAX_PROFILE_NAME_CHARS).contains(&chars)
        || !allowed
        || name.starts_with(' ')
        || name.ends_with(' ')
    {
        return Err(error(
            ErrorKind::InvalidValue,
            format!(
                "profile name must be 1 to {MAX_PROFILE_NAME_CHARS} characters of letters, digits, space, '_' or '-', without leading or trailing spaces"
            ),
        ));
    }
    Ok(())
}

fn normalize_app(app: &str) -> Result<String, IpcError> {
    let app = app.trim().to_lowercase();
    if app.is_empty() {
        return Err(error(ErrorKind::InvalidValue, "app name must not be empty"));
    }
    Ok(app)
}

fn no_such_profile(name: &str) -> IpcError {
    error(ErrorKind::NoSuchProfile, format!("no profile named {name}"))
}

fn lists_app(profile: &Profile, app: &str) -> bool {
    profile.apps.iter().any(|a| a.trim().to_lowercase() == app)
}

fn remove_app(config: &mut Config, app: &str) {
    for profile in config.profiles.values_mut() {
        profile.apps.retain(|a| a.trim().to_lowercase() != app);
    }
}

#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    config: Config,
    load_error: Option<String>,
}

impl Store {
    pub fn open(path: &Path) -> Store {
        let (config, load_error) = match load(path) {
            Loaded::Ok(config) => (config, None),
            Loaded::Missing => (Config::default(), None),
            Loaded::Unreadable(message) => (Config::default(), Some(message)),
        };
        Store {
            path: path.to_path_buf(),
            config,
            load_error,
        }
    }

    fn reload(&mut self) {
        match load(&self.path) {
            Loaded::Ok(config) => {
                self.config = config;
                self.load_error = None;
            }
            Loaded::Missing => {
                self.config = Config::default();
                self.load_error = None;
            }
            Loaded::Unreadable(message) => self.load_error = Some(message),
        }
    }

    pub fn config(&self) -> Result<&Config, IpcError> {
        match &self.load_error {
            Some(message) => Err(error(ErrorKind::ConfigUnavailable, message.clone())),
            None => Ok(&self.config),
        }
    }

    pub fn config_error(&self) -> Option<String> {
        self.load_error.clone()
    }

    pub fn profile_for_app(&self, app: &str) -> Option<&str> {
        if self.load_error.is_some() {
            return None;
        }
        let app = app.trim().to_lowercase();
        self.config
            .profiles
            .iter()
            .find(|(_, profile)| lists_app(profile, &app))
            .map(|(name, _)| name.as_str())
    }

    pub fn save_profile(&mut self, name: &str, settings: ProfileSettings) -> Result<(), IpcError> {
        validate_profile_name(name)?;
        self.mutate(|config| {
            config
                .profiles
                .entry(name.to_string())
                .or_default()
                .settings = settings;
            Ok(())
        })
    }

    pub fn delete_profile(&mut self, name: &str) -> Result<(), IpcError> {
        self.mutate(|config| {
            if !config.profiles.contains_key(name) {
                return Err(no_such_profile(name));
            }
            if config.default_profile == name {
                return Err(error(
                    ErrorKind::InvalidValue,
                    "the default profile cannot be deleted",
                ));
            }
            config.profiles.remove(name);
            Ok(())
        })
    }

    pub fn set_default(&mut self, name: &str) -> Result<(), IpcError> {
        self.mutate(|config| {
            if !config.profiles.contains_key(name) {
                return Err(no_such_profile(name));
            }
            config.default_profile = name.to_string();
            Ok(())
        })
    }

    pub fn link_app(&mut self, profile: &str, app: &str) -> Result<(), IpcError> {
        let app = normalize_app(app)?;
        self.mutate(|config| {
            if !config.profiles.contains_key(profile) {
                return Err(no_such_profile(profile));
            }
            remove_app(config, &app);
            if let Some(target) = config.profiles.get_mut(profile) {
                target.apps.push(app);
            }
            Ok(())
        })
    }

    pub fn unlink_app(&mut self, app: &str) -> Result<(), IpcError> {
        let app = normalize_app(app)?;
        self.mutate(|config| {
            remove_app(config, &app);
            Ok(())
        })
    }

    pub fn set_audio_switch(&mut self, enabled: bool) -> Result<(), IpcError> {
        self.mutate(|config| {
            config.audio_switch.enabled = enabled;
            Ok(())
        })
    }

    /// Re-reads the file first, so hand edits made while the daemon runs
    /// are kept, and a file that no longer parses is never overwritten.
    fn mutate(
        &mut self,
        change: impl FnOnce(&mut Config) -> Result<(), IpcError>,
    ) -> Result<(), IpcError> {
        self.reload();
        self.config()?;
        let mut next = self.config.clone();
        change(&mut next)?;
        save(&self.path, &next).map_err(|e| {
            error(
                ErrorKind::ConfigUnavailable,
                format!("{}: {e}", self.path.display()),
            )
        })?;
        self.config = next;
        Ok(())
    }
}
