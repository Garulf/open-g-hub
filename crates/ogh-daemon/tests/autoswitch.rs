use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ogh_audio::{AudioError, AudioRouter, Defaults, Direction, Role, Slot};
use ogh_daemon::autoswitch::{AudioSwitchOutcome, AutoSwitch};
use ogh_daemon::config::AudioSwitchConfig;
use ogh_ipc::Headset;
use tempfile::TempDir;

const SLOTS: &[Slot] = &[
    Slot::new(Direction::Render, Role::Console),
    Slot::new(Direction::Render, Role::Multimedia),
    Slot::new(Direction::Render, Role::Communications),
    Slot::new(Direction::Capture, Role::Console),
    Slot::new(Direction::Capture, Role::Multimedia),
    Slot::new(Direction::Capture, Role::Communications),
];

const RENDER_CONSOLE: Slot = SLOTS[0];
const RENDER_MULTIMEDIA: Slot = SLOTS[1];
const RENDER_COMMUNICATIONS: Slot = SLOTS[2];
const CAPTURE_CONSOLE: Slot = SLOTS[3];

#[derive(Default)]
struct FakeState {
    current: Defaults,
    endpoints: BTreeMap<(Direction, String), String>,
    failing: BTreeSet<String>,
    find_error: Option<AudioError>,
    sets: Vec<(Slot, String)>,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<FakeState>>);

impl Fake {
    /// Speakers and a headset-less setup, with the A50 X endpoints present.
    fn with_a50x() -> Fake {
        let fake = Fake::default();
        {
            let mut state = fake.0.lock().unwrap();
            for slot in [RENDER_CONSOLE, RENDER_MULTIMEDIA] {
                state.current.insert(slot, "speakers".to_string());
            }
            state
                .current
                .insert(RENDER_COMMUNICATIONS, "chat-headset".to_string());
            for slot in &SLOTS[3..] {
                state.current.insert(*slot, "desk-mic".to_string());
            }
            for (direction, name, id) in [
                (Direction::Render, "A50 X Game", "a50-game"),
                (Direction::Render, "A50 X Voice", "a50-voice"),
                (Direction::Capture, "A50 X Mic", "a50-mic"),
            ] {
                state
                    .endpoints
                    .insert((direction, name.to_string()), id.to_string());
            }
        }
        fake
    }

    fn router(&self) -> Box<dyn AudioRouter> {
        Box::new(self.clone())
    }

    fn current(&self) -> Defaults {
        self.0.lock().unwrap().current.clone()
    }

    fn sets(&self) -> Vec<(Slot, String)> {
        self.0.lock().unwrap().sets.clone()
    }

    fn clear_sets(&self) {
        self.0.lock().unwrap().sets.clear();
    }

    fn forget_endpoint(&self, name: &str) {
        self.0
            .lock()
            .unwrap()
            .endpoints
            .retain(|(_, n), _| n != name);
    }

    fn fail_setting(&self, endpoint: &str) {
        self.0.lock().unwrap().failing.insert(endpoint.to_string());
    }
}

impl AudioRouter for Fake {
    fn slots(&self) -> &'static [Slot] {
        SLOTS
    }

    fn current(&self) -> Result<Defaults, AudioError> {
        Ok(self.0.lock().unwrap().current.clone())
    }

    fn find(&self, direction: Direction, name: &str) -> Result<Option<String>, AudioError> {
        let state = self.0.lock().unwrap();
        if let Some(error) = &state.find_error {
            return Err(error.clone());
        }
        Ok(state.endpoints.get(&(direction, name.to_string())).cloned())
    }

    fn set(&self, slot: Slot, endpoint: &str) -> Result<(), AudioError> {
        let mut state = self.0.lock().unwrap();
        if state.failing.contains(endpoint) {
            return Err(AudioError::Failed(format!("{endpoint} is gone")));
        }
        state.sets.push((slot, endpoint.to_string()));
        state.current.insert(slot, endpoint.to_string());
        Ok(())
    }
}

fn enabled() -> AudioSwitchConfig {
    AudioSwitchConfig {
        enabled: true,
        ..AudioSwitchConfig::default()
    }
}

struct Rig {
    fake: Fake,
    state_file: PathBuf,
    _dir: TempDir,
}

impl Rig {
    fn new() -> Rig {
        let dir = TempDir::new().unwrap();
        Rig {
            fake: Fake::with_a50x(),
            state_file: dir.path().join("data").join("state.toml"),
            _dir: dir,
        }
    }

    fn auto_switch(&self, config: AudioSwitchConfig) -> AutoSwitch {
        AutoSwitch::new(self.fake.router(), config, self.state_file.clone())
    }

    fn state_text(&self) -> String {
        fs::read_to_string(&self.state_file).unwrap()
    }
}

