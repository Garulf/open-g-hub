//! Parameter checks for allowlisted calls whose safety depends on their
//! params (ruling R22). A call with no rule here is checked on (feature,
//! function) alone.

use crate::FeatureId;

/// The only custom EQ slot on the A50 X: getEQInfos reports `0x31`, three
/// read-only factory presets (0..2) and one custom slot.
const CUSTOM_EQ_SLOT: u8 = 3;

struct ParamRule {
    feature: FeatureId,
    function: u8,
    accepts: fn(&[u8]) -> bool,
}

const RULES: &[ParamRule] = &[ParamRule {
    feature: FeatureId::ADVANCED_PARA_EQ,
    function: 2,
    accepts: set_custom_eq_targets_custom_slot,
}];

/// setCustomEQ params are (instance, slot, ...); factory slots must never
/// be written.
fn set_custom_eq_targets_custom_slot(params: &[u8]) -> bool {
    params.get(1) == Some(&CUSTOM_EQ_SLOT)
}

pub(crate) fn permits(feature: FeatureId, function: u8, params: &[u8]) -> bool {
    RULES
        .iter()
        .filter(|rule| rule.feature == feature && rule.function == function)
        .all(|rule| (rule.accepts)(params))
}
