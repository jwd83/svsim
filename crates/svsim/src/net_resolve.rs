#![cfg_attr(not(test), allow(dead_code))]

use std::error::Error as StdError;
use std::fmt;

use crate::hir::NetKind;
use crate::logic_value::{LogicBit, LogicBits, LogicValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DriveStrength {
    Weak,
    Pull,
    Strong,
    Supply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DriveStrengthPair {
    pub zero: DriveStrength,
    pub one: DriveStrength,
}

impl DriveStrengthPair {
    pub(crate) const STRONG: Self = Self::new(DriveStrength::Strong, DriveStrength::Strong);
    pub(crate) const WEAK: Self = Self::new(DriveStrength::Weak, DriveStrength::Weak);

    pub(crate) const fn new(zero: DriveStrength, one: DriveStrength) -> Self {
        Self { zero, one }
    }

    fn max(self) -> DriveStrength {
        self.zero.max(self.one)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NetDriver {
    value: LogicValue,
    strengths: DriveStrengthPair,
}

impl NetDriver {
    pub(crate) fn new(value: LogicValue, strengths: DriveStrengthPair) -> Self {
        Self { value, strengths }
    }

    fn coerced_value(&self, width: usize) -> LogicValue {
        self.value.coerced_to(width)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NetResolveError {
    message: String,
}

impl NetResolveError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for NetResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl StdError for NetResolveError {}

pub(crate) fn resolve_net(
    kind: NetKind,
    width: usize,
    previous: Option<&LogicValue>,
    drivers: &[NetDriver],
) -> Result<LogicValue, NetResolveError> {
    let width = width.max(1);
    let previous = previous.map(|value| value.coerced_to(width));
    let coerced_drivers: Vec<LogicValue> = drivers
        .iter()
        .map(|driver| driver.coerced_value(width))
        .collect();
    let mut bits = LogicBits::zero();

    for index in 0..width {
        let bit = resolve_net_bit(
            kind,
            previous.as_ref().map(|value| value.bit(index)),
            drivers,
            &coerced_drivers,
            index,
        )?;
        bits.set_bit(index, bit);
    }

    Ok(LogicValue::new(bits, width))
}

fn resolve_net_bit(
    kind: NetKind,
    previous: Option<LogicBit>,
    drivers: &[NetDriver],
    coerced_drivers: &[LogicValue],
    index: usize,
) -> Result<LogicBit, NetResolveError> {
    match kind {
        NetKind::Wand | NetKind::Triand => resolve_wired_and_bit(drivers, coerced_drivers, index),
        NetKind::Wor | NetKind::Trior => resolve_wired_or_bit(drivers, coerced_drivers, index),
        NetKind::Trireg => resolve_regular_net_bit(kind, previous, drivers, coerced_drivers, index),
        _ => resolve_regular_net_bit(kind, None, drivers, coerced_drivers, index),
    }
}

fn resolve_regular_net_bit(
    kind: NetKind,
    previous: Option<LogicBit>,
    drivers: &[NetDriver],
    coerced_drivers: &[LogicValue],
    index: usize,
) -> Result<LogicBit, NetResolveError> {
    let mut zero_strength = None;
    let mut one_strength = None;
    let mut active_driver_count = 0usize;

    if let Some((bit, strength)) = implicit_regular_driver(kind) {
        apply_strength(bit, strength, &mut zero_strength, &mut one_strength);
        active_driver_count += 1;
    }

    for (driver, value) in drivers.iter().zip(coerced_drivers) {
        match value.bit(index) {
            LogicBit::Zero => {
                apply_strength(
                    LogicBit::Zero,
                    driver.strengths.zero,
                    &mut zero_strength,
                    &mut one_strength,
                );
                active_driver_count += 1;
            }
            LogicBit::One => {
                apply_strength(
                    LogicBit::One,
                    driver.strengths.one,
                    &mut zero_strength,
                    &mut one_strength,
                );
                active_driver_count += 1;
            }
            LogicBit::X => {
                let strength = driver.strengths.max();
                apply_strength(LogicBit::Zero, strength, &mut zero_strength, &mut one_strength);
                apply_strength(LogicBit::One, strength, &mut zero_strength, &mut one_strength);
                active_driver_count += 1;
            }
            LogicBit::Z => {}
        }
    }

    if matches!(kind, NetKind::Uwire) && active_driver_count > 1 {
        return Err(NetResolveError::new(format!(
            "uwire bit {} has multiple active drivers",
            index
        )));
    }

    let resolved = match (zero_strength, one_strength) {
        (Some(zero), Some(one)) if zero > one => LogicBit::Zero,
        (Some(zero), Some(one)) if one > zero => LogicBit::One,
        (Some(_), Some(_)) => LogicBit::X,
        (Some(_), None) => LogicBit::Zero,
        (None, Some(_)) => LogicBit::One,
        (None, None) => match kind {
            NetKind::Trireg => previous.unwrap_or(LogicBit::Z),
            _ => LogicBit::Z,
        },
    };

    Ok(resolved)
}

fn resolve_wired_and_bit(
    drivers: &[NetDriver],
    coerced_drivers: &[LogicValue],
    index: usize,
) -> Result<LogicBit, NetResolveError> {
    let mut saw_one = false;
    let mut saw_x = false;

    for (_driver, value) in drivers.iter().zip(coerced_drivers) {
        match value.bit(index) {
            LogicBit::Zero => return Ok(LogicBit::Zero),
            LogicBit::One => saw_one = true,
            LogicBit::X => saw_x = true,
            LogicBit::Z => {}
        }
    }

    if saw_x {
        Ok(LogicBit::X)
    } else if saw_one {
        Ok(LogicBit::One)
    } else {
        Ok(LogicBit::Z)
    }
}

fn resolve_wired_or_bit(
    drivers: &[NetDriver],
    coerced_drivers: &[LogicValue],
    index: usize,
) -> Result<LogicBit, NetResolveError> {
    let mut saw_zero = false;
    let mut saw_x = false;

    for (_driver, value) in drivers.iter().zip(coerced_drivers) {
        match value.bit(index) {
            LogicBit::One => return Ok(LogicBit::One),
            LogicBit::Zero => saw_zero = true,
            LogicBit::X => saw_x = true,
            LogicBit::Z => {}
        }
    }

    if saw_x {
        Ok(LogicBit::X)
    } else if saw_zero {
        Ok(LogicBit::Zero)
    } else {
        Ok(LogicBit::Z)
    }
}

fn implicit_regular_driver(kind: NetKind) -> Option<(LogicBit, DriveStrength)> {
    match kind {
        NetKind::Tri0 => Some((LogicBit::Zero, DriveStrength::Pull)),
        NetKind::Tri1 => Some((LogicBit::One, DriveStrength::Pull)),
        NetKind::Supply0 => Some((LogicBit::Zero, DriveStrength::Supply)),
        NetKind::Supply1 => Some((LogicBit::One, DriveStrength::Supply)),
        NetKind::Trireg
        | NetKind::Tri
        | NetKind::Uwire
        | NetKind::Wire
        | NetKind::Wand
        | NetKind::Wor
        | NetKind::Triand
        | NetKind::Trior => None,
    }
}

fn apply_strength(
    bit: LogicBit,
    strength: DriveStrength,
    zero_strength: &mut Option<DriveStrength>,
    one_strength: &mut Option<DriveStrength>,
) {
    match bit {
        LogicBit::Zero => update_strength(zero_strength, strength),
        LogicBit::One => update_strength(one_strength, strength),
        LogicBit::X => {
            update_strength(zero_strength, strength);
            update_strength(one_strength, strength);
        }
        LogicBit::Z => {}
    }
}

fn update_strength(slot: &mut Option<DriveStrength>, next: DriveStrength) {
    match slot {
        Some(current) => *current = (*current).max(next),
        None => *slot = Some(next),
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_net, DriveStrength, DriveStrengthPair, NetDriver};
    use crate::hir::NetKind;
    use crate::logic_value::LogicValue;

    fn logic(text: &str) -> LogicValue {
        LogicValue::from_logic_str(text).expect("parse logic value")
    }

    fn driver(text: &str) -> NetDriver {
        NetDriver::new(logic(text), DriveStrengthPair::STRONG)
    }

    fn weak_driver(text: &str) -> NetDriver {
        NetDriver::new(logic(text), DriveStrengthPair::WEAK)
    }

    #[test]
    fn wire_floats_to_z_when_undriven() {
        let resolved = resolve_net(NetKind::Wire, 4, None, &[]).expect("resolve wire");

        assert_eq!(resolved, logic("zzzz"));
    }

    #[test]
    fn wire_conflict_with_equal_strengths_resolves_to_x() {
        let resolved = resolve_net(NetKind::Wire, 1, None, &[driver("0"), driver("1")])
            .expect("resolve wire conflict");

        assert_eq!(resolved, logic("x"));
    }

    #[test]
    fn stronger_drive_wins_on_plain_wire() {
        let resolved = resolve_net(
            NetKind::Wire,
            1,
            None,
            &[
                weak_driver("0"),
                NetDriver::new(
                    logic("1"),
                    DriveStrengthPair::new(DriveStrength::Strong, DriveStrength::Strong),
                ),
            ],
        )
        .expect("resolve strong wire");

        assert_eq!(resolved, logic("1"));
    }

    #[test]
    fn tri1_pull_up_only_applies_when_not_overdriven() {
        let idle = resolve_net(NetKind::Tri1, 1, None, &[]).expect("resolve idle tri1");
        let driven = resolve_net(NetKind::Tri1, 1, None, &[driver("0")]).expect("resolve tri1");

        assert_eq!(idle, logic("1"));
        assert_eq!(driven, logic("0"));
    }

    #[test]
    fn supply_strength_beats_strong_driver() {
        let resolved = resolve_net(NetKind::Supply0, 1, None, &[driver("1")])
            .expect("resolve supply0");

        assert_eq!(resolved, logic("0"));
    }

    #[test]
    fn wand_and_wor_apply_wired_logic_rules() {
        let wand = resolve_net(NetKind::Wand, 1, None, &[driver("1"), driver("0")])
            .expect("resolve wand");
        let wor = resolve_net(NetKind::Wor, 1, None, &[driver("0"), driver("1")])
            .expect("resolve wor");

        assert_eq!(wand, logic("0"));
        assert_eq!(wor, logic("1"));
    }

    #[test]
    fn trireg_holds_previous_value_when_floating() {
        let resolved =
            resolve_net(NetKind::Trireg, 3, Some(&logic("101")), &[]).expect("resolve trireg");

        assert_eq!(resolved, logic("101"));
    }

    #[test]
    fn uwire_rejects_multiple_active_drivers() {
        let error =
            resolve_net(NetKind::Uwire, 1, None, &[driver("0"), weak_driver("1")]).unwrap_err();

        assert!(
            error.to_string().contains("multiple active drivers"),
            "unexpected error: {error}"
        );
    }
}
