//! A scripted stand-in for the HID device, for tests here and in `ogh-cli`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use ogh_protocol::FRAME_LEN;

use crate::error::DeviceError;
use crate::transport::Transport;

enum ScriptedRead {
    Frame([u8; FRAME_LEN]),
    Timeout,
    Failure,
}

#[derive(Default)]
struct Script {
    reads: VecDeque<ScriptedRead>,
    writes: Vec<[u8; FRAME_LEN]>,
    failed: bool,
}

/// Clones share one script, so a test can keep a handle after moving the
/// transport into a `Session`. Once the script runs out, every read times
/// out, as a silent device would.
#[derive(Clone, Default)]
pub struct ScriptedTransport {
    script: Arc<Mutex<Script>>,
}

impl ScriptedTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_frame(&self, frame: [u8; FRAME_LEN]) {
        self.lock().reads.push_back(ScriptedRead::Frame(frame));
    }

    /// One read that waits out its whole timeout and returns no data.
    pub fn push_timeout(&self) {
        self.lock().reads.push_back(ScriptedRead::Timeout);
    }

    /// The device goes away: this read and every later read or write fails.
    pub fn push_failure(&self) {
        self.lock().reads.push_back(ScriptedRead::Failure);
    }

    pub fn writes(&self) -> Vec<[u8; FRAME_LEN]> {
        self.lock().writes.clone()
    }

    pub fn pending_reads(&self) -> usize {
        self.lock().reads.len()
    }

    fn lock(&self) -> MutexGuard<'_, Script> {
        self.script
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Transport for ScriptedTransport {
    fn write(&mut self, frame: &[u8; FRAME_LEN]) -> Result<(), DeviceError> {
        let mut script = self.lock();
        if script.failed {
            return Err(DeviceError::Disconnected);
        }
        script.writes.push(*frame);
        Ok(())
    }

    fn read(&mut self, timeout_ms: i32) -> Result<Option<[u8; FRAME_LEN]>, DeviceError> {
        let next = {
            let mut script = self.lock();
            if script.failed {
                return Err(DeviceError::Disconnected);
            }
            script.reads.pop_front()
        };
        match next {
            Some(ScriptedRead::Frame(frame)) => Ok(Some(frame)),
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
