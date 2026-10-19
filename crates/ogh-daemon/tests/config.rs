use std::fs;
use std::path::PathBuf;

use ogh_daemon::config::{Config, Loaded, Store, load, save};
use ogh_daemon::paths::Paths;
use ogh_ipc::{Channel, Eq, EqBand, ErrorKind, ProfileSettings, StreamMixSetting};
use tempfile::TempDir;

fn config_path(dir: &TempDir) -> PathBuf {
    Paths::in_dir(dir.path()).config_file
}

fn flat_eq() -> Eq {
    Eq {
        bands: (0..10)
            .map(|i| EqBand {
                freq_hz: 100 * (i + 1),
                q: None,
                gain_db: if i == 2 { 1.5 } else { 0.0 },
            })
            .collect(),
    }
}

fn kind<T: std::fmt::Debug>(result: Result<T, ogh_ipc::IpcError>) -> ErrorKind {
    result.unwrap_err().kind
}

#[test]
fn missing_file_gives_defaults_and_first_save_creates_it() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nested").join("config.toml");
    assert_eq!(load(&path), Loaded::Missing);

    let mut store = Store::open(&path);
    assert_eq!(store.config_error(), None);
    assert_eq!(store.config().unwrap(), &Config::default());
    assert_eq!(store.config().unwrap().default_profile, "default");
    assert!(!store.config().unwrap().audio_switch.enabled);
    assert!(!path.exists());

    store.set_audio_switch(true).unwrap();
    assert!(path.exists());
    assert!(Store::open(&path).config().unwrap().audio_switch.enabled);
}

#[test]
fn round_trip_keeps_profiles_and_links() {
    let dir = TempDir::new().unwrap();
    let path = config_path(&dir);
    let settings = ProfileSettings {
        sidetone_percent: Some(30),
        headphone_eq: Some(flat_eq()),
        stream_mix: Some(StreamMixSetting {
            output: Channel {
                level_percent: 80,
                muted: false,
            },
            game: Channel {
                level_percent: 50,
                muted: true,
            },
            voice: Channel {
                level_percent: 60,
                muted: false,
            },
        }),
        ..ProfileSettings::default()
    };
    let mut store = Store::open(&path);
    store.save_profile("fps", settings.clone()).unwrap();
    store.link_app("fps", "cs2.exe").unwrap();
    store.link_app("fps", "valorant.exe").unwrap();

    let reopened = Store::open(&path);
    let fps = &reopened.config().unwrap().profiles["fps"];
    assert_eq!(fps.settings, settings);
    assert_eq!(fps.apps, ["cs2.exe", "valorant.exe"]);
    assert_eq!(reopened.config().unwrap(), store.config().unwrap());
}

#[test]
fn unreadable_file_is_reported_with_its_line_and_never_overwritten() {
    let dir = TempDir::new().unwrap();
    let path = config_path(&dir);
    fs::write(&path, "default_profile = [").unwrap();

    let mut store = Store::open(&path);
    let message = store.config_error().unwrap();
    assert!(message.contains(path.to_str().unwrap()), "{message}");
    assert!(message.contains("line 1"), "{message}");
    assert_eq!(kind(store.config()), ErrorKind::ConfigUnavailable);

    assert_eq!(
        kind(store.save_profile("fps", ProfileSettings::default())),
        ErrorKind::ConfigUnavailable
    );
    assert_eq!(
        kind(store.delete_profile("fps")),
        ErrorKind::ConfigUnavailable
    );
    assert_eq!(kind(store.set_default("fps")), ErrorKind::ConfigUnavailable);
    assert_eq!(
        kind(store.link_app("fps", "cs2.exe")),
        ErrorKind::ConfigUnavailable
    );
    assert_eq!(
        kind(store.unlink_app("cs2.exe")),
        ErrorKind::ConfigUnavailable
    );
    assert_eq!(
        kind(store.set_audio_switch(true)),
        ErrorKind::ConfigUnavailable
    );

    assert_eq!(fs::read_to_string(&path).unwrap(), "default_profile = [");
    assert!(!dir.path().join("config.toml.tmp").exists());
}

