use std::collections::BTreeMap;

use ogh_audio::pipewire::{defaults, find_node, wpctl_args};
use ogh_audio::{Direction, Role, Slot};
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/pw-dump.json")).unwrap()
}

#[test]
fn finds_the_a50x_sink_by_description() {
    let dump = fixture();
    assert_eq!(
        find_node(&dump, Direction::Render, "a50 x").as_deref(),
        Some("alsa_output.usb-Astro_A50_X-00.analog-stereo")
    );
}

#[test]
fn finds_the_a50x_source_by_nick_when_there_is_no_description() {
    let dump = fixture();
    assert_eq!(
        find_node(&dump, Direction::Capture, "A50 X Mic").as_deref(),
        Some("alsa_input.usb-Astro_A50_X-00.mono-fallback")
    );
}

#[test]
fn description_wins_over_nick_and_streams_are_ignored() {
    let dump = json!([
        node(1, "Audio/Sink", "by_nick", None, Some("Headset")),
        node(2, "Audio/Sink", "by_description", Some("Headset"), None),
        node(3, "Stream/Output/Audio", "stream", Some("Headset"), None),
    ]);
    assert_eq!(
        find_node(&dump, Direction::Render, "headset").as_deref(),
        Some("by_description")
    );
    assert_eq!(find_node(&dump, Direction::Capture, "headset"), None);
}

#[test]
fn reads_the_default_sink_and_source_names() {
    assert_eq!(
        defaults(&fixture()),
        BTreeMap::from([
            (
                Slot::new(Direction::Render, Role::Console),
                "alsa_output.pci-0000_00_1f.3.analog-stereo".to_string()
            ),
            (
                Slot::new(Direction::Capture, Role::Console),
                "alsa_input.pci-0000_00_1f.3.analog-stereo".to_string()
            ),
        ])
    );
}

#[test]
fn defaults_accept_a_value_given_as_a_json_string() {
    let dump = json!([{
        "id": 1,
        "type": "PipeWire:Interface:Metadata",
        "props": { "metadata.name": "default" },
        "metadata": [{
            "subject": 0,
            "key": "default.audio.sink",
            "type": "Spa:String:JSON",
            "value": "{\"name\":\"sink_a\"}"
        }]
    }]);
    assert_eq!(
        defaults(&dump),
        BTreeMap::from([(
            Slot::new(Direction::Render, Role::Console),
            "sink_a".to_string()
        )])
    );
}

#[test]
fn missing_node_is_none() {
    let dump = fixture();
    assert_eq!(find_node(&dump, Direction::Render, "no such headset"), None);
    assert_eq!(wpctl_args(&dump, "no_such_node"), None);
    assert!(defaults(&json!([])).is_empty());
}

#[test]
fn set_default_resolves_the_node_id() {
    let dump = fixture();
    assert_eq!(
        wpctl_args(&dump, "alsa_output.usb-Astro_A50_X-00.analog-stereo"),
        Some(vec!["set-default".to_string(), "57".to_string()])
    );
}

#[test]
fn slots_round_trip_as_strings() {
    let slot = Slot::new(Direction::Capture, Role::Communications);
    assert_eq!(slot.to_string(), "capture.communications");
    assert_eq!("capture.communications".parse::<Slot>(), Ok(slot));
    assert!("render".parse::<Slot>().is_err());
    assert!("render.nope".parse::<Slot>().is_err());
    let map = BTreeMap::from([(slot, "id".to_string())]);
    let text = serde_json::to_string(&map).unwrap();
    assert_eq!(text, r#"{"capture.communications":"id"}"#);
    assert_eq!(
        serde_json::from_str::<BTreeMap<Slot, String>>(&text).unwrap(),
        map
    );
}

fn node(id: u32, class: &str, name: &str, description: Option<&str>, nick: Option<&str>) -> Value {
    let mut props = json!({ "media.class": class, "node.name": name });
    if let Some(description) = description {
        props["node.description"] = json!(description);
    }
    if let Some(nick) = nick {
        props["node.nick"] = json!(nick);
    }
    json!({ "id": id, "type": "PipeWire:Interface:Node", "info": { "props": props } })
}
