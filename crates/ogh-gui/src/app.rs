use eframe::egui::{self, RichText, Ui};
use ogh_ipc::{Endpoint, Request};

use crate::Shared;
use crate::describe::headline;
use crate::link::DaemonLink;
use crate::pages::{self, HeldLevel};

const NOT_RUNNING: &str = "ogh-daemon is not running";

#[cfg(not(windows))]
const START_HINT: &str = "systemctl --user start ogh-daemon";
#[cfg(windows)]
const START_HINT: &str = "start ogh-daemon.exe, or run \"ogh autostart on\"";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Mixer,
    Equalizer,
    Microphone,
    Device,
    Profiles,
}

impl Page {
    const ALL: [Page; 5] = [
        Page::Mixer,
        Page::Equalizer,
        Page::Microphone,
        Page::Device,
        Page::Profiles,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Page::Mixer => "Mixer",
            Page::Equalizer => "Equalizer",
            Page::Microphone => "Microphone",
            Page::Device => "Device",
            Page::Profiles => "Profiles",
        }
    }
}

pub struct GuiState {
    pub shared: Shared,
    pub page: Page,
    pub(crate) held: HeldLevel,
}

impl GuiState {
    pub fn new(shared: Shared) -> GuiState {
        GuiState {
            shared,
            page: Page::Mixer,
            held: HeldLevel::default(),
        }
    }
}

/// Draws one frame and returns the requests the user made in it.
pub fn show(ui: &mut Ui, state: &mut GuiState) -> Vec<Request> {
    let mut requests = Vec::new();
    if let Some(reason) = &state.shared.daemon_down {
        egui::CentralPanel::default().show(ui, |ui| daemon_down(ui, reason));
        return requests;
    }
    egui::Panel::top("top_bar").show(ui, |ui| top_bar(ui, &mut state.shared));
    egui::Panel::left("navigation")
        .resizable(false)
        .show(ui, |ui| navigation(ui, &mut state.page));
    egui::CentralPanel::default().show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| pages::show(ui, state, &mut requests));
    });
    requests
}

fn daemon_down(ui: &mut Ui, reason: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.heading(NOT_RUNNING).on_hover_text(reason);
        ui.label(START_HINT);
    });
}

fn top_bar(ui: &mut Ui, shared: &mut Shared) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.strong(headline(shared));
        if let Some(status) = &shared.status {
            let profile = status.active_profile.as_deref().unwrap_or("none");
            ui.separator();
            ui.label(format!("Profile: {profile}"));
            if status.focus.available {
                let app = status.focus.app.as_deref().unwrap_or("none");
                ui.separator();
                ui.label(format!("Focused app: {app}"));
            }
        }
    });
    if let Some(error) = shared.status.as_ref().and_then(|s| s.config_error.as_ref()) {
        ui.colored_label(ui.visuals().warn_fg_color, error);
    }
    if let Some(error) = &shared.last_error {
        let dismissed = ui
            .horizontal(|ui| {
                ui.colored_label(ui.visuals().error_fg_color, error);
                ui.small_button("x").on_hover_text("Dismiss").clicked()
            })
            .inner;
        if dismissed {
            shared.last_error = None;
        }
    }
    ui.add_space(4.0);
}

fn navigation(ui: &mut Ui, current: &mut Page) {
    ui.add_space(8.0);
    for page in Page::ALL {
        ui.selectable_value(current, page, RichText::new(page.title()).size(15.0));
    }
}

/// The eframe application: forwards link events into the state and the user's requests
/// to the daemon.
pub struct GuiApp {
    state: GuiState,
    link: Option<DaemonLink>,
}

impl GuiApp {
    pub fn new(ctx: &egui::Context) -> GuiApp {
        let mut shared = Shared::default();
        let link = match Endpoint::for_current_user() {
            Ok(endpoint) => {
                let ctx = ctx.clone();
                Some(DaemonLink::start(endpoint, move || ctx.request_repaint()))
            }
            Err(error) => {
                shared.daemon_down = Some(error.to_string());
                None
            }
        };
        GuiApp {
            state: GuiState::new(shared),
            link,
        }
    }
}

impl eframe::App for GuiApp {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(link) = &self.link {
            for event in link.events() {
                self.state.shared.apply(event);
            }
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let requests = show(ui, &mut self.state);
        if let Some(link) = &self.link {
            for request in requests {
                link.send(request);
            }
        }
    }
}