#[test]
fn linking_an_app_moves_it_between_profiles() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(&config_path(&dir));
    store
        .save_profile("fps", ProfileSettings::default())
        .unwrap();
    store
        .save_profile("media", ProfileSettings::default())
        .unwrap();

    store.link_app("fps", "CS2.exe").unwrap();
    assert_eq!(store.profile_for_app("cs2.exe"), Some("fps"));
    store.link_app("media", "CS2.exe").unwrap();

    assert_eq!(store.profile_for_app("cs2.exe"), Some("media"));
    assert_eq!(store.profile_for_app("CS2.EXE"), Some("media"));
    assert!(store.config().unwrap().profiles["fps"].apps.is_empty());
}

#[test]
fn duplicate_app_in_hand_edited_file_resolves_to_the_first_profile_by_name() {
    let dir = TempDir::new().unwrap();
    let path = config_path(&dir);
    fs::write(
        &path,
        r#"
default_profile = "default"

[profiles.zeta]
apps = ["game.exe"]

[profiles.alpha]
apps = ["Game.exe"]
"#,
    )
    .unwrap();

    let mut store = Store::open(&path);
    assert_eq!(store.config_error(), None);
    assert_eq!(store.profile_for_app("game.exe"), Some("alpha"));
    assert_eq!(store.profile_for_app("GAME.EXE"), Some("alpha"));

    store.link_app("zeta", "game.exe").unwrap();
    assert_eq!(store.profile_for_app("game.exe"), Some("zeta"));
    assert!(store.config().unwrap().profiles["alpha"].apps.is_empty());
}

#[test]
fn default_profile_cannot_be_deleted() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(&config_path(&dir));
    let err = store.delete_profile("default").unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidValue);
    assert!(err.message.contains("default profile cannot be deleted"));
    assert!(store.config().unwrap().profiles.contains_key("default"));

    store
        .save_profile("fps", ProfileSettings::default())
        .unwrap();
    store.set_default("fps").unwrap();
    store.delete_profile("default").unwrap();
    assert_eq!(kind(store.delete_profile("fps")), ErrorKind::InvalidValue);
}

#[test]
fn bad_profile_name_is_invalid_value() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(&config_path(&dir));
    let too_long = "a".repeat(65);
    for name in [
        "",
        " lead",
        "trail ",
        "semi;colon",
        "dot.dot",
        "slash/",
        "\u{e9}",
        &too_long,
    ] {
        assert_eq!(
            kind(store.save_profile(name, ProfileSettings::default())),
            ErrorKind::InvalidValue,
            "{name:?}"
        );
    }
    let longest = "a".repeat(64);
    for name in ["Night mode", "a_b-c", "x", &longest] {
        store
            .save_profile(name, ProfileSettings::default())
            .unwrap();
    }
}

#[test]
fn save_is_atomic() {
    let dir = TempDir::new().unwrap();
    let path = config_path(&dir);
    save(&path, &Config::default()).unwrap();
    save(&path, &Config::default()).unwrap();
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["config.toml"]);
}

#[test]
fn failed_save_leaves_memory_unchanged_and_reports_config_unavailable() {
    let dir = TempDir::new().unwrap();
    let blocker = dir.path().join("blocker");
    let path = blocker.join("config.toml");
    let mut store = Store::open(&path);
    fs::write(&blocker, "not a directory").unwrap();

    let err = store
        .save_profile("fps", ProfileSettings::default())
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::ConfigUnavailable);
    assert!(
        err.message.contains(path.to_str().unwrap()),
        "{}",
        err.message
    );
    assert_eq!(store.config().unwrap(), &Config::default());
    assert_eq!(store.config_error(), None);
}

#[test]
fn unknown_profile_errors() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(&config_path(&dir));
    assert_eq!(kind(store.set_default("nope")), ErrorKind::NoSuchProfile);
    assert_eq!(kind(store.delete_profile("nope")), ErrorKind::NoSuchProfile);
    assert_eq!(
        kind(store.link_app("nope", "cs2.exe")),
        ErrorKind::NoSuchProfile
    );
    assert_eq!(store.unlink_app("nobody.exe"), Ok(()));
}

#[test]
fn app_names_are_trimmed_lowercased_and_must_not_be_empty() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(&config_path(&dir));
    store
        .save_profile("fps", ProfileSettings::default())
        .unwrap();
    store.link_app("fps", "  CS2.EXE ").unwrap();
    assert_eq!(store.config().unwrap().profiles["fps"].apps, ["cs2.exe"]);
    assert_eq!(kind(store.link_app("fps", "   ")), ErrorKind::InvalidValue);
    assert_eq!(kind(store.unlink_app("")), ErrorKind::InvalidValue);

    store.unlink_app(" Cs2.Exe").unwrap();
    assert_eq!(store.profile_for_app("cs2.exe"), None);
}

