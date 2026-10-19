use std::collections::HashMap;
use std::io;
use std::ops::ControlFlow;

use crossbeam_channel::{Receiver, Sender, unbounded};
use ogh_ipc::Endpoint;
use tray_icon::menu::{
    CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostThreadMessageW, TranslateMessage, WM_APP,
};

use super::perform;
use crate::Shared;
use crate::icon::{self, IconSize};
use crate::launcher::Launcher;
use crate::link::DaemonLink;
use crate::tray_menu::{self, TrayAction, TrayItem};

const WM_WAKE: u32 = WM_APP + 1;

#[derive(Debug, PartialEq)]
enum Input {
    Menu(MenuId),
    OpenWindow,
}

struct App {
    tray: TrayIcon,
    shared: Shared,
    ids: HashMap<MenuId, TrayAction>,
    link: Option<DaemonLink>,
    launcher: Launcher,
    inputs: Receiver<Input>,
}

impl App {
    /// Applies everything that arrived since the last call and redraws the tray if it changed.
    /// Runs after every dispatched message, so a wake that a modal menu loop swallowed is
    /// picked up by the next message.
    fn handle(&mut self) -> ControlFlow<()> {
        let mut changed = false;
        if let Some(link) = &self.link {
            for event in link.events() {
                self.shared.apply(event);
                changed = true;
            }
        }
        while let Ok(input) = self.inputs.try_recv() {
            let action = match input {
                Input::Menu(id) => self.ids.get(&id).cloned(),
                Input::OpenWindow => Some(TrayAction::OpenWindow),
            };
            let Some(action) = action else { continue };
            let shared = &mut self.shared;
            let flow = perform(action, self.link.as_ref(), &mut self.launcher, |error| {
                shared.last_error = Some(error)
            });
            if flow.is_break() {
                return flow;
            }
            changed = true;
        }
        if changed {
            self.redraw();
        }
        ControlFlow::Continue(())
    }

    fn redraw(&mut self) {
        let (menu, ids) = build_menu(&tray_menu::menu(&self.shared));
        self.ids = ids;
        self.tray.set_menu(Some(Box::new(menu)));
        let _ = self
            .tray
            .set_tooltip(Some(tray_menu::tooltip(&self.shared)));
    }
}

fn escape_mnemonic(label: &str) -> String {
    label.replace('&', "&&")
}

fn build_item(item: &TrayItem, ids: &mut HashMap<MenuId, TrayAction>) -> Box<dyn IsMenuItem> {
    match item {
        TrayItem::Label(label) => Box::new(MenuItem::new(escape_mnemonic(label), false, None)),
        TrayItem::Separator => Box::new(PredefinedMenuItem::separator()),
        TrayItem::Action {
            label,
            enabled,
            action,
        } => {
            let entry = MenuItem::new(escape_mnemonic(label), *enabled, None);
            ids.insert(entry.id().clone(), action.clone());
            Box::new(entry)
        }
        TrayItem::Check {
            label,
            checked,
            enabled,
            action,
        } => {
            let entry = CheckMenuItem::new(escape_mnemonic(label), *enabled, *checked, None);
            ids.insert(entry.id().clone(), action.clone());
            Box::new(entry)
        }
        TrayItem::Submenu {
            label,
            enabled,
            items,
        } => {
            let submenu = Submenu::new(escape_mnemonic(label), *enabled);
            for child in items {
                let _ = submenu.append(build_item(child, ids).as_ref());
            }
            Box::new(submenu)
        }
    }
}

/// An item the OS refuses to add is left out; the next redraw builds the menu again.
fn build_menu(items: &[TrayItem]) -> (Menu, HashMap<MenuId, TrayAction>) {
    let menu = Menu::new();
    let mut ids = HashMap::new();
    for item in items {
        let _ = menu.append(build_item(item, &mut ids).as_ref());
    }
    (menu, ids)
}

fn click_input(event: &TrayIconEvent) -> Option<Input> {
    match event {
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } => Some(Input::OpenWindow),
        _ => None,
    }
}

fn wake(thread_id: u32) {
    // SAFETY: posting a thread message has no memory preconditions; it fails harmlessly when
    // the thread has no queue yet or has exited.
    let _ = unsafe { PostThreadMessageW(thread_id, WM_WAKE, 0, 0) };
}

fn forward_events(inputs: Sender<Input>, thread_id: u32) {
    let menu_inputs = inputs.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = menu_inputs.send(Input::Menu(event.id().clone()));
        wake(thread_id);
    }));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let Some(input) = click_input(&event) {
            let _ = inputs.send(input);
            wake(thread_id);
        }
    }));
}

fn tray_icon(shared: &Shared) -> Result<TrayIcon, String> {
    let (rgba, width, height) = icon::rgba(IconSize::Small);
    let icon = Icon::from_rgba(rgba, width, height)
        .map_err(|error| format!("could not build the tray icon: {error}"))?;
    let (menu, _) = build_menu(&tray_menu::menu(shared));
    TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip(tray_menu::tooltip(shared))
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .map_err(|error| format!("could not register the tray: {error}"))
}

