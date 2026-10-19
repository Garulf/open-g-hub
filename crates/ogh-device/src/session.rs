use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ogh_protocol::discovery::{
    FeatureInfo, get_count_request, get_feature_request, list_features_request, parse_count,
    parse_feature_page, parse_get_feature,
};
use ogh_protocol::features::{Event, mic_test, parse_event};
use ogh_protocol::{
    A50X_PREFIX, CentppError, Decoded, FRAME_LEN, GHUB_SW_ID, Message, ParseError, decode, encode,
    request,
};

use crate::allowlist::Allowlist;
use crate::error::{DeviceError, Refusal};
use crate::transport::Transport;
use crate::{FeatureId, HidTransport, guard, param_rules};

/// Longer than the slowest reply captured: 2.25 s for getHardwareInfo while
/// the base station was still booting after a replug.
const REQUEST_TIMEOUT: Duration = Duration::from_millis(3000);
/// Longer waits are clamped, so the deadline arithmetic cannot overflow.
const MAX_REQUEST_TIMEOUT: Duration = Duration::from_secs(3600);
const ROOT_INDEX: u8 = 0;
/// Root and FeatureSet sit at 0 and 1; the paged list starts after them.
const FIRST_LISTED_INDEX: u8 = 2;
const SINGLE_FRAME: u8 = 0;
const EVENT_SW_ID: u8 = 0;
const NO_WAIT: i32 = 0;
/// A chatty device must not stall a send: whatever is left is read while
/// waiting for the reply.
const MAX_DRAIN_FRAMES: usize = 64;

/// Root is never listed by the device, so its flags and version read 0.
const ROOT: FeatureInfo = FeatureInfo {
    index: ROOT_INDEX,
    id: FeatureId::ROOT,
    flags: 0,
    version: 0,
};

/// An open device with its discovered feature table. One request is in
/// flight at a time; frames that arrive while waiting for a reply and are
/// not that reply go to the event queue or are dropped.
///
/// Replies are matched on feature index, function and G Hub's fixed sw_id,
/// so they carry no request identity. Input already waiting is drained
/// before every write, which removes late replies to earlier requests that
/// arrived in the meantime. A late reply that lands after that drain and
/// before the real reply is still taken as the answer; only a per-request
/// sw_id would close that window, and no sw_id other than 0xB has been seen
/// on this device.
pub struct Session<T: Transport> {
    transport: T,
    allowlist: &'static Allowlist,
    features: Vec<FeatureInfo>,
    events: VecDeque<Event>,
    headset_available: Option<bool>,
    request_timeout: Duration,
}

impl Session<HidTransport> {
    /// Refuses while G Hub's agent runs, then opens the HID device, runs
    /// discovery and reads whether the headset is connected. A timed-out or
    /// rejected availability read leaves it unknown instead of failing.
    pub fn open() -> Result<Self, DeviceError> {
        Self::open_with(guard::ghub_agent_running, HidTransport::open)
    }
}

impl<T: Transport> Session<T> {
    /// `guard` is checked before `opener` is called; true or an error means
    /// nothing is opened.
    pub(crate) fn open_with(
        guard: fn() -> Result<bool, DeviceError>,
        opener: impl FnOnce() -> Result<T, DeviceError>,
    ) -> Result<Self, DeviceError> {
        Self::open_with_timeout(guard, opener, REQUEST_TIMEOUT)
    }

    /// Like `open_with`, with `request_timeout` also applying to the
    /// requests of the open sequence.
    pub(crate) fn open_with_timeout(
        guard: fn() -> Result<bool, DeviceError>,
        opener: impl FnOnce() -> Result<T, DeviceError>,
        request_timeout: Duration,
    ) -> Result<Self, DeviceError> {
        if guard()? {
            return Err(DeviceError::GHubRunning);
        }
        let allowlist = Allowlist::embedded().map_err(|e| DeviceError::Io(e.to_string()))?;
        let mut session = Self {
            transport: opener()?,
            allowlist,
            features: vec![ROOT],
            events: VecDeque::new(),
            headset_available: None,
            request_timeout: request_timeout.min(MAX_REQUEST_TIMEOUT),
        };
        session.discover()?;
        match session.headset_available() {
            Ok(_) | Err(DeviceError::Timeout | DeviceError::Rejected { .. }) => Ok(session),
            Err(e) => Err(e),
        }
    }

    /// The table read at open, from Root at index 0 upwards.
    pub fn features(&self) -> &[FeatureInfo] {
        &self.features
    }

    /// Sends a message built from the discovered index of `id`.
    pub fn call(
        &mut self,
        id: FeatureId,
        build: impl FnOnce(u8) -> Message,
    ) -> Result<Vec<u8>, DeviceError> {
        let index = self.index_of(id)?;
        self.send(id, build(index))
    }

    pub fn request(
        &mut self,
        id: FeatureId,
        function: u8,
        params: &[u8],
    ) -> Result<Vec<u8>, DeviceError> {
        let index = self.index_of(id)?;
        self.send(id, request(index, function, params))
    }

