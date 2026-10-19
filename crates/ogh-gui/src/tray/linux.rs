use std::sync::{Arc, OnceLock};

use crossbeam_channel::{Receiver, Sender, unbounded};
use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{CheckmarkItem, MenuItem, StandardItem, SubMenu};
use ksni::{Category, Icon, ToolTip, Tray};
use ogh_ipc::Endpoint;

use crate::Shared;
use crate::icon::{self, IconSize};
use crate::launcher::Launcher;
use crate::link::DaemonLink;
use crate::tray_menu::{self, TrayAction, TrayItem};

struct TrayModel {
    shared: Shared,
    link: Option<Arc<DaemonLink>>,
    actions: Sender<TrayAction>,
}

impl TrayModel {
    fn drain(&mut self) {
        if let Some(link) = &self.link {
            for event in link.events() {
                self.shared.apply(event);
            }
        }
    }
}

impl Tray for TrayModel {
    fn id(&self) -> String {
        "open-g-hub".to_string()
    }

    fn title(&self) -> String {
        "open-g-hub".to_string()
    }

    fn category(&self) -> Category {
        Category::Hardware
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        [IconSize::Small, IconSize::Medium]
            .map(|size| {
                let (rgba, width, height) = icon::rgba(size);
                Icon {
                    width: width as i32,
                    height: height as i32,
                    data: argb(&rgba),
                }
            })
            .to_vec()
    }

