use ogh_ipc::{Request, Setting, StreamMixSetting};

use crate::Shared;
use crate::describe::{balance_label, headline};

#[derive(Debug, Clone, PartialEq)]
pub enum TrayAction {
    Request(Request),
    OpenWindow,
    Quit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrayItem {
    Label(String),
    Separator,
    Action {
        label: String,
        enabled: bool,
        action: TrayAction,
    },
    Check {
        label: String,
        checked: bool,
        enabled: bool,
        action: TrayAction,
    },
    Submenu {
        label: String,
        enabled: bool,
        items: Vec<TrayItem>,
    },
}

const SIDETONE_PERCENTS: [u8; 5] = [0, 25, 50, 75, 100];
const STREAM_PERCENTS: [u8; 5] = [0, 25, 50, 75, 100];
const BALANCE_VALUES: [u8; 7] = [0, 2, 4, 6, 8, 10, 12];

pub fn tooltip(shared: &Shared) -> String {
    format!("open-g-hub\n{}", headline(shared))
}

pub fn menu(shared: &Shared) -> Vec<TrayItem> {
    let mut items = vec![TrayItem::Label(headline(shared))];
    if let Some(error) = &shared.last_error {
        items.push(TrayItem::Label(format!("Error: {error}")));
    }
    items.push(TrayItem::Separator);
    items.push(profile_menu(shared));
    items.push(sidetone_menu(shared));
    items.push(balance_menu(shared));
    items.push(stream_level_menu(shared));
    items.push(auto_switch_item(shared));
    items.push(TrayItem::Separator);
    items.push(TrayItem::Action {
        label: "Open open-g-hub".to_string(),
        enabled: true,
        action: TrayAction::OpenWindow,
    });
    items.push(TrayItem::Action {
        label: "Quit tray".to_string(),
        enabled: true,
        action: TrayAction::Quit,
    });
    items
}

fn set(setting: Setting) -> TrayAction {
    TrayAction::Request(Request::Set { setting })
}

fn profile_menu(shared: &Shared) -> TrayItem {
    let enabled = shared.can_write();
    let active = shared
        .status
        .as_ref()
        .and_then(|status| status.active_profile.as_deref());
    let items = shared
        .profiles
        .iter()
        .map(|profile| TrayItem::Check {
            label: profile.name.clone(),
            checked: active == Some(profile.name.as_str()),
            enabled,
            action: TrayAction::Request(Request::ApplyProfile {
                name: profile.name.clone(),
            }),
        })
        .collect::<Vec<_>>();
    TrayItem::Submenu {
        label: "Profile".to_string(),
        enabled: enabled && !items.is_empty(),
        items,
    }
}

fn sidetone_menu(shared: &Shared) -> TrayItem {
    let enabled = shared.can_write();
    let current = shared
        .status
        .as_ref()
        .and_then(|status| status.settings.profile.sidetone_percent);
    let items = SIDETONE_PERCENTS
        .map(|percent| TrayItem::Check {
            label: format!("{percent}%"),
            checked: current == Some(percent),
            enabled,
            action: set(Setting::Sidetone { percent }),
        })
        .to_vec();
    TrayItem::Submenu {
        label: "Sidetone".to_string(),
        enabled,
        items,
    }
}

fn balance_menu(shared: &Shared) -> TrayItem {
    let enabled = shared.can_write();
    let current = shared
        .status
        .as_ref()
        .and_then(|status| status.settings.profile.balance);
    let items = BALANCE_VALUES
        .map(|value| TrayItem::Check {
            label: balance_label(value),
            checked: current == Some(value),
            enabled,
            action: set(Setting::Balance { value }),
        })
        .to_vec();
    TrayItem::Submenu {
        label: "Balance".to_string(),
        enabled,
        items,
    }
}

fn stream_level_menu(shared: &Shared) -> TrayItem {
    let mix: Option<StreamMixSetting> = shared
        .status
        .as_ref()
        .and_then(|status| status.settings.profile.stream_mix);
    let enabled = shared.can_write() && mix.is_some();
    let items = mix.map_or_else(Vec::new, |mix| {
        STREAM_PERCENTS
            .map(|percent| {
                let mut next = mix;
                next.output.level_percent = percent;
                TrayItem::Check {
                    label: format!("{percent}%"),
                    checked: mix.output.level_percent == percent,
                    enabled,
                    action: set(Setting::StreamMix(next)),
                }
            })
            .to_vec()
    });
    TrayItem::Submenu {
        label: "Stream level".to_string(),
        enabled,
        items,
    }
}

fn auto_switch_item(shared: &Shared) -> TrayItem {
    let current = shared
        .status
        .as_ref()
        .is_some_and(|status| status.auto_switch.enabled);
    TrayItem::Check {
        label: "Switch audio on dock".to_string(),
        checked: current,
        enabled: shared.status.is_some(),
        action: TrayAction::Request(Request::SetAutoSwitch { enabled: !current }),
    }
}
