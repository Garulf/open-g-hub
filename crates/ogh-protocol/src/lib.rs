//! Frame encoding and decoding for the Astro A50 X wire protocol.

pub mod discovery;
mod error;
mod feature_id;
pub mod features;
mod frame;

pub use error::{Allowed, CentppError, FrameError, ParseError, RangeError};
pub use feature_id::FeatureId;
pub use frame::{A50X_PREFIX, Decoded, FRAME_LEN, GHUB_SW_ID, Message, decode, encode, request};
