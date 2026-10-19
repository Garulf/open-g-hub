use ogh_gui::Shared;
use ogh_gui::link::LinkEvent;
use ogh_gui::tray_menu::{TrayAction, TrayItem, menu, tooltip};
use ogh_ipc::{
    AutoSwitchStatus, Battery, Channel, Charge, Connection, DeviceSettings, FocusStatus, Headset,
    ProfileInfo, ProfileSettings, Request, Setting, Status, StreamMixSetting,
};

fn mix() -> StreamMixSetting {
    StreamMixSetting {
        output: Channel {
            level_percent: 50,
            muted: true,
        },
        game: Channel {
            level_percent: 70,
            muted: false,
        },
        voice: Channel {
            level_percent: 30,
            muted: true,
        },
    }
}

fn status(connection: Connection) -> Status {
    Status {
        connection,
        headset: Headset::InUse,
        battery: Some(Battery {
            percent: 80,
            charge: Charge::Charging,
        }),
        firmware: None,
        settings: DeviceSettings {
            profile: ProfileSettings {
                sidetone_percent: Some(50),
                balance: Some(6),
                stream_mix: Some(mix()),
                ..Default::default()
            },
            ..Default::default()
        },
        active_profile: Some("Music".to_string()),
        focus: FocusStatus {
            available: true,
            app: None,
        },
        auto_switch: AutoSwitchStatus {
            enabled: true,
            active: false,
            error: None,
        },
        config_error: None,
    }
}

fn profile(name: &str) -> ProfileInfo {
    ProfileInfo {
        name: name.to_string(),
        apps: Vec::new(),
        is_default: false,
        settings: ProfileSettings::default(),
    }
}

fn shared(status: Status) -> Shared {
    let mut shared = Shared::default();
    shared.apply(LinkEvent::Status(Box::new(status)));
    shared.apply(LinkEvent::Profiles(vec![profile("Game"), profile("Music")]));
    shared
}