    fn tool_tip(&self) -> ToolTip {
        let text = tray_menu::tooltip(&self.shared);
        let (title, description) = text.split_once('\n').unwrap_or((text.as_str(), ""));
        ToolTip {
            title: title.to_string(),
            description: description.to_string(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        to_ksni(&tray_menu::menu(&self.shared), &self.actions)
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.actions.send(TrayAction::OpenWindow);
    }
}

/// SNI pixmaps are ARGB32 in network byte order; the icon decoder yields RGBA.
fn argb(rgba: &[u8]) -> Vec<u8> {
    let (pixels, _) = rgba.as_chunks::<4>();
    pixels
        .iter()
        .flat_map(|&[r, g, b, a]| [a, r, g, b])
        .collect()
}

fn to_ksni(items: &[TrayItem], actions: &Sender<TrayAction>) -> Vec<MenuItem<TrayModel>> {
    items.iter().map(|item| convert(item, actions)).collect()
}

fn sender(action: &TrayAction, actions: &Sender<TrayAction>) -> Box<dyn Fn(&mut TrayModel) + Send> {
    let action = action.clone();
    let actions = actions.clone();
    Box::new(move |_| {
        let _ = actions.send(action.clone());
    })
}

fn convert(item: &TrayItem, actions: &Sender<TrayAction>) -> MenuItem<TrayModel> {
    match item {
        TrayItem::Label(label) => StandardItem {
            label: label.clone(),
            enabled: false,
            ..Default::default()
        }
        .into(),
        TrayItem::Separator => MenuItem::Separator,
        TrayItem::Action {
            label,
            enabled,
            action,
        } => StandardItem {
            label: label.clone(),
            enabled: *enabled,
            activate: sender(action, actions),
            ..Default::default()
        }
        .into(),
        TrayItem::Check {
            label,
            checked,
            enabled,
            action,
        } => CheckmarkItem {
            label: label.clone(),
            checked: *checked,
            enabled: *enabled,
            activate: sender(action, actions),
            ..Default::default()
        }
        .into(),
        TrayItem::Submenu {
            label,
            enabled,
            items,
        } => SubMenu {
            label: label.clone(),
            enabled: *enabled,
            submenu: to_ksni(items, actions),
            ..Default::default()
        }
        .into(),
    }
}

pub fn run() -> Result<(), String> {
    let mut launcher = Launcher::for_current_exe()?;
    let (actions, received) = unbounded();
    let handle_cell: Arc<OnceLock<Handle<TrayModel>>> = Arc::new(OnceLock::new());

    let mut shared = Shared::default();
    let link = match Endpoint::for_current_user() {
        Ok(endpoint) => {
            let cell = handle_cell.clone();
            Some(Arc::new(DaemonLink::start(endpoint, move || {
                if let Some(handle) = cell.get() {
                    handle.update(TrayModel::drain);
                }
            })))
        }
        Err(error) => {
            shared.daemon_down = Some(error.to_string());
            None
        }
    };

    let model = TrayModel {
        shared,
        link: link.clone(),
        actions,
    };
    let handle = model
        .spawn()
        .map_err(|error| format!("could not register the tray: {error}"))?;
    let _ = handle_cell.set(handle.clone());
    handle.update(TrayModel::drain);

    let outcome = serve(&received, link.as_deref(), &mut launcher, |error| {
        handle.update(|model| model.shared.last_error = Some(error));
    });
    handle.shutdown().wait();
    outcome
}

/// Runs the actions the menu sends until Quit. A closed channel means the tray service died.
fn serve(
    received: &Receiver<TrayAction>,
    link: Option<&DaemonLink>,
    launcher: &mut Launcher,
    report: impl Fn(String),
) -> Result<(), String> {
    loop {
        match received.recv() {
            Ok(action) => {
                if super::perform(action, link, launcher, &report).is_break() {
                    return Ok(());
                }
            }
            Err(_) => return Err("the tray service stopped".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::unbounded;
    use ksni::menu::MenuItem;
    use ogh_ipc::{Request, Setting};

    use super::{TrayModel, argb, serve, to_ksni};
    use crate::Shared;
    use crate::launcher::Launcher;
    use crate::tray::test_fixtures::connected_headset;
    use crate::tray_menu::{TrayAction, TrayItem, menu};

    #[derive(Debug, PartialEq)]
    enum Shape {
        Separator,
        Item {
            label: String,
            enabled: bool,
        },
        Check {
            label: String,
            checked: bool,
            enabled: bool,
        },
        Submenu {
            label: String,
            enabled: bool,
            items: Vec<Shape>,
        },
    }

    fn shape_of_ksni(items: &[MenuItem<TrayModel>]) -> Vec<Shape> {
        items
            .iter()
            .map(|item| match item {
                MenuItem::Separator => Shape::Separator,
                MenuItem::Standard(item) => Shape::Item {
                    label: item.label.clone(),
                    enabled: item.enabled,
                },
                MenuItem::Checkmark(item) => Shape::Check {
                    label: item.label.clone(),
                    checked: item.checked,
                    enabled: item.enabled,
                },
                MenuItem::SubMenu(item) => Shape::Submenu {
                    label: item.label.clone(),
                    enabled: item.enabled,
                    items: shape_of_ksni(&item.submenu),
                },
                MenuItem::RadioGroup(_) => panic!("the tray menu has no radio groups"),
            })
            .collect()
    }

    fn shape_of_items(items: &[TrayItem]) -> Vec<Shape> {
        items
            .iter()
            .map(|item| match item {
                TrayItem::Separator => Shape::Separator,
                TrayItem::Label(label) => Shape::Item {
                    label: label.clone(),
                    enabled: false,
                },
                TrayItem::Action { label, enabled, .. } => Shape::Item {
                    label: label.clone(),
                    enabled: *enabled,
                },
                TrayItem::Check {
                    label,
                    checked,
                    enabled,
                    ..
                } => Shape::Check {
                    label: label.clone(),
                    checked: *checked,
                    enabled: *enabled,
                },
                TrayItem::Submenu {
                    label,
                    enabled,
                    items,
                } => Shape::Submenu {
                    label: label.clone(),
                    enabled: *enabled,
                    items: shape_of_items(items),
                },
            })
            .collect()
    }

    fn model(shared: Shared) -> (TrayModel, crossbeam_channel::Receiver<TrayAction>) {
        let (actions, received) = unbounded();
        (
            TrayModel {
                shared,
                link: None,
                actions,
            },
            received,
        )
    }

    #[test]
    fn ksni_menu_keeps_structure() {
        let items = menu(&connected_headset());
        let (actions, _) = unbounded();

        let converted = to_ksni(&items, &actions);

        let shape = shape_of_ksni(&converted);
        assert_eq!(shape, shape_of_items(&items));
        assert_eq!(
            shape[0],
            Shape::Item {
                label: "A50 X: in use, 80% (charging)".to_string(),
                enabled: false,
            }
        );
        let Shape::Submenu { label, items, .. } = &shape[2] else {
            panic!("expected the profile submenu, got {:?}", shape[2]);
        };
        assert_eq!(label, "Profile");
        assert_eq!(
            items[1],
            Shape::Check {
                label: "Music".to_string(),
                checked: true,
                enabled: true,
            }
        );
    }

    #[test]
    fn activating_an_item_sends_its_action() {
        let (mut model, received) = model(connected_headset());
        let converted = to_ksni(&menu(&model.shared), &model.actions);

        let sidetone = converted
            .iter()
            .find_map(|item| match item {
                MenuItem::SubMenu(sub) if sub.label == "Sidetone" => Some(sub),
                _ => None,
            })
            .unwrap();
        let MenuItem::Checkmark(zero) = &sidetone.submenu[0] else {
            panic!("expected a checkmark");
        };
        (zero.activate)(&mut model);

        assert_eq!(
            received.try_recv().unwrap(),
            TrayAction::Request(Request::Set {
                setting: Setting::Sidetone { percent: 0 }
            })
        );
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn argb_pixmap_reorders_channels() {
        assert_eq!(argb(&[1, 2, 3, 4]), vec![4, 1, 2, 3]);
        assert_eq!(
            argb(&[1, 2, 3, 4, 5, 6, 7, 8]),
            vec![4, 1, 2, 3, 8, 5, 6, 7]
        );
    }

    #[test]
    fn quit_ends_the_loop_cleanly() {
        let (actions, received) = unbounded();
        actions.send(TrayAction::Quit).unwrap();
        let mut launcher = Launcher::new("missing".into());

        assert_eq!(serve(&received, None, &mut launcher, |_| {}), Ok(()));
    }

    #[test]
    fn a_stopped_service_is_an_error() {
        let (actions, received) = unbounded();
        drop(actions);
        let mut launcher = Launcher::new("missing".into());

        let outcome = serve(&received, None, &mut launcher, |_| {});

        assert_eq!(outcome, Err("the tray service stopped".to_string()));
    }

    #[test]
    fn a_failed_launch_is_reported() {
        let (actions, received) = unbounded();
        actions.send(TrayAction::OpenWindow).unwrap();
        actions.send(TrayAction::Quit).unwrap();
        let mut launcher = Launcher::new("/nonexistent/ogh-gui".into());
        let reported = std::cell::RefCell::new(Vec::new());

        serve(&received, None, &mut launcher, |error| {
            reported.borrow_mut().push(error)
        })
        .unwrap();

        let reported = reported.into_inner();
        assert_eq!(reported.len(), 1);
        assert!(reported[0].contains("ogh-gui"), "{}", reported[0]);
    }
}
