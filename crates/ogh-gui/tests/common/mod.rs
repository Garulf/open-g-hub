#![allow(dead_code)]

use eframe::egui::vec2;
use egui_kittest::Harness;
use ogh_gui::link::LinkEvent;
use ogh_gui::{GuiState, Page, Shared, show};
use ogh_ipc::{
    AutoSwitchStatus, Channel, Connection, DeviceSettings, FocusStatus, Headset, ProfileSettings,
    Request, Setting, Status, StreamMixSetting,
};

pub struct Window {
    pub gui: GuiState,
    pub sent: Vec<Request>,
}

pub fn harness(gui: GuiState) -> Harness<'static, Window> {
    Harness::builder()
        .with_size(vec2(820.0, 560.0))
        .build_ui_state(
            |ui, window: &mut Window| {
                let requests = show(ui, &mut window.gui);
                window.sent.extend(requests);
            },
            Window {
                gui,
                sent: Vec::new(),
            },
        )
}

pub fn mix() -> StreamMixSetting {
    StreamMixSetting {
        output: Channel {
            level_percent: 80,
            muted: false,
        },
        game: Channel {
            level_percent: 70,
            muted: false,
        },
        voice: Channel {
            level_percent: 60,
            muted: false,
        },
    }
}

pub fn status(connection: Connection) -> Status {
    Status {
        connection,
        headset: Headset::InUse,
        battery: None,
        firmware: None,
        settings: DeviceSettings {
            profile: ProfileSettings {
                sidetone_percent: Some(50),
                balance: Some(6),
                stream_mix: Some(mix()),
                ..ProfileSettings::default()
            },
            volume_percent: Some(40),
            led_brightness_percent: Some(30),
            sleep_after_minutes: Some(0),
            tones: Some(1),
        },
        active_profile: Some("Default".to_string()),
        focus: FocusStatus {
            available: true,
            app: Some("firefox".to_string()),
        },
        auto_switch: AutoSwitchStatus {
            enabled: false,
            active: false,
            error: None,
        },
        config_error: None,
    }
}

pub fn gui_with(status: Status) -> GuiState {
    let mut shared = Shared::default();
    shared.apply(LinkEvent::Status(Box::new(status)));
    GuiState::new(shared)
}

pub fn connected() -> GuiState {
    gui_with(status(Connection::Connected))
}

pub fn on_page(mut gui: GuiState, page: Page) -> GuiState {
    gui.page = page;
    gui
}

pub fn set(setting: Setting) -> Request {
    Request::Set { setting }
}
