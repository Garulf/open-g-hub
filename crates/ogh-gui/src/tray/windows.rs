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

/// The items behind the menu the tray shows, and the action of each of its entries.
struct MenuState {
    items: Vec<TrayItem>,
    ids: HashMap<MenuId, TrayAction>,
}

impl MenuState {
    fn new(items: Vec<TrayItem>) -> (MenuState, Menu) {
        let (menu, ids) = build_menu(&items);
        (MenuState { items, ids }, menu)
    }

    /// A new menu to show, or None when `items` is what the tray already shows. `clicked`
    /// rebuilds regardless: muda flips a check entry when it is clicked, so after a click the
    /// menu on screen can differ from `items` even though `items` did not change.
    fn update(&mut self, items: Vec<TrayItem>, clicked: bool) -> Option<Menu> {
        if !clicked && items == self.items {
            return None;
        }
        let (state, menu) = MenuState::new(items);
        *self = state;
        Some(menu)
    }

    fn action(&self, id: &MenuId) -> Option<TrayAction> {
        self.ids.get(id).cloned()
    }
}

struct App {
    tray: TrayIcon,
    shared: Shared,
    menu: MenuState,
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
        let mut clicked = false;
        if let Some(link) = &self.link {
            for event in link.events() {
                self.shared.apply(event);
                changed = true;
            }
        }
        while let Ok(input) = self.inputs.try_recv() {
            let action = match input {
                Input::Menu(id) => {
                    clicked = true;
                    self.menu.action(&id)
                }
                Input::OpenWindow => Some(TrayAction::OpenWindow),
            };
            let Some(action) = action else { continue };
            let shared = &mut self.shared;
            let flow = perform(action, self.link.as_ref(), &mut self.launcher, |error| {
                shared.last_error = error
            });
            if flow.is_break() {
                return flow;
            }
            changed = true;
        }
        if changed || clicked {
            self.redraw(clicked);
        }
        ControlFlow::Continue(())
    }

    fn redraw(&mut self, clicked: bool) {
        if let Some(menu) = self.menu.update(tray_menu::menu(&self.shared), clicked) {
            self.tray.set_menu(Some(Box::new(menu)));
        }
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

fn tray_icon(shared: &Shared) -> Result<(TrayIcon, MenuState), String> {
    let (rgba, width, height) = icon::rgba(IconSize::Small);
    let icon = Icon::from_rgba(rgba, width, height)
        .map_err(|error| format!("could not build the tray icon: {error}"))?;
    let (state, menu) = MenuState::new(tray_menu::menu(shared));
    let tray = TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip(tray_menu::tooltip(shared))
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .map_err(|error| format!("could not register the tray: {error}"))?;
    Ok((tray, state))
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
        // SAFETY: `message` was filled in by the successful GetMessageW call above.
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

    let endpoint = Endpoint::for_current_user();
    let shared = Shared::starting(&endpoint);
    let (tray, menu) = tray_icon(&shared)?;
    // The thread has a message queue only once the tray window exists, so the link starts after it.
    let link = endpoint
        .ok()
        .map(|endpoint| DaemonLink::start(endpoint, move || wake(thread_id)));
    let mut app = App {
        tray,
        shared,
        menu,
        link,
        launcher,
        inputs,
    };
    message_loop(&mut app)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tray_icon::dpi::PhysicalPosition;
    use tray_icon::menu::{MenuId, MenuItemKind};
    use tray_icon::{MouseButton, MouseButtonState, Rect, TrayIconEvent, TrayIconId};

    use super::{Input, MenuState, build_menu, click_input, escape_mnemonic};
    use crate::Shared;
    use crate::tray::test_fixtures::connected_headset;
    use crate::tray_menu::{TrayAction, TrayItem, menu};

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
                    assert_eq!(entry.text(), escape_mnemonic(label));
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
                    assert_eq!(entry.text(), escape_mnemonic(label));
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
                    assert_eq!(entry.text(), escape_mnemonic(label));
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
                    assert_eq!(entry.text(), escape_mnemonic(label));
                    assert_eq!(entry.is_enabled(), *enabled);
                    assert_matches(items, &entry.items(), actions);
                }
                (item, _) => panic!("menu entry does not match {item:?}"),
            }
        }
    }

    #[test]
    fn labels_double_their_ampersands() {
        assert_eq!(escape_mnemonic("Music"), "Music");
        assert_eq!(escape_mnemonic("Rock & Roll"), "Rock && Roll");
        assert_eq!(escape_mnemonic("&&"), "&&&&");
    }

    #[test]
    fn build_menu_maps_every_action() {
        let items = menu(&connected_headset());

        let (built, actions) = build_menu(&items);

        assert_matches(&items, &built.items(), &actions);
        assert_eq!(actions.len(), count_actionable(&items));
    }

    #[test]
    fn an_unchanged_menu_is_not_rebuilt() {
        let items = menu(&connected_headset());
        let (mut state, built) = MenuState::new(items.clone());
        let ids = state.ids.clone();

        assert!(state.update(items, false).is_none());

        assert_eq!(state.ids, ids);
        assert_matches(&state.items, &built.items(), &state.ids);
    }

    #[test]
    fn a_changed_menu_is_rebuilt_with_its_own_ids() {
        let (mut state, _) = MenuState::new(menu(&Shared::default()));
        let items = menu(&connected_headset());

        let built = state
            .update(items.clone(), false)
            .expect("the menu changed");

        assert_eq!(state.items, items);
        assert_matches(&items, &built.items(), &state.ids);
        assert_eq!(state.ids.len(), count_actionable(&items));
    }

    #[test]
    fn a_click_rebuilds_an_unchanged_menu() {
        let items = menu(&connected_headset());
        let (mut state, _) = MenuState::new(items.clone());
        let old_ids = state.ids.clone();

        let built = state.update(items.clone(), true).expect("a click rebuilds");

        assert_matches(&items, &built.items(), &state.ids);
        assert!(state.ids.keys().all(|id| !old_ids.contains_key(id)));
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