    /// Reads the mic test state, which is "not connected" while the headset
    /// is docked or off. Later timeouts are read in the light of this answer.
    pub fn headset_available(&mut self) -> Result<bool, DeviceError> {
        let params = match self.call(FeatureId::MIC_TEST, mic_test::get_state_request) {
            Err(DeviceError::HeadsetDisconnected) => {
                self.headset_available = Some(false);
                return Ok(false);
            }
            other => other?,
        };
        let available = mic_test::parse_state(&params)
            .map_err(bad_reply)?
            .is_headset_available();
        self.headset_available = Some(available);
        Ok(available)
    }

    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    /// How long a request waits for its reply, measured as wall time across
    /// reads. Capped at an hour.
    pub fn set_request_timeout(&mut self, timeout: Duration) {
        self.request_timeout = timeout.min(MAX_REQUEST_TIMEOUT);
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        self.events.drain(..).collect()
    }

    /// The oldest queued event, or the next one to arrive within
    /// `timeout_ms`. Zero or less polls once without blocking. Frames that
    /// are not modelled events are dropped.
    pub fn next_event(&mut self, timeout_ms: i32) -> Result<Option<Event>, DeviceError> {
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        let wait = Duration::from_millis(u64::try_from(timeout_ms).unwrap_or(0));
        let deadline = Instant::now() + wait;
        loop {
            let remaining = remaining_ms(deadline);
            if let Some(frame) = self.transport.read(remaining.unwrap_or(NO_WAIT))?
                && let Some(event) = self.event_in(&frame)
            {
                return Ok(Some(event));
            }
            if remaining.is_none() {
                return Ok(None);
            }
        }
    }

    fn discover(&mut self) -> Result<(), DeviceError> {
        let reply = self.send(FeatureId::ROOT, get_feature_request(FeatureId::FEATURE_SET))?;
        let feature_set = parse_get_feature(FeatureId::FEATURE_SET, &reply)
            .map_err(bad_reply)?
            .ok_or(DeviceError::FeatureMissing(FeatureId::FEATURE_SET))?;
        self.add_feature(feature_set)?;

        let reply = self.send(FeatureId::FEATURE_SET, get_count_request(feature_set.index))?;
        let count = parse_count(&reply).map_err(bad_reply)?;

        let mut start = FIRST_LISTED_INDEX;
        while start < count {
            let reply = self.send(
                FeatureId::FEATURE_SET,
                list_features_request(feature_set.index, start),
            )?;
            let page = parse_feature_page(start, &reply).map_err(bad_reply)?;
            let Some(next) = u8::try_from(page.len())
                .ok()
                .filter(|&n| n > 0)
                .and_then(|n| start.checked_add(n))
            else {
                return Err(bad_reply(ParseError::IndexOverflow));
            };
            for info in page {
                self.add_feature(info)?;
            }
            start = next;
        }
        Ok(())
    }

    fn add_feature(&mut self, info: FeatureInfo) -> Result<(), DeviceError> {
        if self.features.iter().any(|known| known.id == info.id) {
            let FeatureId(id) = info.id;
            return Err(DeviceError::Io(format!(
                "the device lists feature {id:#06x} twice"
            )));
        }
        self.features.push(info);
        Ok(())
    }

