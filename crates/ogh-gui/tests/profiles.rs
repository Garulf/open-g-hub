use eframe::egui::Pos2;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use ogh_gui::link::LinkEvent;
use ogh_gui::{GuiState, Page};
use ogh_ipc::{Connection, Request};

mod common;
use common::{Window, gui_with, harness, on_page, profile, status, with_profiles};

fn profiles_gui(connection: Connection) -> GuiState {
    let mut status = status(connection);
    status.active_profile = Some("Gaming".to_string());
    let gui = with_profiles(
        gui_with(status),
        vec![
            profile("Default", &["firefox"], true),
            profile("Gaming", &[], false),
        ],
    );
    on_page(gui, Page::Profiles)
}

fn started(connection: Connection) -> Harness<'static, Window> {
    let mut harness = harness(profiles_gui(connection));
    harness.run();
    harness
}

fn click_nth(harness: &mut Harness<'_, Window>, label: &str, index: usize) {
    harness
        .get_all_by_label(label)
        .nth(index)
        .unwrap_or_else(|| panic!("no {label} number {index}"))
        .click();
    harness.run();
}

fn type_into(harness: &mut Harness<'_, Window>, label: &str, text: &str) {
    harness
        .get_by_role_and_label(Role::TextInput, label)
        .focus();
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, label)
        .type_text(text);
    harness.run();
}

fn field_text(harness: &Harness<'_, Window>, label: &str) -> String {
    harness
        .get_by_role_and_label(Role::TextInput, label)
        .accesskit_node()
        .value()
        .unwrap_or_default()
        .to_string()
}

fn disabled(harness: &Harness<'_, Window>, label: &str, index: usize) -> bool {
    harness
        .get_all_by_label(label)
        .nth(index)
        .unwrap_or_else(|| panic!("no {label} number {index}"))
        .accesskit_node()
        .is_disabled()
}

fn same_line(harness: &Harness<'_, Window>, tag: &str, name: &str) -> bool {
    let tag = harness.get_by_label(tag).rect().center().y;
    let name = harness.get_by_label(name).rect().center().y;
    (tag - name).abs() < 1.0
}

#[test]
fn profiles_are_listed_with_tags() {
    let harness = started(Connection::Connected);

    harness.get_by_label("firefox");
    assert_eq!(harness.get_all_by_label("default").count(), 1);
    assert_eq!(harness.get_all_by_label("active").count(), 1);
    assert!(same_line(&harness, "default", "Default"));
    assert!(!same_line(&harness, "default", "Gaming"));
    assert!(same_line(&harness, "active", "Gaming"));
    assert!(!same_line(&harness, "active", "Default"));
}

#[test]
fn apply_sends_apply_profile() {
    let mut harness = started(Connection::Connected);

    click_nth(&mut harness, "Apply", 1);

    assert_eq!(
        harness.state().sent,
        vec![Request::ApplyProfile {
            name: "Gaming".to_string()
        }]
    );
}

#[test]
fn update_sends_save_profile_for_that_row() {
    let mut harness = started(Connection::Connected);

    click_nth(&mut harness, "Update", 0);

    assert_eq!(
        harness.state().sent,
        vec![Request::SaveProfile {
            name: "Default".to_string()
        }]
    );
}

#[test]
fn make_default_sends_set_default_profile() {
    let mut harness = started(Connection::Connected);

    harness.get_by_label("Make default").click();
    harness.run();

    assert_eq!(
        harness.state().sent,
        vec![Request::SetDefaultProfile {
            name: "Gaming".to_string()
        }]
    );
}

#[test]
fn make_default_is_hidden_on_the_default() {
    let harness = started(Connection::Connected);

    assert_eq!(harness.get_all_by_label("Make default").count(), 1);
}

#[test]
fn delete_needs_a_second_click() {
    let mut harness = started(Connection::Connected);

    click_nth(&mut harness, "Delete", 1);
    assert!(harness.state().sent.is_empty());
    harness.get_by_label("Really delete?");
    assert_eq!(harness.get_all_by_label("Delete").count(), 1);

    harness.get_by_label("Really delete?").click();
    harness.run();

    assert_eq!(
        harness.state().sent,
        vec![Request::DeleteProfile {
            name: "Gaming".to_string()
        }]
    );
    assert!(harness.query_by_label("Really delete?").is_none());
}

#[test]
fn clicking_another_control_disarms_delete() {
    let mut harness = started(Connection::Connected);
    click_nth(&mut harness, "Delete", 1);
    harness.get_by_label("Really delete?");

    click_nth(&mut harness, "Update", 0);

    assert!(harness.query_by_label("Really delete?").is_none());
    assert_eq!(harness.get_all_by_label("Delete").count(), 2);
}

#[test]
fn clicking_blank_space_disarms_delete() {
    let mut harness = started(Connection::Connected);
    click_nth(&mut harness, "Delete", 1);
    harness.get_by_label("Really delete?");

    let blank = Pos2::new(800.0, 540.0);
    harness.hover_at(blank);
    harness.drag_at(blank);
    harness.run();
    harness.drop_at(blank);
    harness.run();

    assert_eq!(harness.get_all_by_label("Delete").count(), 2);
    click_nth(&mut harness, "Delete", 1);
    assert!(harness.state().sent.is_empty());
    harness.get_by_label("Really delete?");
}

#[test]
fn arming_another_row_moves_the_confirmation() {
    let mut harness = started(Connection::Connected);
    click_nth(&mut harness, "Delete", 1);

    click_nth(&mut harness, "Delete", 0);

    assert!(harness.state().sent.is_empty());
    assert_eq!(harness.get_all_by_label("Really delete?").count(), 1);
    harness.get_by_label("Really delete?").click();
    harness.run();
    assert_eq!(
        harness.state().sent,
        vec![Request::DeleteProfile {
            name: "Default".to_string()
        }]
    );
}

