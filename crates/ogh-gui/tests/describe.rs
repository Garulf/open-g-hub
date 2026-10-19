use ogh_gui::Shared;
use ogh_gui::describe::{
    auto_switch_text, balance_label, gate_label, headline, sleep_label, tones_label,
};
use ogh_gui::link::LinkEvent;
use ogh_ipc::{
    AutoSwitchStatus, Battery, Charge, Connection, FocusStatus, Gate, Headset, ProfileInfo,
    ProfileSettings, Status,
};

fn status(connection: Connection, headset: Headset, battery: Option<Battery>) -> Status {
    Status {
        connection,
        headset,
        battery,
        firmware: None,
        settings: Default::default(),
        active_profile: None,
        focus: FocusStatus {
            available: true,
            app: None,
        },
        auto_switch: AutoSwitchStatus {
            enabled: false,
            active: false,
            error: None,
        },
        config_error: None,
    }
}

fn shared_with(status: Status) -> Shared {
    let mut shared = Shared::default();
    shared.apply(LinkEvent::Status(Box::new(status)));
    shared
}

fn battery(percent: u8, charge: Charge) -> Option<Battery> {
    Some(Battery { percent, charge })
}

fn profile(name: &str) -> ProfileInfo {
    ProfileInfo {
        name: name.to_string(),
        apps: Vec::new(),
        is_default: false,
        settings: ProfileSettings::default(),
    }
}

#[test]
fn headline_when_the_daemon_is_down() {
    let mut shared = Shared::default();
    shared.apply(LinkEvent::DaemonDown("refused".to_string()));
    assert_eq!(headline(&shared), "ogh-daemon is not running");
}

#[test]
fn headline_before_any_event() {
    assert_eq!(headline(&Shared::default()), "ogh-daemon is not running");
}

#[test]
fn headline_when_ghub_is_running() {
    let shared = shared_with(status(Connection::GhubRunning, Headset::Unknown, None));
    assert_eq!(
        headline(&shared),
        "G Hub is running; quit it to use open-g-hub"
    );
}

#[test]
fn headline_when_the_headset_is_not_found() {
    let shared = shared_with(status(Connection::NotFound, Headset::Unknown, None));
    assert_eq!(headline(&shared), "A50 X not found");
}

#[test]
fn headline_when_access_is_denied() {
    let denied = Connection::AccessDenied {
        message: "permission denied".to_string(),
    };
    let shared = shared_with(status(denied, Headset::Unknown, None));
    assert_eq!(
        headline(&shared),
        "No access to the A50 X: permission denied"
    );
}

#[test]
fn headline_on_a_device_error() {
    let error = Connection::Error {
        message: "read failed".to_string(),
    };
    let shared = shared_with(status(error, Headset::Unknown, None));
    assert_eq!(headline(&shared), "A50 X error: read failed");
}

#[test]
fn headline_in_use_and_charging() {
    let shared = shared_with(status(
        Connection::Connected,
        Headset::InUse,
        battery(80, Charge::Charging),
    ));
    assert_eq!(headline(&shared), "A50 X: in use, 80% (charging)");
}

#[test]
fn headline_docked_and_full() {
    let shared = shared_with(status(
        Connection::Connected,
        Headset::Docked,
        battery(100, Charge::Full),
    ));
    assert_eq!(headline(&shared), "A50 X: docked, 100% (full)");
}

#[test]
fn headline_off_without_a_battery() {
    let shared = shared_with(status(Connection::Connected, Headset::Off, None));
    assert_eq!(headline(&shared), "A50 X: off");
}

#[test]
fn headline_unknown_headset() {
    let shared = shared_with(status(Connection::Connected, Headset::Unknown, None));
    assert_eq!(headline(&shared), "A50 X: unknown");
}

#[test]
fn headline_discharging_and_other_charge() {
    let shared = shared_with(status(
        Connection::Connected,
        Headset::InUse,
        battery(55, Charge::Discharging),
    ));
    assert_eq!(headline(&shared), "A50 X: in use, 55% (discharging)");
    let shared = shared_with(status(
        Connection::Connected,
        Headset::InUse,
        battery(55, Charge::Other(7)),
    ));
    assert_eq!(headline(&shared), "A50 X: in use, 55% (status 7)");
}

#[test]
fn shared_apply_status_clears_daemon_down() {
    let mut shared = Shared::default();
    shared.apply(LinkEvent::DaemonDown("gone".to_string()));
    shared.apply(LinkEvent::Status(Box::new(status(
        Connection::Connected,
        Headset::InUse,
        None,
    ))));
    assert!(shared.status.is_some());
    assert_eq!(shared.daemon_down, None);
}

#[test]
fn shared_apply_daemon_down_clears_status() {
    let mut shared = shared_with(status(Connection::Connected, Headset::InUse, None));
    shared.apply(LinkEvent::DaemonDown("gone".to_string()));
    assert_eq!(shared.status, None);
    assert_eq!(shared.daemon_down.as_deref(), Some("gone"));
    assert!(!shared.can_write());
}

#[test]
fn shared_apply_request_failed_sets_the_last_error() {
    let mut shared = Shared::default();
    shared.apply(LinkEvent::RequestFailed("no such profile".to_string()));
    assert_eq!(shared.last_error.as_deref(), Some("no such profile"));
}

#[test]
fn shared_apply_status_and_profiles_keep_the_last_error() {
    let mut shared = Shared::default();
    shared.apply(LinkEvent::RequestFailed("boom".to_string()));
    shared.apply(LinkEvent::Status(Box::new(status(
        Connection::Connected,
        Headset::InUse,
        None,
    ))));
    shared.apply(LinkEvent::Profiles(vec![profile("Music")]));
    assert_eq!(shared.last_error.as_deref(), Some("boom"));
    assert_eq!(shared.profiles, vec![profile("Music")]);
}

#[test]
fn can_write_only_when_connected() {
    assert!(!Shared::default().can_write());
    assert!(shared_with(status(Connection::Connected, Headset::Off, None)).can_write());
    assert!(!shared_with(status(Connection::GhubRunning, Headset::Unknown, None)).can_write());
}

#[test]
fn sleep_labels() {
    assert_eq!(sleep_label(0), "Never");
    assert_eq!(sleep_label(15), "15 min");
}

#[test]
fn tones_labels() {
    assert_eq!(tones_label(0), "0");
    assert_eq!(tones_label(1), "1 (All)");
    assert_eq!(tones_label(2), "2");
}

#[test]
fn balance_labels() {
    assert_eq!(balance_label(6), "6 (centre)");
    assert_eq!(balance_label(4), "4");
}

#[test]
fn gate_labels() {
    assert_eq!(gate_label(Gate::Off), "Off");
    assert_eq!(gate_label(Gate::Home), "Home");
    assert_eq!(gate_label(Gate::Night), "Night");
    assert_eq!(gate_label(Gate::Tournament), "Tournament");
}

#[test]
fn auto_switch_texts() {
    let off = AutoSwitchStatus {
        enabled: false,
        active: false,
        error: None,
    };
    assert_eq!(auto_switch_text(&off), "Off");
    let on_active = AutoSwitchStatus {
        enabled: true,
        active: true,
        error: None,
    };
    assert_eq!(auto_switch_text(&on_active), "On, active");
    let failing = AutoSwitchStatus {
        enabled: true,
        active: false,
        error: Some("no sink".to_string()),
    };
    assert_eq!(auto_switch_text(&failing), "On, error: no sink");
}