    fn index_of(&self, id: FeatureId) -> Result<u8, DeviceError> {
        self.features
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.index)
            .ok_or(DeviceError::FeatureMissing(id))
    }

    fn id_at(&self, index: u8) -> Option<FeatureId> {
        self.features
            .iter()
            .find(|f| f.index == index)
            .map(|f| f.id)
    }

    /// The single gate every outgoing message passes. The message must
    /// target the discovered index of `id` with G Hub's sw_id, (`id`,
    /// function) must be allowlisted, and its params must pass any parameter
    /// rule for that call. Nothing is written otherwise.
    fn send(&mut self, id: FeatureId, msg: Message) -> Result<Vec<u8>, DeviceError> {
        self.check_sendable(id, &msg)?;
        let frame = encode(A50X_PREFIX, SINGLE_FRAME, &msg)
            .map_err(|e| DeviceError::Io(format!("cannot encode request: {e}")))?;
        self.drain_pending()?;
        self.transport.write(&frame)?;
        self.await_reply(&msg)
    }

    fn check_sendable(&self, id: FeatureId, msg: &Message) -> Result<(), DeviceError> {
        let refused = |reason| DeviceError::Refused {
            feature: id,
            function: msg.function,
            reason,
        };
        if self.index_of(id)? != msg.feature_index {
            return Err(refused(Refusal::FeatureIndex));
        }
        if msg.sw_id != GHUB_SW_ID {
            return Err(refused(Refusal::SwId));
        }
        if !self.allowlist.permits(id, msg.function) {
            return Err(DeviceError::NotAllowed {
                feature: id,
                function: msg.function,
            });
        }
        if !param_rules::permits(id, msg.function, &msg.params) {
            return Err(refused(Refusal::Params));
        }
        Ok(())
    }

    /// Queues events already waiting and discards everything else, so a late
    /// reply to an earlier request cannot answer the next one. Stops after
    /// `MAX_DRAIN_FRAMES` frames.
    fn drain_pending(&mut self) -> Result<(), DeviceError> {
        for _ in 0..MAX_DRAIN_FRAMES {
            let Some(frame) = self.transport.read(NO_WAIT)? else {
                break;
            };
            if let Some(event) = self.event_in(&frame) {
                self.events.push_back(event);
            }
        }
        Ok(())
    }

    fn await_reply(&mut self, sent: &Message) -> Result<Vec<u8>, DeviceError> {
        let deadline = Instant::now() + self.request_timeout;
        while let Some(remaining) = remaining_ms(deadline) {
            let Some(frame) = self.transport.read(remaining)? else {
                continue;
            };
            if let Some(outcome) = self.handle_frame(sent, &frame) {
                return outcome;
            }
        }
        Err(if self.headset_available == Some(false) {
            DeviceError::HeadsetDisconnected
        } else {
            DeviceError::Timeout
        })
    }

    /// `Some` ends the request; `None` keeps waiting.
    fn handle_frame(
        &mut self,
        sent: &Message,
        frame: &[u8; FRAME_LEN],
    ) -> Option<Result<Vec<u8>, DeviceError>> {
        match decode(A50X_PREFIX, frame).ok()? {
            Decoded::Reply {
                flags: SINGLE_FRAME,
                msg,
            } => {
                if answers(sent, msg.feature_index, msg.function, msg.sw_id) {
                    return Some(Ok(msg.params));
                }
                if let Some(event) = self.event_from(&msg) {
                    self.events.push_back(event);
                }
                None
            }
            Decoded::Error {
                feature_index,
                function,
                sw_id,
                code,
            } if answers(sent, feature_index, function, sw_id) => {
                Some(Err(rejection(feature_index, code)))
            }
            _ => None,
        }
    }

    fn event_in(&mut self, frame: &[u8; FRAME_LEN]) -> Option<Event> {
        match decode(A50X_PREFIX, frame).ok()? {
            Decoded::Reply {
                flags: SINGLE_FRAME,
                msg,
            } => self.event_from(&msg),
            _ => None,
        }
    }

    fn event_from(&mut self, msg: &Message) -> Option<Event> {
        if msg.sw_id != EVENT_SW_ID {
            return None;
        }
        let event = parse_event(self.id_at(msg.feature_index)?, msg)?;
        if let Event::HeadsetAvailability(available) = event {
            self.headset_available = Some(available);
        }
        Some(event)
    }
}

fn answers(sent: &Message, feature_index: u8, function: u8, sw_id: u8) -> bool {
    feature_index == sent.feature_index && function == sent.function && sw_id == GHUB_SW_ID
}

/// The NotConnected code value is the agent's list position, unverified.
fn rejection(feature_index: u8, code: u8) -> DeviceError {
    if CentppError::from_code(code) == Some(CentppError::NotConnected) {
        DeviceError::HeadsetDisconnected
    } else {
        DeviceError::Rejected {
            feature_index,
            code,
        }
    }
}

fn bad_reply(e: ParseError) -> DeviceError {
    DeviceError::Io(format!("unexpected reply: {e}"))
}

/// Time left before `deadline`, rounded up to whole milliseconds, or `None`
/// once it has passed.
fn remaining_ms(deadline: Instant) -> Option<i32> {
    let left = deadline.checked_duration_since(Instant::now())?;
    if left.is_zero() {
        return None;
    }
    let ms = left.as_micros().div_ceil(1000);
    Some(i32::try_from(ms).unwrap_or(i32::MAX))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    struct Unopened;

    impl Transport for Unopened {
        fn write(&mut self, _frame: &[u8; FRAME_LEN]) -> Result<(), DeviceError> {
            unreachable!("never opened")
        }

        fn read(&mut self, _timeout_ms: i32) -> Result<Option<[u8; FRAME_LEN]>, DeviceError> {
            unreachable!("never opened")
        }
    }

    fn ghub_running() -> Result<bool, DeviceError> {
        Ok(true)
    }

    fn process_list_unreadable() -> Result<bool, DeviceError> {
        Err(DeviceError::Io("no process list".into()))
    }

    fn open_counting(
        guard: fn() -> Result<bool, DeviceError>,
    ) -> (Result<Session<Unopened>, DeviceError>, bool) {
        let opened = Cell::new(false);
        let result = Session::open_with(guard, || {
            opened.set(true);
            Ok(Unopened)
        });
        (result, opened.get())
    }

    #[test]
    fn guard_blocks_open() {
        let (result, opened) = open_counting(ghub_running);

        assert!(matches!(result, Err(DeviceError::GHubRunning)));
        assert!(!opened);
    }

    #[test]
    fn unreadable_process_list_blocks_open() {
        let (result, opened) = open_counting(process_list_unreadable);

        assert!(matches!(result, Err(DeviceError::Io(_))));
        assert!(!opened);
    }
}
