//! A scripted stand-in for the HID device, for tests here and in `ogh-cli`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use ogh_protocol::FRAME_LEN;

use crate::error::DeviceError;
use crate::session::Session;
use crate::transport::Transport;

/// G Hub's launch replies (fixture `session-a/ghub-launch` lines 5, 7, 17,
/// 21 and 133) to the requests `Session` sends at open: getFeature(0x0001),
/// getCount, the feature list from index 2 and from 16, then the mic test
/// state `05` (headset docked).
pub const LAUNCH_OPEN_REPLIES: [&str; 5] = [
    "020c0600000b010001",
    "020c0400010b18",
    "020c3c00011b0e 010a0006 01020001 01000006 01010002 01040004 01080003 \
     02000004 06040002 06090002 03030004 08000001 020d0001 03050001 \
     01100001",
    "020c2400011b08 060b0001 01030001 00040004 020e0002 08060001 08050001 \
     08010001 08070001",
    "020c0400136b05",
];

#[derive(Clone, Copy, PartialEq)]
enum Arrival {
    /// Comes in while a read waits.
    Waiting,
    /// Comes in the moment the next write happens, then is buffered.
    OnNextWrite,
    Buffered,
}

enum ScriptedRead {
    Frame {
        frame: [u8; FRAME_LEN],
        arrival: Arrival,
    },
    Timeout,
    Failure,
}

#[derive(Default)]
struct Script {
    reads: VecDeque<ScriptedRead>,
    writes: Vec<[u8; FRAME_LEN]>,
    pending_at_write: Vec<usize>,
    failed: bool,
}

/// Clones share one script, so a test can keep a handle after moving the
/// transport into a `Session`.
///
/// Reads come back in script order. A frame from `push_frame` arrives while
/// a read waits, so a zero-timeout read does not see it; a frame from
/// `push_arrived` is already buffered and any read returns it; a frame from
/// `push_reply` is buffered by the next write, like an immediate answer. Once the
/// script runs out, every read times out, as a silent device would.
#[derive(Clone, Default)]
pub struct ScriptedTransport {
    script: Arc<Mutex<Script>>,
}

impl ScriptedTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// A device that answers `Session`'s open sequence as the A50 X did at
    /// G Hub's launch, with the headset docked.
    pub fn with_launch_discovery() -> Self {
        let device = Self::new();
        for frame in LAUNCH_OPEN_REPLIES {
            device.push_frame(frame_from_hex(frame));
        }
        device
    }

    pub fn push_frame(&self, frame: [u8; FRAME_LEN]) {
        self.push_with(frame, Arrival::Waiting);
    }

    pub fn push_arrived(&self, frame: [u8; FRAME_LEN]) {
        self.push_with(frame, Arrival::Buffered);
    }

    /// A frame the device sends the moment the next write reaches it, so a
    /// drain before that write cannot see it.
    pub fn push_reply(&self, frame: [u8; FRAME_LEN]) {
        self.push_with(frame, Arrival::OnNextWrite);
    }

    /// One waiting read that runs out its whole timeout and returns no data.
    pub fn push_timeout(&self) {
        self.push(ScriptedRead::Timeout);
    }

    /// The device goes away: this read and every later read or write fails.
    pub fn push_failure(&self) {
        self.push(ScriptedRead::Failure);
    }

    pub fn writes(&self) -> Vec<[u8; FRAME_LEN]> {
        self.lock().writes.clone()
    }

    pub fn pending_reads(&self) -> usize {
        self.lock().reads.len()
    }

    /// Scripted reads still unserved at each write, in write order.
    pub fn pending_reads_at_writes(&self) -> Vec<usize> {
        self.lock().pending_at_write.clone()
    }

    fn push_with(&self, frame: [u8; FRAME_LEN], arrival: Arrival) {
        self.push(ScriptedRead::Frame { frame, arrival });
    }

    fn push(&self, read: ScriptedRead) {
        self.lock().reads.push_back(read);
    }

    fn lock(&self) -> MutexGuard<'_, Script> {
        self.script
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Hex with optional spaces, zero-padded to a full frame.
pub fn frame_from_hex(hex: &str) -> [u8; FRAME_LEN] {
    let digits: Vec<u8> = hex.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    assert!(
        digits.len().is_multiple_of(2) && digits.len() <= FRAME_LEN * 2,
        "bad frame hex {hex}"
    );
    let mut frame = [0u8; FRAME_LEN];
    for (byte, pair) in frame.iter_mut().zip(digits.chunks(2)) {
        let text = std::str::from_utf8(pair).expect("ascii hex");
        *byte = u8::from_str_radix(text, 16).unwrap_or_else(|_| panic!("bad frame hex {hex}"));
    }
    frame
}

impl<T: Transport> Session<T> {
    /// Opens a session on any transport and runs the open sequence on it.
    /// Test-only: it skips the G Hub guard, and no public `Transport` reaches
    /// the real device.
    pub fn with_transport(transport: T) -> Result<Self, DeviceError> {
        Self::open_with(|| Ok(false), || Ok(transport))
    }

    /// `with_transport` with a request timeout that also applies to the
    /// requests of the open sequence.
    pub fn with_transport_and_timeout(
        transport: T,
        request_timeout: Duration,
    ) -> Result<Self, DeviceError> {
        Self::open_with_timeout(|| Ok(false), || Ok(transport), request_timeout)
    }
}

impl Transport for ScriptedTransport {
    fn write(&mut self, frame: &[u8; FRAME_LEN]) -> Result<(), DeviceError> {
        let mut script = self.lock();
        if script.failed {
            return Err(DeviceError::Disconnected);
        }
        script.writes.push(*frame);
        let pending = script.reads.len();
        script.pending_at_write.push(pending);
        let released = script.reads.iter_mut().find_map(|read| match read {
            ScriptedRead::Frame { arrival, .. } if *arrival == Arrival::OnNextWrite => {
                Some(arrival)
            }
            _ => None,
        });
        if let Some(arrival) = released {
            *arrival = Arrival::Buffered;
        }
        Ok(())
    }

    fn read(&mut self, timeout_ms: i32) -> Result<Option<[u8; FRAME_LEN]>, DeviceError> {
        let next = {
            let mut script = self.lock();
            if script.failed {
                return Err(DeviceError::Disconnected);
            }
            let waits = timeout_ms > 0;
            match script.reads.front() {
                Some(
                    ScriptedRead::Frame {
                        arrival: Arrival::Waiting | Arrival::OnNextWrite,
                        ..
                    }
                    | ScriptedRead::Timeout,
                ) if !waits => None,
                _ => script.reads.pop_front(),
            }
        };
        match next {
            Some(ScriptedRead::Frame { frame, .. }) => Ok(Some(frame)),
            Some(ScriptedRead::Failure) => {
                self.lock().failed = true;
                Err(DeviceError::Disconnected)
            }
            Some(ScriptedRead::Timeout) | None => {
                let wait = u64::try_from(timeout_ms.max(0)).unwrap_or(0);
                thread::sleep(Duration::from_millis(wait));
                Ok(None)
            }
        }
    }
}
