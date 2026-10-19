use ogh_ipc::{Connection, ProfileInfo, Status};

use crate::link::LinkEvent;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shared {
    pub status: Option<Status>,
    pub profiles: Vec<ProfileInfo>,
    pub daemon_down: Option<String>,
    pub last_error: Option<String>,
}

impl Shared {
    pub fn apply(&mut self, event: LinkEvent) {
        match event {
            LinkEvent::Status(status) => {
                self.status = Some(*status);
                self.daemon_down = None;
            }
            LinkEvent::Profiles(profiles) => self.profiles = profiles,
            LinkEvent::DaemonDown(message) => {
                self.status = None;
                self.daemon_down = Some(message);
            }
            LinkEvent::RequestFailed(message) => self.last_error = Some(message),
        }
    }

    pub fn can_write(&self) -> bool {
        self.status
            .as_ref()
            .is_some_and(|status| status.connection == Connection::Connected)
    }
}
