use std::io::ErrorKind;
use std::process::Command;

use serde_json::Value;

use crate::{AudioError, AudioRouter, Defaults, Direction, Role, Slot};

const SLOTS: &[Slot] = &[
    Slot::new(Direction::Render, Role::Console),
    Slot::new(Direction::Capture, Role::Console),
];

pub struct PipeWireRouter;

impl AudioRouter for PipeWireRouter {
    fn slots(&self) -> &'static [Slot] {
        SLOTS
    }

    fn current(&self) -> Result<Defaults, AudioError> {
        Ok(defaults(&dump()?))
    }

    fn find(
        &self,
        direction: Direction,
        name_contains: &str,
    ) -> Result<Option<String>, AudioError> {
        Ok(find_node(&dump()?, direction, name_contains))
    }

    fn set(&self, _slot: Slot, endpoint: &str) -> Result<(), AudioError> {
        let args = wpctl_args(&dump()?, endpoint)
            .ok_or_else(|| AudioError::Failed(format!("audio endpoint {endpoint} not found")))?;
        run("wpctl", &args).map(|_| ())
    }
}

/// The `node.name` of the first node of the right class whose description,
/// then nick, contains `name_contains` ignoring case.
pub fn find_node(dump: &Value, direction: Direction, name_contains: &str) -> Option<String> {
    let class = match direction {
        Direction::Render => "Audio/Sink",
        Direction::Capture => "Audio/Source",
    };
    let needle = name_contains.to_lowercase();
    ["node.description", "node.nick"].iter().find_map(|key| {
        nodes(dump).find_map(|props| {
            let matches = prop(props, "media.class") == Some(class)
                && prop(props, key).is_some_and(|text| text.to_lowercase().contains(&needle));
            matches.then(|| prop(props, "node.name").map(str::to_string))?
        })
    })
}

/// The default sink and source named by the `default` metadata object.
pub fn defaults(dump: &Value) -> Defaults {
    let mut found = Defaults::new();
    let entries = objects(dump)
        .filter(|object| object["props"]["metadata.name"] == "default")
        .filter_map(|object| object["metadata"].as_array())
        .flatten();
    for entry in entries {
        let slot = match entry["key"].as_str() {
            Some("default.audio.sink") => Slot::new(Direction::Render, Role::Console),
            Some("default.audio.source") => Slot::new(Direction::Capture, Role::Console),
            _ => continue,
        };
        if let Some(name) = default_name(&entry["value"]) {
            found.insert(slot, name);
        }
    }
    found
}

fn default_name(value: &Value) -> Option<String> {
    let parsed;
    let value = match value {
        Value::String(text) => {
            parsed = serde_json::from_str::<Value>(text).ok()?;
            &parsed
        }
        other => other,
    };
    value["name"].as_str().map(str::to_string)
}

/// The `wpctl` arguments that make the node called `node_name` the default.
pub fn wpctl_args(dump: &Value, node_name: &str) -> Option<Vec<String>> {
    let id = objects(dump)
        .filter(|object| object["type"] == "PipeWire:Interface:Node")
        .find(|object| prop(&object["info"]["props"], "node.name") == Some(node_name))?["id"]
        .as_u64()?;
    Some(vec!["set-default".to_string(), id.to_string()])
}

fn objects(dump: &Value) -> impl Iterator<Item = &Value> {
    dump.as_array().into_iter().flatten()
}

fn nodes(dump: &Value) -> impl Iterator<Item = &Value> {
    objects(dump)
        .filter(|object| object["type"] == "PipeWire:Interface:Node")
        .map(|object| &object["info"]["props"])
}

fn prop<'a>(props: &'a Value, key: &str) -> Option<&'a str> {
    props[key].as_str()
}

fn dump() -> Result<Value, AudioError> {
    let output = run("pw-dump", &[])?;
    serde_json::from_slice(&output)
        .map_err(|e| AudioError::Failed(format!("pw-dump gave unreadable output: {e}")))
}

fn run(program: &str, args: &[String]) -> Result<Vec<u8>, AudioError> {
    let output = Command::new(program).args(args).output().map_err(|e| {
        if e.kind() == ErrorKind::NotFound {
            AudioError::Unavailable(format!("{program} not found on PATH"))
        } else {
            AudioError::Failed(format!("cannot run {program}: {e}"))
        }
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AudioError::Failed(format!(
            "{program} failed ({}): {}",
            output.status,
            stderr.trim()
        )));
    }
    Ok(output.stdout)
}