#[test]
fn save_profile_keeps_existing_app_links() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(&config_path(&dir));
    store
        .save_profile("fps", ProfileSettings::default())
        .unwrap();
    store.link_app("fps", "cs2.exe").unwrap();
    let settings = ProfileSettings {
        balance: Some(6),
        ..ProfileSettings::default()
    };
    store.save_profile("fps", settings.clone()).unwrap();

    let fps = &store.config().unwrap().profiles["fps"];
    assert_eq!(fps.apps, ["cs2.exe"]);
    assert_eq!(fps.settings, settings);
}

#[test]
fn default_profile_missing_from_file_is_added_in_memory() {
    let dir = TempDir::new().unwrap();
    let path = config_path(&dir);
    fs::write(&path, "default_profile = \"gaming\"\n").unwrap();
    let store = Store::open(&path);
    assert_eq!(store.config_error(), None);
    assert!(store.config().unwrap().profiles.contains_key("gaming"));
}

#[test]
fn paths_in_dir_name_the_three_files() {
    let paths = Paths::in_dir(std::path::Path::new("/x"));
    assert_eq!(paths.config_file, PathBuf::from("/x/config.toml"));
    assert_eq!(paths.state_file, PathBuf::from("/x/state.toml"));
    assert_eq!(paths.log_file, PathBuf::from("/x/daemon.log"));
}

#[test]
fn paths_for_current_user_use_the_app_name() {
    let paths = Paths::for_current_user().unwrap();
    assert!(paths.config_file.ends_with("config.toml"));
    assert!(paths.state_file.ends_with("state.toml"));
    assert!(paths.log_file.ends_with("daemon.log"));
    assert!(paths.config_file.to_string_lossy().contains("open-g-hub"));
}

#[test]
fn hand_edit_between_two_changes_survives_the_second() {
    let dir = TempDir::new().unwrap();
    let path = config_path(&dir);
    let mut store = Store::open(&path);
    store
        .save_profile("fps", ProfileSettings::default())
        .unwrap();

    let mut edited = match load(&path) {
        Loaded::Ok(config) => config,
        other => panic!("expected a config, got {other:?}"),
    };
    edited.audio_switch.game_output = "Speakers".to_string();
    edited
        .profiles
        .insert("music".to_string(), Default::default());
    save(&path, &edited).unwrap();

    store.link_app("fps", "cs2.exe").unwrap();

    let Loaded::Ok(on_disk) = load(&path) else {
        panic!("the file should load");
    };
    assert_eq!(on_disk.audio_switch.game_output, "Speakers");
    assert!(on_disk.profiles.contains_key("music"));
    assert_eq!(on_disk.profiles["fps"].apps, vec!["cs2.exe".to_string()]);
    assert_eq!(store.config().unwrap(), &on_disk);
}

#[test]
fn file_broken_while_running_is_not_overwritten_until_fixed() {
    let dir = TempDir::new().unwrap();
    let path = config_path(&dir);
    let mut store = Store::open(&path);
    store
        .save_profile("fps", ProfileSettings::default())
        .unwrap();
    let good = fs::read_to_string(&path).unwrap();

    fs::write(&path, "default_profile = [").unwrap();
    assert_eq!(
        kind(store.set_audio_switch(true)),
        ErrorKind::ConfigUnavailable
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "default_profile = [");
    assert!(store.config_error().unwrap().contains("line 1"));

    fs::write(&path, &good).unwrap();
    store.set_audio_switch(true).unwrap();
    assert_eq!(store.config_error(), None);
    assert!(store.config().unwrap().audio_switch.enabled);
    assert!(store.config().unwrap().profiles.contains_key("fps"));
}

#[test]
fn file_deleted_while_running_starts_again_from_defaults() {
    let dir = TempDir::new().unwrap();
    let path = config_path(&dir);
    let mut store = Store::open(&path);
    store
        .save_profile("fps", ProfileSettings::default())
        .unwrap();
    fs::remove_file(&path).unwrap();

    store.set_audio_switch(true).unwrap();
    let Loaded::Ok(on_disk) = load(&path) else {
        panic!("the file should load");
    };
    assert!(!on_disk.profiles.contains_key("fps"));
    assert!(on_disk.audio_switch.enabled);
}
