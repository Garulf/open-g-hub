//! Reads and changes the system default audio devices.

pub mod pipewire;
#[cfg(windows)]
mod windows;

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Direction {
    Render,
    Capture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Console,
    Multimedia,
    Communications,
}

/// One default-device assignment the system keeps: a direction and a role.
/// Written as `"<direction>.<role>"`, for example `render.console`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Slot {
    pub direction: Direction,
    pub role: Role,
}

impl Slot {
    pub const fn new(direction: Direction, role: Role) -> Slot {
        Slot { direction, role }
    }
}

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let direction = match self.direction {
            Direction::Render => "render",
            Direction::Capture => "capture",
        };
        let role = match self.role {
            Role::Console => "console",
            Role::Multimedia => "multimedia",
            Role::Communications => "communications",
        };
        write!(f, "{direction}.{role}")
    }
}

impl FromStr for Slot {
    type Err = String;

    fn from_str(text: &str) -> Result<Slot, String> {
        let invalid = || format!("invalid audio slot: {text}");
        let (direction, role) = text.split_once('.').ok_or_else(invalid)?;
        let direction = match direction {
            "render" => Direction::Render,
            "capture" => Direction::Capture,
            _ => return Err(invalid()),
        };
        let role = match role {
            "console" => Role::Console,
            "multimedia" => Role::Multimedia,
            "communications" => Role::Communications,
            _ => return Err(invalid()),
        };
        Ok(Slot { direction, role })
    }
}

impl Serialize for Slot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Slot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Slot, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(de::Error::custom)
    }
}

/// Endpoint ids by slot: the MMDevice id on Windows, the PipeWire `node.name` on Linux.
pub type Defaults = BTreeMap<Slot, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    /// A tool or API this machine lacks.
    Unavailable(String),
    /// A command or OS call that ran and failed.
    Failed(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::Unavailable(message) | AudioError::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for AudioError {}

pub trait AudioRouter: Send {
    /// The slots this system can set.
    fn slots(&self) -> &'static [Slot];
    fn current(&self) -> Result<Defaults, AudioError>;
    /// The id of the first active endpoint whose name contains `name_contains`, ignoring case.
    fn find(&self, direction: Direction, name_contains: &str)
    -> Result<Option<String>, AudioError>;
    fn set(&self, slot: Slot, endpoint: &str) -> Result<(), AudioError>;
}

#[cfg(windows)]
pub fn system_router() -> Result<Box<dyn AudioRouter>, AudioError> {
    Ok(Box::new(windows::WindowsRouter))
}

#[cfg(not(windows))]
pub fn system_router() -> Result<Box<dyn AudioRouter>, AudioError> {
    Ok(Box::new(pipewire::PipeWireRouter))
}

/// Prepares the calling thread for the router's OS calls. Call it once on the
/// thread that uses the router.
pub fn init_thread() {
    #[cfg(windows)]
    windows::init_thread();
}
