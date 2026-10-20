use ogh_ipc::{
    AutoSwitchStatus, Battery, Channel, Charge, Connection, DeviceSettings, FocusStatus, Headset,
    ProfileInfo, ProfileSettings, Status, StreamMixSetting,
};

use crate::Shared;
use crate::link::LinkEvent;

fn channel(level_percent: u8) -> Channel {
    Channel {
        level_percent,
        muted: false,
    }
}

pub fn connected_headset() -> Shared {
    let status = Status {
        connection: Connection::Connected,
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
                stream_mix: Some(StreamMixSetting {
                    output: channel(50),
                    game: channel(70),
                    voice: channel(30),
                }),
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
    };
    let profile = |name: &str| ProfileInfo {
        name: name.to_string(),
        apps: Vec::new(),
        is_default: false,
        settings: ProfileSettings::default(),
    };
    let mut shared = Shared::default();
    shared.apply(LinkEvent::Status(Box::new(status)));
    shared.apply(LinkEvent::Profiles(vec![
        profile("Rock & Roll"),
        profile("Music"),
    ]));
    shared
}
