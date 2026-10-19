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
use crate::error::DeviceError;
use crate::transport::Transport;
use crate::{FeatureId, HidTransport, guard};

const REQUEST_TIMEOUT: Duration = Duration::from_millis(1000);
const ROOT_INDEX: u8 = 0;
/// Root and FeatureSet sit at 0 and 1; the paged list starts after them.
const FIRST_LISTED_INDEX: u8 = 2;
const SINGLE_FRAME: u8 = 0;
const EVENT_SW_ID: u8 = 0;

/// An open device with its discovered feature table. One request is in
/// flight at a time; frames that arrive while waiting for a reply and are
/// not that reply go to the event queue or are dropped.
pub struct Session<T: Transport> {
    transport: T,
    allowlist: &'static Allowlist,
    features: Vec<FeatureInfo>,
    events: VecDeque<Event>,
    headset_available: Option<bool>,
}

impl Session<HidTransport> {
    /// Refuses while G Hub's agent runs, then opens the HID device and runs
    /// discovery.
    pub fn open() -> Result<Self, DeviceError> {
        Self::open_with(guard::ghub_agent_running, HidTransport::open)
    }
}

impl<T: Transport> Session<T> {
    /// `guard` is checked before `opener` is called; a true guard means G Hub
    /// owns the device and nothing is opened.
    pub fn open_with(
        guard: fn() -> bool,
        opener: impl FnOnce() -> Result<T, DeviceError>,
    ) -> Result<Self, DeviceError> {
        if guard() {
            return Err(DeviceError::GHubRunning);
        }
        let allowlist = Allowlist::embedded().map_err(|e| DeviceError::Io(e.to_string()))?;
        let mut session = Self {
            transport: opener()?,
            allowlist,
            features: Vec::new(),
            events: VecDeque::new(),
            headset_available: None,
        };
        session.discover()?;
        Ok(session)
    }

    /// The table read at open: FeatureSet and every listed feature. Root is
    /// always index 0 and is not listed.
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

    pub fn take_events(&mut self) -> Vec<Event> {
        self.events.drain(..).collect()
    }

    /// The oldest queued event, or the next one to arrive within
    /// `timeout_ms`. Frames that are not modelled events are dropped.
    pub fn next_event(&mut self, timeout_ms: i32) -> Result<Option<Event>, DeviceError> {
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        let wait = Duration::from_millis(u64::try_from(timeout_ms).unwrap_or(0));
        let deadline = Instant::now() + wait;
        while let Some(remaining) = remaining_ms(deadline) {
            let Some(frame) = self.transport.read(remaining)? else {
                continue;
            };
            if let Ok(Decoded::Reply {
                flags: SINGLE_FRAME,
                msg,
            }) = decode(A50X_PREFIX, &frame)
                && let Some(event) = self.event_from(&msg)
            {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }

    fn discover(&mut self) -> Result<(), DeviceError> {
        let reply = self.send(FeatureId::ROOT, get_feature_request(FeatureId::FEATURE_SET))?;
        let feature_set = parse_get_feature(FeatureId::FEATURE_SET, &reply)
            .map_err(bad_reply)?
            .ok_or(DeviceError::FeatureMissing(FeatureId::FEATURE_SET))?;
        self.features.push(feature_set);

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
            self.features.extend(page);
            start = next;
        }
        Ok(())
    }

    fn index_of(&self, id: FeatureId) -> Result<u8, DeviceError> {
        if id == FeatureId::ROOT {
            return Ok(ROOT_INDEX);
        }
        self.features
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.index)
            .ok_or(DeviceError::FeatureMissing(id))
    }

    fn id_at(&self, index: u8) -> Option<FeatureId> {
        if index == ROOT_INDEX {
            return Some(FeatureId::ROOT);
        }
        self.features
            .iter()
            .find(|f| f.index == index)
            .map(|f| f.id)
    }

    /// The single gate every outgoing message passes: the message must target
    /// the discovered index of `id`, and (`id`, function) must be allowlisted.
    fn send(&mut self, id: FeatureId, msg: Message) -> Result<Vec<u8>, DeviceError> {
        let refused = DeviceError::NotAllowed {
            feature: id,
            function: msg.function,
        };
        if self.index_of(id).ok() != Some(msg.feature_index)
            || !self.allowlist.permits(id, msg.function)
        {
            return Err(refused);
        }
        let frame = encode(A50X_PREFIX, SINGLE_FRAME, &msg)
            .map_err(|e| DeviceError::Io(format!("cannot encode request: {e}")))?;
        self.transport.write(&frame)?;
        self.await_reply(&msg)
    }

    fn await_reply(&mut self, sent: &Message) -> Result<Vec<u8>, DeviceError> {
        let deadline = Instant::now() + REQUEST_TIMEOUT;
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
