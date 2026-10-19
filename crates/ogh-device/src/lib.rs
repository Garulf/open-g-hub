//! HID session for the Astro A50 X base station.
//!
//! Every request passes one allowlist gate before anything is written, and
//! unsolicited frames read while waiting for a reply are kept as events.
//!
//! [`Session::open`] is the only way to reach the real device: it checks for
//! G Hub first, and the raw transport cannot be opened from outside.
//!
//! ```compile_fail,E0624
//! let _ = ogh_device::HidTransport::open();
//! ```
//!
//! ```compile_fail,E0624
//! let _ = ogh_device::Session::<ogh_device::HidTransport>::open_with(
//!     || Ok(false),
//!     || unreachable!(),
//! );
//! ```

pub mod allowlist;
mod error;
pub mod guard;
mod param_rules;
mod session;
#[cfg(feature = "testing")]
pub mod testing;
mod transport;

pub use error::{DeviceError, Refusal};
pub use ogh_protocol::FeatureId;
pub use ogh_protocol::discovery::FeatureInfo;
pub use ogh_protocol::features::Event;
pub use session::Session;
pub use transport::{HidTransport, Transport};
