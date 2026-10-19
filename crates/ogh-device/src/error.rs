use std::fmt;

use ogh_protocol::CentppError;

use crate::FeatureId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceError {
    NotFound,
    GHubRunning,
    NotAllowed {
        feature: FeatureId,
        function: u8,
    },
    /// The device answered with an error frame. `code` is raw: its mapping to
    /// the agent's error names is unverified.
    Rejected {
        feature_index: u8,
        code: u8,
    },
    Timeout,
    HeadsetDisconnected,
    Disconnected,
    FeatureMissing(FeatureId),
    /// The operating system refused to open the device.
    AccessDenied(String),
    Io(String),
}

struct FeatureName(FeatureId);

impl fmt::Display for FeatureName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let FeatureId(id) = self.0;
        match self.0.name() {
            Some(name) => write!(f, "{id:#06x} ({name})"),
            None => write!(f, "{id:#06x}"),
        }
    }
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(
                f,
                "no Astro A50 X base station found (USB 046D:0B0B); check that it is plugged in"
            ),
            Self::GHubRunning => write!(
                f,
                "Logitech G Hub is running (lghub_agent.exe) and owns the headset; \
                 quit G Hub from its tray icon, then try again"
            ),
            Self::NotAllowed { feature, function } => write!(
                f,
                "refusing to send function {function} of feature {}: G Hub was never seen \
                 sending it, so it is not on the allowlist",
                FeatureName(*feature)
            ),
            Self::Rejected {
                feature_index,
                code,
            } => {
                write!(
                    f,
                    "the device rejected a request to feature index {feature_index} with error code {code}"
                )?;
                match CentppError::from_code(*code) {
                    Some(name) => write!(f, " (probably {name:?})"),
                    None => Ok(()),
                }
            }
            Self::Timeout => write!(f, "the device did not answer in time"),
            Self::HeadsetDisconnected => write!(
                f,
                "the headset is not connected to the base station (docked, powered off or out of range)"
            ),
            Self::Disconnected => write!(f, "the base station was disconnected"),
            Self::FeatureMissing(feature) => write!(
                f,
                "the device does not offer feature {}",
                FeatureName(*feature)
            ),
            Self::AccessDenied(detail) => write!(f, "{detail}"),
            Self::Io(detail) => write!(f, "device I/O error: {detail}"),
        }
    }
}

impl std::error::Error for DeviceError {}
