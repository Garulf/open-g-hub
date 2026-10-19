use crossbeam_channel::Sender;
use ogh_device::DeviceError;
use ogh_ipc::Response;

use super::{Controller, device_error, respond};

pub(super) struct DeferredSave {
    name: String,
    reply: Sender<Response>,
}

impl Controller {
    pub(super) fn finish_write(&mut self, id: u64, result: Result<(), DeviceError>) {
        let Some(pending) = self.pending.remove(&id) else {
            return;
        };
        let response = match result {
            Ok(()) => {
                for write in &pending.writes {
                    self.snapshot.record_write(write);
                }
                Response::Done
            }
            Err(error) => Response::Error(device_error(&error)),
        };
        if let Some(reply) = pending.reply {
            let _ = reply.send(response);
        }
        self.apply_if_wanted();
        if self.pending.is_empty() {
            self.run_deferred_saves();
        }
    }

    pub(super) fn run_deferred_saves(&mut self) {
        for save in std::mem::take(&mut self.deferred_saves) {
            let response = self.save_profile(&save.name);
            respond(&save.reply, response);
        }
    }

    pub(super) fn apply_if_wanted(&mut self) {
        if self.apply_wanted && self.pending.is_empty() {
            self.apply_wanted = false;
            self.apply_active();
        }
    }

    pub(super) fn save_profile_when_idle(&mut self, name: String, reply: Sender<Response>) {
        if self.is_connected() && !self.pending.is_empty() {
            self.deferred_saves.push(DeferredSave { name, reply });
        } else {
            respond(&reply, self.save_profile(&name));
        }
    }
}
