use std::collections::HashMap;

use eframe::egui::{Button, Response, RichText, TextEdit, Ui};
use ogh_ipc::{ProfileInfo, Request};

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

#[derive(Default)]
struct RowOutcome {
    delete_clicked: bool,
    other_activity: bool,
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
    let mut other_activity = false;
    for profile in profiles {
        let armed = drafts.armed_delete.as_deref() == Some(profile.name.as_str());
        let outcome = ui
            .push_id(("profile", &profile.name), |ui| {
                row(ui, profile, active, armed, drafts, can_write, requests)
            })
            .inner;
        if outcome.delete_clicked {
            drafts.armed_delete = (!armed).then(|| profile.name.clone());
        } else {
            other_activity |= outcome.other_activity;
        }
        ui.separator();
    }

    ui.add_space(4.0);
    other_activity |= new_profile(ui, profiles, drafts, can_write, requests);
    if other_activity {
        drafts.disarm();
    }
}

fn row(
    ui: &mut Ui,
    profile: &ProfileInfo,
    active: Option<&str>,
    armed: bool,
    drafts: &mut Drafts,
    can_write: bool,
    requests: &mut Vec<Request>,
) -> RowOutcome {
    let mut outcome = RowOutcome::default();
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
            label(ui, &unlink, format!("Unlink {app}"));
            if unlink.clicked() {
                outcome.other_activity = true;
                requests.push(Request::UnlinkApp { app: app.clone() });
            }
        }
    });

    ui.horizontal(|ui| {
        if click(ui, "Apply", can_write, &mut outcome) {
            requests.push(Request::ApplyProfile { name: name.clone() });
        }
        if !profile.is_default && click(ui, "Make default", true, &mut outcome) {
            requests.push(Request::SetDefaultProfile { name: name.clone() });
        }
        if click(ui, "Update", can_write, &mut outcome) {
            requests.push(Request::SaveProfile { name: name.clone() });
        }
        let delete_label = if armed { "Really delete?" } else { "Delete" };
        if ui.button(delete_label).clicked() {
            outcome.delete_clicked = true;
            if armed {
                requests.push(Request::DeleteProfile { name: name.clone() });
            }
        }
    });

    ui.horizontal(|ui| {
        let text = drafts.link_app.entry(name.clone()).or_default();
        let field = ui.add(TextEdit::singleline(text).hint_text("App to link"));
        label(ui, &field, format!("App to link to {name}"));
        outcome.other_activity |= field.clicked();
        let app = text.trim().to_string();
        if click(ui, "Link app", !app.is_empty(), &mut outcome) {
            text.clear();
            requests.push(Request::LinkApp {
                profile: name.clone(),
                app,
            });
        }
    });
    outcome
}

/// Returns whether the user did anything here that should cancel a pending delete.
fn new_profile(
    ui: &mut Ui,
    profiles: &[ProfileInfo],
    drafts: &mut Drafts,
    can_write: bool,
    requests: &mut Vec<Request>,
) -> bool {
    let mut outcome = RowOutcome::default();
    ui.horizontal(|ui| {
        let field =
            ui.add(TextEdit::singleline(&mut drafts.new_name).hint_text("New profile name"));
        label(ui, &field, "New profile name".to_string());
        outcome.other_activity |= field.clicked();
        let name = drafts.new_name.trim().to_string();
        let free = !name.is_empty() && profiles.iter().all(|profile| profile.name.trim() != name);
        if click(
            ui,
            "Save current settings as new profile",
            can_write && free,
            &mut outcome,
        ) {
            drafts.new_name.clear();
            requests.push(Request::SaveProfile { name });
        }
    });
    outcome.other_activity
}

/// Draws a button and counts a click as activity that cancels a pending delete.
fn click(ui: &mut Ui, text: &str, enabled: bool, outcome: &mut RowOutcome) -> bool {
    let clicked = ui
        .add_enabled(enabled, Button::new(RichText::new(text)))
        .clicked();
    outcome.other_activity |= clicked;
    clicked
}

fn label(ui: &Ui, response: &Response, text: String) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_label(text);
    });
}