fn message_loop(app: &mut App) -> Result<(), String> {
    loop {
        // SAFETY: MSG is plain data for which all-zero is valid, and the pointers passed to
        // the message functions refer to it for the duration of each call.
        let mut message: MSG = unsafe { std::mem::zeroed() };
        match unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } {
            0 => return Err("the tray message loop received a quit message".to_string()),
            -1 => {
                return Err(format!(
                    "the tray message loop failed: {}",
                    io::Error::last_os_error()
                ));
            }
            _ => {}
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        if app.handle().is_break() {
            return Ok(());
        }
    }
}

pub fn run() -> Result<(), String> {
    let launcher = Launcher::for_current_exe()?;
    // SAFETY: reads the calling thread's id.
    let thread_id = unsafe { GetCurrentThreadId() };
    let (sender, inputs) = unbounded();
    forward_events(sender, thread_id);

    let mut shared = Shared::default();
    let endpoint = match Endpoint::for_current_user() {
        Ok(endpoint) => Some(endpoint),
        Err(error) => {
            shared.daemon_down = Some(error.to_string());
            None
        }
    };
    let tray = tray_icon(&shared)?;
    // The thread has a message queue only once the tray window exists, so the link starts after it.
    let link = endpoint.map(|endpoint| DaemonLink::start(endpoint, move || wake(thread_id)));
    let mut app = App {
        tray,
        shared,
        ids: HashMap::new(),
        link,
        launcher,
        inputs,
    };
    app.redraw();

    message_loop(&mut app)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use ogh_ipc::{
        AutoSwitchStatus, Battery, Channel, Charge, Connection, DeviceSettings, FocusStatus,
        Headset, ProfileInfo, ProfileSettings, Status, StreamMixSetting,
    };
    use tray_icon::dpi::PhysicalPosition;
    use tray_icon::menu::{MenuId, MenuItemKind};
    use tray_icon::{MouseButton, MouseButtonState, Rect, TrayIconEvent, TrayIconId};

    use super::{Input, build_menu, click_input};
    use crate::Shared;
    use crate::link::LinkEvent;
    use crate::tray_menu::{TrayAction, TrayItem, menu};

    fn channel(level_percent: u8) -> Channel {
        Channel {
            level_percent,
            muted: false,
        }
    }

    fn connected_headset() -> Shared {
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
        shared.apply(LinkEvent::Profiles(vec![profile("Game"), profile("Music")]));
        shared
    }

    fn count_actionable(items: &[TrayItem]) -> usize {
        items
            .iter()
            .map(|item| match item {
                TrayItem::Action { .. } | TrayItem::Check { .. } => 1,
                TrayItem::Submenu { items, .. } => count_actionable(items),
                TrayItem::Label(_) | TrayItem::Separator => 0,
            })
            .sum()
    }

    fn assert_matches(
        items: &[TrayItem],
        kinds: &[MenuItemKind],
        actions: &HashMap<MenuId, TrayAction>,
    ) {
        assert_eq!(items.len(), kinds.len());
        for (item, kind) in items.iter().zip(kinds) {
            match (item, kind) {
                (TrayItem::Label(label), MenuItemKind::MenuItem(entry)) => {
                    assert_eq!(&entry.text(), label);
                    assert!(!entry.is_enabled());
                    assert!(!actions.contains_key(entry.id()));
                }
                (TrayItem::Separator, MenuItemKind::Predefined(_)) => {}
                (
                    TrayItem::Action {
                        label,
                        enabled,
                        action,
                    },
                    MenuItemKind::MenuItem(entry),
                ) => {
                    assert_eq!(&entry.text(), label);
                    assert_eq!(entry.is_enabled(), *enabled);
                    assert_eq!(actions.get(entry.id()), Some(action));
                }
                (
                    TrayItem::Check {
                        label,
                        checked,
                        enabled,
                        action,
                    },
                    MenuItemKind::Check(entry),
                ) => {
                    assert_eq!(&entry.text(), label);
                    assert_eq!(entry.is_checked(), *checked);
                    assert_eq!(entry.is_enabled(), *enabled);
                    assert_eq!(actions.get(entry.id()), Some(action));
                }
                (
                    TrayItem::Submenu {
                        label,
                        enabled,
                        items,
                    },
                    MenuItemKind::Submenu(entry),
                ) => {
                    assert_eq!(&entry.text(), label);
                    assert_eq!(entry.is_enabled(), *enabled);
                    assert_matches(items, &entry.items(), actions);
                }
                (item, _) => panic!("menu entry does not match {item:?}"),
            }
        }
    }

    #[test]
    fn build_menu_maps_every_action() {
        let items = menu(&connected_headset());

        let (built, actions) = build_menu(&items);

        assert_matches(&items, &built.items(), &actions);
        assert_eq!(actions.len(), count_actionable(&items));
    }

    #[test]
    fn only_a_left_button_release_opens_the_window() {
        let click = |button, button_state| TrayIconEvent::Click {
            id: TrayIconId::new("tray"),
            position: PhysicalPosition::new(0.0, 0.0),
            rect: Rect::default(),
            button,
            button_state,
        };

        assert_eq!(
            click_input(&click(MouseButton::Left, MouseButtonState::Up)),
            Some(Input::OpenWindow)
        );
        assert_eq!(
            click_input(&click(MouseButton::Left, MouseButtonState::Down)),
            None
        );
        assert_eq!(
            click_input(&click(MouseButton::Right, MouseButtonState::Up)),
            None
        );
    }
}
