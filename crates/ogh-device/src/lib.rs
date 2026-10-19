//! HID session for the Astro A50 X base station.
//!
//! Every request passes one allowlist gate before anything is written, and
//! unsolicited frames read while waiting for a reply are kept as events.

pub mod allowlist;
mod error;
pub mod guard;
mod session;
#[cfg(feature = "testing")]
pub mod testing;
mod transport;

pub use error::DeviceError;
pub use ogh_protocol::FeatureId;
pub use ogh_protocol::discovery::FeatureInfo;
pub use ogh_protocol::features::Event;
pub use session::Session;
pub use transport::{HidTransport, Transport};