#[test]
fn leaving_the_page_disarms_delete() {
    let mut harness = started(Connection::Connected);
    click_nth(&mut harness, "Delete", 1);
    harness.get_by_label("Really delete?");

    harness.state_mut().gui.page = Page::Mixer;
    harness.run();
    harness.state_mut().gui.page = Page::Profiles;
    harness.run();

    assert!(harness.query_by_label("Really delete?").is_none());
}

#[test]
fn deleting_the_armed_profile_elsewhere_disarms_it() {
    let mut harness = started(Connection::Connected);
    click_nth(&mut harness, "Delete", 1);

    let shared = &mut harness.state_mut().gui.shared;
    shared.apply(LinkEvent::Profiles(vec![profile("Default", &[], true)]));
    harness.run();
    let shared = &mut harness.state_mut().gui.shared;
    shared.apply(LinkEvent::Profiles(vec![
        profile("Default", &[], true),
        profile("Gaming", &[], false),
    ]));
    harness.run();

    assert!(harness.query_by_label("Really delete?").is_none());
}

#[test]
fn link_app_sends_trimmed_name_and_clears_the_field() {
    let mut harness = started(Connection::Connected);
    assert!(disabled(&harness, "Link app", 1));

    type_into(&mut harness, "App to link to Gaming", "  gimp  ");
    assert!(!disabled(&harness, "Link app", 1));
    assert!(disabled(&harness, "Link app", 0));
    click_nth(&mut harness, "Link app", 1);

    assert_eq!(
        harness.state().sent,
        vec![Request::LinkApp {
            profile: "Gaming".to_string(),
            app: "gimp".to_string()
        }]
    );
    assert_eq!(field_text(&harness, "App to link to Gaming"), "");
}

#[test]
fn whitespace_only_app_keeps_link_disabled() {
    let mut harness = started(Connection::Connected);

    type_into(&mut harness, "App to link to Gaming", "   ");

    assert!(disabled(&harness, "Link app", 1));
}

#[test]
fn link_drafts_are_per_profile_and_dropped_with_the_profile() {
    let mut harness = started(Connection::Connected);
    type_into(&mut harness, "App to link to Gaming", "gimp");
    assert_eq!(field_text(&harness, "App to link to Default"), "");

    let shared = &mut harness.state_mut().gui.shared;
    shared.apply(LinkEvent::Profiles(vec![profile("Default", &[], true)]));
    harness.run();
    let shared = &mut harness.state_mut().gui.shared;
    shared.apply(LinkEvent::Profiles(vec![
        profile("Default", &[], true),
        profile("Gaming", &[], false),
    ]));
    harness.run();

    assert_eq!(field_text(&harness, "App to link to Gaming"), "");
}

#[test]
fn unlink_sends_unlink_app() {
    let mut harness = started(Connection::Connected);

    harness
        .get_by_role_and_label(Role::Button, "Unlink firefox")
        .click();
    harness.run();

    assert_eq!(
        harness.state().sent,
        vec![Request::UnlinkApp {
            app: "firefox".to_string()
        }]
    );
}

#[test]
fn save_new_is_disabled_for_an_existing_name() {
    let mut harness = started(Connection::Connected);
    let save = "Save current settings as new profile";
    assert!(disabled(&harness, save, 0));

    type_into(&mut harness, "New profile name", "  Gaming ");
    assert!(disabled(&harness, save, 0));
    click_nth(&mut harness, save, 0);
    assert!(harness.state().sent.is_empty());
}

#[test]
fn save_new_sends_the_trimmed_name_and_clears_the_field() {
    let mut harness = started(Connection::Connected);
    let save = "Save current settings as new profile";

    type_into(&mut harness, "New profile name", "  Racing ");
    assert!(!disabled(&harness, save, 0));
    click_nth(&mut harness, save, 0);

    assert_eq!(
        harness.state().sent,
        vec![Request::SaveProfile {
            name: "Racing".to_string()
        }]
    );
    assert_eq!(field_text(&harness, "New profile name"), "");
}

#[test]
fn apply_and_save_are_disabled_when_not_connected() {
    let mut harness = started(Connection::GhubRunning);
    let save = "Save current settings as new profile";
    type_into(&mut harness, "New profile name", "Racing");
    type_into(&mut harness, "App to link to Gaming", "gimp");

    assert!(disabled(&harness, "Apply", 0));
    assert!(disabled(&harness, "Apply", 1));
    assert!(disabled(&harness, "Update", 0));
    assert!(disabled(&harness, "Update", 1));
    assert!(disabled(&harness, save, 0));
    click_nth(&mut harness, "Apply", 1);
    click_nth(&mut harness, "Update", 1);
    assert!(harness.state().sent.is_empty());

    assert!(!disabled(&harness, "Make default", 0));
    assert!(!disabled(&harness, "Delete", 0));
    assert!(!disabled(&harness, "Link app", 1));
    click_nth(&mut harness, "Link app", 1);
    harness
        .get_by_role_and_label(Role::Button, "Unlink firefox")
        .click();
    harness.run();
    assert_eq!(
        harness.state().sent,
        vec![
            Request::LinkApp {
                profile: "Gaming".to_string(),
                app: "gimp".to_string()
            },
            Request::UnlinkApp {
                app: "firefox".to_string()
            }
        ]
    );
}

#[test]
fn an_empty_list_says_so() {
    let gui = on_page(gui_with(status(Connection::Connected)), Page::Profiles);
    let mut harness = harness(gui);
    harness.run();

    harness.get_by_label("No profiles yet.");
}
