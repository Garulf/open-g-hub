//! The (feature id, function) pairs this crate may send, from `allowlist.toml`.

use std::collections::HashSet;
use std::fmt;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::FeatureId;

const EMBEDDED: &str = include_str!("../allowlist.toml");
const MAX_FUNCTION: u8 = 0x0f;

static PARSED: LazyLock<Result<Allowlist, AllowlistError>> =
    LazyLock::new(|| Allowlist::parse(EMBEDDED));

/// A call G Hub was seen making, with the fixture (`<session dir>/<label>`)
/// that shows it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllowEntry {
    pub feature: u16,
    pub function: u8,
    pub seen_in: String,
}

/// A call that must never be sent. Without `function`, the whole feature.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DenyEntry {
    pub feature: u16,
    pub function: Option<u8>,
    pub reason: String,
}

impl DenyEntry {
    fn covers(&self, feature: u16, function: u8) -> bool {
        self.feature == feature && self.function.is_none_or(|denied| denied == function)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllowlistError(String);

impl fmt::Display for AllowlistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid allowlist: {}", self.0)
    }
}

impl std::error::Error for AllowlistError {}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AllowlistFile {
    #[serde(default)]
    allow: Vec<AllowEntry>,
    #[serde(default)]
    deny: Vec<DenyEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allowlist {
    allow: Vec<AllowEntry>,
    deny: Vec<DenyEntry>,
}

impl Allowlist {
    /// Rejects a list where an allow entry hits a deny entry, a function does
    /// not fit in four bits, an entry repeats, or a citation or reason is empty.
    pub fn parse(text: &str) -> Result<Self, AllowlistError> {
        let file: AllowlistFile =
            toml::from_str(text).map_err(|e| AllowlistError(e.to_string()))?;
        validate(&file)?;
        Ok(Self {
            allow: file.allow,
            deny: file.deny,
        })
    }

    /// The list compiled into this crate from `allowlist.toml`.
    pub fn embedded() -> Result<&'static Self, AllowlistError> {
        PARSED.as_ref().map_err(Clone::clone)
    }

    pub fn allowed(&self) -> &[AllowEntry] {
        &self.allow
    }

    pub fn denied(&self) -> &[DenyEntry] {
        &self.deny
    }

    pub fn permits(&self, feature: FeatureId, function: u8) -> bool {
        let FeatureId(id) = feature;
        let listed = self
            .allow
            .iter()
            .any(|entry| entry.feature == id && entry.function == function);
        listed && !self.deny.iter().any(|entry| entry.covers(id, function))
    }
}

fn validate(file: &AllowlistFile) -> Result<(), AllowlistError> {
    let mut seen = HashSet::new();
    for entry in &file.allow {
        let call = format!("feature {:#06x} function {}", entry.feature, entry.function);
        if entry.function > MAX_FUNCTION {
            return Err(AllowlistError(format!("{call}: function above 15")));
        }
        if entry.seen_in.trim().is_empty() {
            return Err(AllowlistError(format!("{call}: no seen_in citation")));
        }
        if !seen.insert((entry.feature, entry.function)) {
            return Err(AllowlistError(format!("{call}: listed twice")));
        }
        if let Some(deny) = file
            .deny
            .iter()
            .find(|deny| deny.covers(entry.feature, entry.function))
        {
            return Err(AllowlistError(format!(
                "{call} is allowed but denied: {}",
                deny.reason
            )));
        }
    }
    for entry in &file.deny {
        if entry
            .function
            .is_some_and(|function| function > MAX_FUNCTION)
        {
            return Err(AllowlistError(format!(
                "deny entry for feature {:#06x}: function above 15",
                entry.feature
            )));
        }
        if entry.reason.trim().is_empty() {
            return Err(AllowlistError(format!(
                "deny entry for feature {:#06x} has no reason",
                entry.feature
            )));
        }
    }
    Ok(())
}
