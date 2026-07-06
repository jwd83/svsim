//! Four-state runtime values and the primitive logic operations on them.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ObjectValue {
    pub(super) logic: LogicValue,
}

impl ObjectValue {
    pub(super) fn zero(width: usize) -> Self {
        Self {
            logic: LogicValue::zero(width),
        }
    }

    pub(super) fn from_logic(logic: LogicValue) -> Self {
        Self { logic }
    }
}

pub(super) fn logic_inputs_from_public_bits(
    inputs: &BTreeMap<String, BitValue>,
) -> BTreeMap<String, LogicValue> {
    inputs
        .iter()
        .map(|(name, value)| (name.clone(), LogicValue::from(value.clone())))
        .collect()
}

pub(super) fn logic_outputs_to_public_bits(
    logic_outputs: BTreeMap<String, LogicValue>,
) -> Result<BTreeMap<String, BitValue>> {
    let mut outputs = BTreeMap::new();

    for (name, logic) in logic_outputs {
        outputs.insert(
            name.clone(),
            logic_to_public_bit_value(&logic, format!("output '{}'", name))?,
        );
    }

    Ok(outputs)
}

pub(super) fn logic_to_public_bit_value(logic: &LogicValue, context: String) -> Result<BitValue> {
    logic
        .to_bit_value_checked()
        .ok_or_else(|| {
            Error::Unsupported(format!(
                "{} resolved to four-state value '{}' and cannot be represented through the explicit 2-state wrapper",
                context, logic
            ))
        })
        .map(|bits| bits.truncate(logic.width()))
}
