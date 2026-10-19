use serde::{Deserialize, Serialize};

pub const IPC_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Hello { version: u32 },
    Status,
    Features,
    Set { setting: Setting },
    ListProfiles,
    ApplyProfile { name: String },
    SaveProfile { name: String },
    DeleteProfile { name: String },
    SetDefaultProfile { name: String },
    LinkApp { profile: String, app: String },
    UnlinkApp { app: String },
    SetAutoSwitch { enabled: bool },
    Subscribe,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Setting {
    Sidetone { percent: u8 },
    Balance { value: u8 },
    NoiseGate(Gate),
    HeadphoneEq(Eq),
    MicEq(Eq),
    StreamMix(StreamMixSetting),
    Volume { percent: u8 },
    LedBrightness { percent: u8 },
    SleepAfter { minutes: u8 },
    Tones { value: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    Off,
    Home,
    Night,
    Tournament,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Eq {
    pub bands: Vec<EqBand>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EqBand {
    pub freq_hz: u16,
    pub q: Option<f32>,
    pub gain_db: f32,
}

const EQ_BANDS: usize = 10;

impl Eq {
    pub fn validate(&self, min_freq_hz: u16) -> Result<(), String> {
        if self.bands.len() != EQ_BANDS {
            return Err(format!(
                "expected {EQ_BANDS} bands, got {}",
                self.bands.len()
            ));
        }
        if self.bands[0].freq_hz < min_freq_hz {
            return Err(format!("first band must be at least {min_freq_hz} Hz"));
        }
        if self.bands.windows(2).any(|w| w[1].freq_hz <= w[0].freq_hz) {
            return Err("band frequencies must rise".to_string());
        }
        let parametric = self.bands.iter().filter(|b| b.q.is_some()).count();
        if parametric != 0 && parametric != EQ_BANDS {
            return Err("bands must be all graphic or all parametric".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Channel {
    pub level_percent: u8,
    pub muted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StreamMixSetting {
    pub output: Channel,
    pub game: Channel,
    pub voice: Channel,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProfileSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidetone_percent: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noise_gate: Option<Gate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headphone_eq: Option<Eq>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mic_eq: Option<Eq>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_mix: Option<StreamMixSetting>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DeviceSettings {
    pub profile: ProfileSettings,
    pub volume_percent: Option<u8>,
    pub led_brightness_percent: Option<u8>,
    pub sleep_after_minutes: Option<u8>,
    pub tones: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    Connected,
    NotFound,
    GhubRunning,
    AccessDenied { message: String },
    Error { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Headset {
    InUse,
    Docked,
    Off,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Charge {
    Discharging,
    Charging,
    Full,
    Other(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Battery {
    pub percent: u8,
    pub charge: Charge,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AutoSwitchStatus {
    pub enabled: bool,
    pub active: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FocusStatus {
    pub available: bool,
    pub app: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Status {
    pub connection: Connection,
    pub headset: Headset,
    pub battery: Option<Battery>,
    pub firmware: Option<String>,
    pub settings: DeviceSettings,
    pub active_profile: Option<String>,
    pub focus: FocusStatus,
    pub auto_switch: AutoSwitchStatus,
    pub config_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Feature {
    pub index: u8,
    pub id: u16,
    pub name: Option<String>,
    pub version: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProfileInfo {
    pub name: String,
    pub apps: Vec<String>,
    pub is_default: bool,
    pub settings: ProfileSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Hello {
    pub version: u32,
    pub daemon_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    VersionMismatch,
    BadRequest,
    NotConnected,
    GhubRunning,
    HeadsetOff,
    InvalidValue,
    NoSuchProfile,
    ConfigUnavailable,
    Device,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct IpcError {
    pub kind: ErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum Response {
    Hello(Hello),
    Status(Status),
    Features(Vec<Feature>),
    Profiles(Vec<ProfileInfo>),
    Done,
    Error(IpcError),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Notification {
    Status(Status),
}
