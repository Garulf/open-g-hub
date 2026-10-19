use eframe::egui::accesskit::Role;
use eframe::egui::{Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use ogh_gui::icon::{self, IconSize};
use ogh_gui::link::LinkEvent;
use ogh_gui::{GuiState, Page, Shared, show};
use ogh_ipc::{
    AutoSwitchStatus, Channel, Connection, DeviceSettings, FocusStatus, Headset, ProfileSettings,
    Request, Setting, Status, StreamMixSetting,
};

#[cfg(not(windows))]
const HINT: &str = "systemctl --user start ogh-daemon";
#[cfg(windows)]
const HINT: &str = "start ogh-daemon.exe, or run \"ogh autostart on\"";

struct Window {
    gui: GuiState,
    sent: Vec<Request>,
}

fn harness(gui: GuiState) -> Harness<'static, Window> {
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

fn mix() -> StreamMixSetting {
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

fn status(connection: Connection) -> Status {
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

fn gui_with(status: Status) -> GuiState {
    let mut shared = Shared::default();
    shared.apply(LinkEvent::Status(Box::new(status)));
    GuiState::new(shared)
}

fn connected() -> GuiState {
    gui_with(status(Connection::Connected))
}

fn on_page(mut gui: GuiState, page: Page) -> GuiState {
    gui.page = page;
    gui
}

fn set(setting: Setting) -> Request {
    Request::Set { setting }
}

fn sidetone_value(harness: &Harness<'_, Window>) -> Option<f64> {
    harness
        .get_by_role_and_label(Role::Slider, "Sidetone")
        .accesskit_node()
        .numeric_value()
}

#[test]
fn daemon_down_shows_only_the_hint() {
    let mut shared = Shared::default();
    shared.apply(LinkEvent::DaemonDown("connection refused".to_string()));
    let mut harness = harness(GuiState::new(shared));
    harness.run();

    harness.get_by_label("ogh-daemon is not running");
    harness.get_by_label(HINT);
    assert!(
        harness
            .query_by_role_and_label(Role::Slider, "Sidetone")
            .is_none()
    );
    assert!(harness.query_by_label("Mixer").is_none());
}

#[test]
fn controls_are_disabled_when_not_connected() {
    let mut harness = harness(gui_with(status(Connection::GhubRunning)));
    harness.run();

    let slider = harness.get_by_role_and_label(Role::Slider, "Sidetone");
    assert!(slider.accesskit_node().is_disabled());
    slider.click();
    harness.run();

    assert!(harness.state().sent.is_empty());
}

#[test]
fn moving_sidetone_sends_a_set() {
    let mut harness = harness(connected());
    harness.run();

    harness
        .get_by_role_and_label(Role::Slider, "Sidetone")
        .focus();
    harness.run();
    harness.key_press(eframe::egui::Key::ArrowRight);
    harness.run();

    assert_eq!(
        harness.state().sent,
        vec![set(Setting::Sidetone { percent: 51 })]
    );
}

#[test]
fn stream_mute_sends_the_whole_mix() {
    let mut harness = harness(connected());
    harness.run();

    let mutes = harness.get_all_by_role_and_label(Role::CheckBox, "Mute");
    mutes.into_iter().nth(1).expect("a Game mute").click();
    harness.run();

    let mut expected = mix();
    expected.game.muted = true;
    assert_eq!(
        harness.state().sent,
        vec![set(Setting::StreamMix(expected))]
    );
}

#[test]
fn sleep_after_combo_sends_minutes() {
    let mut harness = harness(on_page(connected(), Page::Device));
    harness.run();

    harness
        .get_by_role_and_label(Role::ComboBox, "Sleep After")
        .click();
    harness.run();
    harness.get_by_label("15 min").click();
    harness.run();

    assert_eq!(
        harness.state().sent,
        vec![set(Setting::SleepAfter { minutes: 15 })]
    );
}

#[test]
fn auto_switch_checkbox_toggles() {
    let mut harness = harness(on_page(connected(), Page::Device));
    harness.run();

    harness.get_by_label("Off");
    harness
        .get_by_role_and_label(Role::CheckBox, "Switch audio devices on dock")
        .click();
    harness.run();

    assert_eq!(
        harness.state().sent,
        vec![Request::SetAutoSwitch { enabled: true }]
    );
}

#[test]
fn request_failure_shows_a_banner() {
    let mut gui = connected();
    gui.shared
        .apply(LinkEvent::RequestFailed("headset is off".to_string()));
    let mut harness = harness(gui);
    harness.run();

    harness.get_by_label_contains("headset is off");
    harness.get_by_role_and_label(Role::Button, "x").click();
    harness.run();

    assert_eq!(harness.state().gui.shared.last_error, None);
    assert!(harness.query_by_label_contains("headset is off").is_none());
}

#[test]
fn config_error_banner_is_shown() {
    let mut broken = status(Connection::Connected);
    broken.config_error = Some("config.toml line 3: expected a table".to_string());
    let mut harness = harness(gui_with(broken));
    harness.run();

    harness.get_by_label_contains("config.toml line 3: expected a table");
    assert!(harness.query_by_role_and_label(Role::Button, "x").is_none());
}

#[test]
fn held_slider_ignores_incoming_status() {
    let mut harness = harness(connected());
    harness.run();

    let rect = harness
        .get_by_role_and_label(Role::Slider, "Sidetone")
        .rect();
    let grip = Pos2::new(rect.left() + rect.width() * 0.8, rect.center().y);
    harness.hover_at(grip);
    harness.drag_at(grip);
    harness.run();
    let held = sidetone_value(&harness).expect("a slider value");
    assert!(held != 50.0 && held != 10.0, "held at {held}");

    let mut quieter = status(Connection::Connected);
    quieter.settings.profile.sidetone_percent = Some(10);
    harness.state_mut().gui.shared.status = Some(quieter);
    harness.run();
    assert_eq!(sidetone_value(&harness), Some(held));

    harness.drop_at(grip);
    harness.run();
    assert_eq!(sidetone_value(&harness), Some(10.0));
    assert_eq!(
        harness.state().sent,
        vec![set(Setting::Sidetone {
            percent: held as u8
        })]
    );
}

#[test]
fn icons_decode_at_their_sizes() {
    for (size, side) in [
        (IconSize::Small, 32),
        (IconSize::Medium, 64),
        (IconSize::Large, 256),
    ] {
        let (rgba, width, height) = icon::rgba(size);
        assert_eq!((width, height), (side, side));
        assert_eq!(rgba.len(), (side * side * 4) as usize);
    }
}
