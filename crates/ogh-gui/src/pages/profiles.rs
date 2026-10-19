use std::collections::HashMap;

use eframe::egui::{Button, TextEdit, Ui};
use ogh_ipc::{ProfileInfo, Request};

use super::set_accessible_label;
use crate::app::GuiState;

/// Text the user is typing and the delete confirmation, none of which the daemon knows.
#[derive(Debug, Default)]
pub(crate) struct Drafts {
    link_app: HashMap<String, String>,
    new_name: String,
    armed_delete: Option<String>,
}

impl Drafts {
    pub(crate) fn disarm(&mut self) {
        self.armed_delete = None;
    }

    fn forget_missing(&mut self, profiles: &[ProfileInfo]) {
        let exists = |name: &str| profiles.iter().any(|profile| profile.name == name);
        self.link_app.retain(|name, _| exists(name));
        if self
            .armed_delete
            .as_deref()
            .is_some_and(|name| !exists(name))
        {
            self.armed_delete = None;
        }
    }
}

pub(crate) fn show(ui: &mut Ui, state: &mut GuiState, requests: &mut Vec<Request>) {
    let can_write = state.shared.can_write();
    let active = state
        .shared
        .status
        .as_ref()
        .and_then(|status| status.active_profile.as_deref());
    let profiles = &state.shared.profiles;
    let drafts = &mut state.profile_drafts;
    drafts.forget_missing(profiles);

    if profiles.is_empty() {
        ui.weak("No profiles yet.");
    }
    let mut delete_clicked = false;
    for profile in profiles {
        let armed = drafts.armed_delete.as_deref() == Some(profile.name.as_str());
        let clicked = ui
            .push_id(("profile", &profile.name), |ui| {
                row(ui, profile, active, armed, drafts, can_write, requests)
            })
            .inner;
        if clicked {
            drafts.armed_delete = (!armed).then(|| profile.name.clone());
            delete_clicked = true;
        }
        ui.separator();
    }

    ui.add_space(4.0);
    ui.push_id("new_profile", |ui| {
        new_profile(ui, profiles, drafts, can_write, requests)
    });
    // The click that arms a row must not also disarm it.
    if !delete_clicked && ui.input(|input| input.pointer.any_click()) {
        drafts.disarm();
    }
}

/// Returns whether this row's delete button was clicked.
fn row(
    ui: &mut Ui,
    profile: &ProfileInfo,
    active: Option<&str>,
    armed: bool,
    drafts: &mut Drafts,
    can_write: bool,
    requests: &mut Vec<Request>,
) -> bool {
    let mut delete_clicked = false;
    let name = &profile.name;

    ui.horizontal(|ui| {
        ui.strong(name);
        if profile.is_default {
            ui.weak("default");
        }
        if active == Some(name.as_str()) {
            ui.colored_label(ui.visuals().selection.stroke.color, "active");
        }
    });

    ui.horizontal_wrapped(|ui| {
        for app in &profile.apps {
            ui.label(app);
            let unlink = ui.small_button("x");
            set_accessible_label(ui, &unlink, format!("Unlink {app}"));
            if unlink.clicked() {
                requests.push(Request::UnlinkApp { app: app.clone() });
            }
        }
    });

    ui.horizontal(|ui| {
        if button(ui, "Apply", can_write) {
            requests.push(Request::ApplyProfile { name: name.clone() });
        }
        if !profile.is_default && button(ui, "Make default", true) {
            requests.push(Request::SetDefaultProfile { name: name.clone() });
        }
        if button(ui, "Update", can_write) {
            requests.push(Request::SaveProfile { name: name.clone() });
        }
        let delete_label = if armed { "Really delete?" } else { "Delete" };
        if ui.button(delete_label).clicked() {
            delete_clicked = true;
            if armed {
                requests.push(Request::DeleteProfile { name: name.clone() });
            }
        }
    });

    ui.horizontal(|ui| {
        let text = drafts.link_app.entry(name.clone()).or_default();
        let field = ui.add(TextEdit::singleline(text).hint_text("App to link"));
        set_accessible_label(ui, &field, format!("App to link to {name}"));
        let app = text.trim().to_string();
        if button(ui, "Link app", !app.is_empty()) {
            text.clear();
            requests.push(Request::LinkApp {
                profile: name.clone(),
                app,
            });
        }
    });
    delete_clicked
}

fn new_profile(
    ui: &mut Ui,
    profiles: &[ProfileInfo],
    drafts: &mut Drafts,
    can_write: bool,
    requests: &mut Vec<Request>,
) {
    ui.horizontal(|ui| {
        let field =
            ui.add(TextEdit::singleline(&mut drafts.new_name).hint_text("New profile name"));
        set_accessible_label(ui, &field, "New profile name");
        let name = drafts.new_name.trim().to_string();
        let free = !name.is_empty() && profiles.iter().all(|profile| profile.name.trim() != name);
        if button(
            ui,
            "Save current settings as new profile",
            can_write && free,
        ) {
            drafts.new_name.clear();
            requests.push(Request::SaveProfile { name });
        }
    });
}

fn button(ui: &mut Ui, text: &str, enabled: bool) -> bool {
    ui.add_enabled(enabled, Button::new(text)).clicked()
}