fn ok(active: bool) -> AudioSwitchOutcome {
    AudioSwitchOutcome {
        active,
        error: None,
    }
}

fn error_of(outcome: &AudioSwitchOutcome) -> &str {
    outcome.error.as_deref().expect("an error")
}

fn original_defaults() -> Defaults {
    Fake::with_a50x().current()
}

fn a50x_defaults() -> Defaults {
    BTreeMap::from([
        (RENDER_CONSOLE, "a50-game".to_string()),
        (RENDER_MULTIMEDIA, "a50-game".to_string()),
        (RENDER_COMMUNICATIONS, "a50-voice".to_string()),
        (CAPTURE_CONSOLE, "a50-mic".to_string()),
        (SLOTS[4], "a50-mic".to_string()),
        (SLOTS[5], "a50-mic".to_string()),
    ])
}

#[test]
fn undock_remembers_and_switches_every_slot() {
    let rig = Rig::new();
    let mut auto_switch = rig.auto_switch(enabled());

    assert_eq!(auto_switch.on_headset(Headset::InUse), ok(true));

    assert_eq!(rig.fake.current(), a50x_defaults());
    assert_eq!(rig.fake.sets().len(), 6);
    let text = rig.state_text();
    assert!(text.contains("[remembered]"), "{text}");
    assert!(
        text.contains(r#""render.communications" = "chat-headset""#),
        "{text}"
    );
}

#[test]
fn dock_restores_each_slot_separately() {
    let rig = Rig::new();
    let mut auto_switch = rig.auto_switch(enabled());
    auto_switch.on_headset(Headset::InUse);

    assert_eq!(auto_switch.on_headset(Headset::Docked), ok(false));

    assert_eq!(rig.fake.current(), original_defaults());
    assert!(!rig.state_text().contains("speakers"));
}

#[test]
fn power_off_restores_too() {
    let rig = Rig::new();
    let mut auto_switch = rig.auto_switch(enabled());
    auto_switch.on_headset(Headset::InUse);

    assert_eq!(auto_switch.on_headset(Headset::Off), ok(false));

    assert_eq!(rig.fake.current(), original_defaults());
}

#[test]
fn remembered_state_survives_a_restart_and_restores_when_docked() {
    let rig = Rig::new();
    rig.auto_switch(enabled()).on_headset(Headset::InUse);

    let mut restarted = rig.auto_switch(enabled());
    assert_eq!(restarted.on_headset(Headset::Docked), ok(false));

    assert_eq!(rig.fake.current(), original_defaults());
}

#[test]
fn remembered_state_restores_after_a_restart_even_when_disabled() {
    let rig = Rig::new();
    rig.auto_switch(enabled()).on_headset(Headset::InUse);

    let mut restarted = rig.auto_switch(AudioSwitchConfig::default());
    restarted.on_headset(Headset::Off);

    assert_eq!(rig.fake.current(), original_defaults());
}

#[test]
fn restart_while_in_use_keeps_the_original_defaults() {
    let rig = Rig::new();
    rig.auto_switch(enabled()).on_headset(Headset::InUse);

    let mut restarted = rig.auto_switch(enabled());
    assert_eq!(restarted.on_headset(Headset::InUse), ok(true));
    restarted.on_headset(Headset::Docked);

    assert_eq!(rig.fake.current(), original_defaults());
}

#[test]
fn restore_skips_a_vanished_endpoint_and_reports_it() {
    let rig = Rig::new();
    let mut auto_switch = rig.auto_switch(enabled());
    auto_switch.on_headset(Headset::InUse);
    rig.fake.fail_setting("chat-headset");

    let outcome = auto_switch.on_headset(Headset::Docked);

    assert!(!outcome.active);
    assert_eq!(
        error_of(&outcome),
        "could not restore render.communications: chat-headset is gone"
    );
    let current = rig.fake.current();
    assert_eq!(current[&RENDER_CONSOLE], "speakers");
    assert_eq!(current[&CAPTURE_CONSOLE], "desk-mic");
    assert_eq!(current[&RENDER_COMMUNICATIONS], "a50-voice");
    assert!(!auto_switch.on_headset(Headset::Docked).active);
}

#[test]
fn missing_a50x_endpoint_changes_nothing() {
    let rig = Rig::new();
    rig.fake.forget_endpoint("A50 X Voice");
    let mut auto_switch = rig.auto_switch(enabled());

    let outcome = auto_switch.on_headset(Headset::InUse);

    assert!(!outcome.active);
    assert_eq!(error_of(&outcome), "audio device not found: A50 X Voice");
    assert!(rig.fake.sets().is_empty());
    assert_eq!(rig.fake.current(), original_defaults());
    assert!(!rig.state_file.exists());
}

#[test]
fn every_missing_name_is_reported_once() {
    let rig = Rig::new();
    rig.fake.forget_endpoint("A50 X Game");
    rig.fake.forget_endpoint("A50 X Mic");
    let mut auto_switch = rig.auto_switch(enabled());

    let outcome = auto_switch.on_headset(Headset::InUse);

    assert_eq!(
        error_of(&outcome),
        "audio device not found: A50 X Game, A50 X Mic"
    );
}

#[test]
fn a_lookup_failure_changes_nothing() {
    let rig = Rig::new();
    rig.fake.0.lock().unwrap().find_error = Some(AudioError::Unavailable(
        "pw-dump not found on PATH".to_string(),
    ));
    let mut auto_switch = rig.auto_switch(enabled());

    let outcome = auto_switch.on_headset(Headset::InUse);

    assert_eq!(error_of(&outcome), "pw-dump not found on PATH");
    assert!(rig.fake.sets().is_empty());
}

#[test]
fn disabled_does_nothing() {
    let rig = Rig::new();
    let mut auto_switch = rig.auto_switch(AudioSwitchConfig::default());

    assert_eq!(auto_switch.on_headset(Headset::InUse), ok(false));
    assert_eq!(auto_switch.on_headset(Headset::Docked), ok(false));

    assert!(rig.fake.sets().is_empty());
}

#[test]
fn disabling_while_active_restores() {
    let rig = Rig::new();
    let mut auto_switch = rig.auto_switch(enabled());
    auto_switch.on_headset(Headset::InUse);

    let outcome = auto_switch.configure(AudioSwitchConfig::default());

    assert_eq!(outcome, ok(false));
    assert_eq!(rig.fake.current(), original_defaults());
}

#[test]
fn enabling_while_in_use_switches() {
    let rig = Rig::new();
    let mut auto_switch = rig.auto_switch(AudioSwitchConfig::default());
    auto_switch.on_headset(Headset::InUse);
    assert!(rig.fake.sets().is_empty());

    assert_eq!(auto_switch.configure(enabled()), ok(true));

    assert_eq!(rig.fake.current(), a50x_defaults());
}

#[test]
fn enabling_while_docked_waits_for_the_next_undock() {
    let rig = Rig::new();
    let mut auto_switch = rig.auto_switch(AudioSwitchConfig::default());
    auto_switch.on_headset(Headset::Docked);

    assert_eq!(auto_switch.configure(enabled()), ok(false));
    assert!(rig.fake.sets().is_empty());
    assert_eq!(auto_switch.on_headset(Headset::InUse), ok(true));
}

#[test]
fn unknown_does_not_count_as_a_transition() {
    let rig = Rig::new();
    let mut auto_switch = rig.auto_switch(enabled());
    auto_switch.on_headset(Headset::InUse);
    rig.fake.clear_sets();
    rig.fake
        .0
        .lock()
        .unwrap()
        .current
        .insert(RENDER_CONSOLE, "user-pick".to_string());

    assert_eq!(auto_switch.on_headset(Headset::Unknown), ok(true));
    assert_eq!(auto_switch.on_headset(Headset::InUse), ok(true));

    assert!(rig.fake.sets().is_empty());
    assert_eq!(rig.fake.current()[&RENDER_CONSOLE], "user-pick");
}

#[test]
fn unknown_never_restores() {
    let rig = Rig::new();
    let mut auto_switch = rig.auto_switch(enabled());
    auto_switch.on_headset(Headset::InUse);
    rig.fake.clear_sets();

    assert_eq!(auto_switch.on_headset(Headset::Unknown), ok(true));

    assert!(rig.fake.sets().is_empty());
}

#[test]
fn a_failed_set_is_reported_and_the_rest_still_switch() {
    let rig = Rig::new();
    rig.fake.fail_setting("a50-voice");
    let mut auto_switch = rig.auto_switch(enabled());

    let outcome = auto_switch.on_headset(Headset::InUse);

    assert!(outcome.active);
    assert_eq!(
        error_of(&outcome),
        "could not set render.communications: a50-voice is gone"
    );
    assert_eq!(rig.fake.current()[&RENDER_CONSOLE], "a50-game");
    assert_eq!(rig.fake.current()[&CAPTURE_CONSOLE], "a50-mic");
}

#[test]
fn a_successful_action_clears_the_error() {
    let rig = Rig::new();
    rig.fake.forget_endpoint("A50 X Game");
    let mut auto_switch = rig.auto_switch(enabled());
    assert!(auto_switch.on_headset(Headset::InUse).error.is_some());
    auto_switch.on_headset(Headset::Docked);
    rig.fake.0.lock().unwrap().endpoints.insert(
        (Direction::Render, "A50 X Game".to_string()),
        "a50-game".to_string(),
    );

    assert_eq!(auto_switch.on_headset(Headset::InUse), ok(true));
}

#[test]
fn a_malformed_state_file_counts_as_empty() {
    let rig = Rig::new();
    fs::create_dir_all(rig.state_file.parent().unwrap()).unwrap();
    fs::write(&rig.state_file, "remembered = [not toml").unwrap();
    let mut auto_switch = rig.auto_switch(enabled());

    assert_eq!(auto_switch.on_headset(Headset::Docked), ok(false));
    assert!(rig.fake.sets().is_empty());
}

#[test]
fn state_file_ignores_unknown_keys() {
    let rig = Rig::new();
    rig.auto_switch(enabled()).on_headset(Headset::InUse);
    let text = format!("future = 1\n\n{}", rig.state_text());
    fs::write(&rig.state_file, text).unwrap();

    let mut restarted = rig.auto_switch(enabled());
    assert_eq!(restarted.on_headset(Headset::Docked), ok(false));

    assert_eq!(rig.fake.current(), original_defaults());
}

#[test]
fn restore_clears_the_state_file() {
    let rig = Rig::new();
    let mut auto_switch = rig.auto_switch(enabled());
    auto_switch.on_headset(Headset::InUse);

    auto_switch.on_headset(Headset::Docked);

    let state: toml::Table = rig.state_text().parse().unwrap();
    let remembered = state.get("remembered").and_then(|v| v.as_table());
    assert!(remembered.is_none_or(|table| table.is_empty()), "{state:?}");
}

#[test]
fn failed_state_save_aborts_the_switch() {
    let dir = TempDir::new().unwrap();
    let blocker = dir.path().join("blocker");
    fs::write(&blocker, "not a directory").unwrap();
    let fake = Fake::with_a50x();
    let mut auto_switch = AutoSwitch::new(fake.router(), enabled(), blocker.join("state.toml"));

    let outcome = auto_switch.on_headset(Headset::InUse);

    assert!(fake.sets().is_empty());
    assert_eq!(fake.current(), original_defaults());
    assert!(!outcome.active);
    assert!(
        error_of(&outcome).starts_with("could not save audio state"),
        "{outcome:?}"
    );
}

#[test]
fn failed_state_clear_is_reported_after_the_restore() {
    let rig = Rig::new();
    let mut auto_switch = rig.auto_switch(enabled());
    auto_switch.on_headset(Headset::InUse);
    fs::remove_dir_all(rig.state_file.parent().unwrap()).unwrap();
    fs::write(rig.state_file.parent().unwrap(), "not a directory").unwrap();

    let outcome = auto_switch.on_headset(Headset::Docked);

    assert_eq!(rig.fake.current(), original_defaults());
    assert!(!outcome.active);
    assert!(
        error_of(&outcome).starts_with("could not save audio state"),
        "{outcome:?}"
    );
}

#[test]
fn successful_dock_clears_a_previous_error() {
    let rig = Rig::new();
    rig.fake.forget_endpoint("A50 X Game");
    let mut auto_switch = rig.auto_switch(enabled());
    assert!(auto_switch.on_headset(Headset::InUse).error.is_some());

    assert_eq!(auto_switch.on_headset(Headset::Docked), ok(false));
}

#[test]
fn disabling_with_nothing_remembered_clears_a_previous_error() {
    let rig = Rig::new();
    rig.fake.forget_endpoint("A50 X Game");
    let mut auto_switch = rig.auto_switch(enabled());
    assert!(auto_switch.on_headset(Headset::InUse).error.is_some());

    assert_eq!(
        auto_switch.configure(AudioSwitchConfig::default()),
        ok(false)
    );
}

#[test]
fn new_names_while_in_use_switch_again_and_keep_the_originals() {
    let rig = Rig::new();
    rig.fake.0.lock().unwrap().endpoints.insert(
        (Direction::Render, "Desk Amp".to_string()),
        "desk-amp".to_string(),
    );
    let mut auto_switch = rig.auto_switch(enabled());
    auto_switch.on_headset(Headset::InUse);
    rig.fake.clear_sets();

    let renamed = AudioSwitchConfig {
        game_output: "Desk Amp".to_string(),
        ..enabled()
    };
    assert_eq!(auto_switch.configure(renamed.clone()), ok(true));
    let current = rig.fake.current();
    assert_eq!(current[&RENDER_CONSOLE], "desk-amp");
    assert_eq!(current[&RENDER_MULTIMEDIA], "desk-amp");
    assert_eq!(current[&RENDER_COMMUNICATIONS], "a50-voice");

    rig.fake.clear_sets();
    assert_eq!(auto_switch.configure(renamed), ok(true));
    assert!(rig.fake.sets().is_empty(), "unchanged names switch nothing");

    assert_eq!(auto_switch.on_headset(Headset::Docked), ok(false));
    assert_eq!(rig.fake.current(), original_defaults());
}