fn submenu<'a>(items: &'a [TrayItem], name: &str) -> (&'a bool, &'a Vec<TrayItem>) {
    items
        .iter()
        .find_map(|item| match item {
            TrayItem::Submenu {
                label,
                enabled,
                items,
            } if label == name => Some((enabled, items)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no submenu {name}"))
}

fn check(label: &str, checked: bool, enabled: bool, action: TrayAction) -> TrayItem {
    TrayItem::Check {
        label: label.to_string(),
        checked,
        enabled,
        action,
    }
}

fn set(setting: Setting) -> TrayAction {
    TrayAction::Request(Request::Set { setting })
}

#[test]
fn menu_for_a_connected_headset() {
    let items = menu(&shared(status(Connection::Connected)));

    let expected = vec![
        TrayItem::Label("A50 X: in use, 80% (charging)".to_string()),
        TrayItem::Separator,
        TrayItem::Submenu {
            label: "Profile".to_string(),
            enabled: true,
            items: vec![
                check(
                    "Game",
                    false,
                    true,
                    TrayAction::Request(Request::ApplyProfile {
                        name: "Game".to_string(),
                    }),
                ),
                check(
                    "Music",
                    true,
                    true,
                    TrayAction::Request(Request::ApplyProfile {
                        name: "Music".to_string(),
                    }),
                ),
            ],
        },
        TrayItem::Submenu {
            label: "Sidetone".to_string(),
            enabled: true,
            items: [0u8, 25, 50, 75, 100]
                .map(|percent| {
                    check(
                        &format!("{percent}%"),
                        percent == 50,
                        true,
                        set(Setting::Sidetone { percent }),
                    )
                })
                .to_vec(),
        },
        TrayItem::Submenu {
            label: "Balance".to_string(),
            enabled: true,
            items: [
                (0u8, "0"),
                (2, "2"),
                (4, "4"),
                (6, "6 (centre)"),
                (8, "8"),
                (10, "10"),
                (12, "12"),
            ]
            .map(|(value, label)| check(label, value == 6, true, set(Setting::Balance { value })))
            .to_vec(),
        },
        TrayItem::Submenu {
            label: "Stream level".to_string(),
            enabled: true,
            items: [0u8, 25, 50, 75, 100]
                .map(|percent| {
                    let mut stream = mix();
                    stream.output.level_percent = percent;
                    check(
                        &format!("{percent}%"),
                        percent == 50,
                        true,
                        set(Setting::StreamMix(stream)),
                    )
                })
                .to_vec(),
        },
        check(
            "Switch audio on dock",
            true,
            true,
            TrayAction::Request(Request::SetAutoSwitch { enabled: false }),
        ),
        TrayItem::Separator,
        TrayItem::Action {
            label: "Open open-g-hub".to_string(),
            enabled: true,
            action: TrayAction::OpenWindow,
        },
        TrayItem::Action {
            label: "Quit tray".to_string(),
            enabled: true,
            action: TrayAction::Quit,
        },
    ];
    assert_eq!(items, expected);
}

#[test]
fn menu_disables_device_controls_when_not_connected() {
    let items = menu(&shared(status(Connection::GhubRunning)));

    for name in ["Profile", "Sidetone", "Balance", "Stream level"] {
        assert!(!submenu(&items, name).0, "{name} should be disabled");
    }
    let open = items.iter().any(|item| {
        matches!(
            item,
            TrayItem::Action {
                enabled: true,
                action: TrayAction::OpenWindow,
                ..
            }
        )
    });
    let quit = items.iter().any(|item| {
        matches!(
            item,
            TrayItem::Action {
                enabled: true,
                action: TrayAction::Quit,
                ..
            }
        )
    });
    assert!(open && quit);
}

#[test]
fn menu_shows_the_last_error() {
    let mut shared = shared(status(Connection::Connected));
    shared.apply(LinkEvent::RequestFailed("no such profile".to_string()));
    let items = menu(&shared);
    assert_eq!(
        items[1],
        TrayItem::Label("Error: no such profile".to_string())
    );
    assert_eq!(items[2], TrayItem::Separator);
}

#[test]
fn stream_level_keeps_game_and_voice() {
    let items = menu(&shared(status(Connection::Connected)));
    let (_, levels) = submenu(&items, "Stream level");
    let TrayItem::Check { action, .. } = &levels[3] else {
        panic!("expected a check");
    };
    let TrayAction::Request(Request::Set {
        setting: Setting::StreamMix(stream),
    }) = action
    else {
        panic!("expected a stream mix request");
    };
    assert_eq!(stream.output.level_percent, 75);
    assert!(stream.output.muted);
    assert_eq!(stream.game, mix().game);
    assert_eq!(stream.voice, mix().voice);
}

#[test]
fn stream_level_is_disabled_without_a_known_mix() {
    let mut status = status(Connection::Connected);
    status.settings.profile.stream_mix = None;
    let items = menu(&shared(status));
    assert!(!submenu(&items, "Stream level").0);
    assert!(*submenu(&items, "Sidetone").0);
}

#[test]
fn daemon_down_menu() {
    let mut shared = Shared::default();
    shared.apply(LinkEvent::DaemonDown("refused".to_string()));
    let items = menu(&shared);

    assert_eq!(
        items[0],
        TrayItem::Label("ogh-daemon is not running".to_string())
    );
    for name in ["Profile", "Sidetone", "Balance", "Stream level"] {
        assert!(!submenu(&items, name).0);
    }
    assert!(items.iter().any(|item| matches!(
        item,
        TrayItem::Check {
            enabled: false,
            checked: false,
            ..
        }
    )));
    assert!(items.iter().any(|item| matches!(
        item,
        TrayItem::Action {
            enabled: true,
            action: TrayAction::OpenWindow,
            ..
        }
    )));
    assert!(items.iter().any(|item| matches!(
        item,
        TrayItem::Action {
            enabled: true,
            action: TrayAction::Quit,
            ..
        }
    )));
}

#[test]
fn tooltip_names_the_app_and_the_headline() {
    let shared = shared(status(Connection::Connected));
    assert_eq!(
        tooltip(&shared),
        "open-g-hub\nA50 X: in use, 80% (charging)"
    );
}
