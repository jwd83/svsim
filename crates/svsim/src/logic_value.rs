use std::error::Error as StdError;
use std::fmt;
use std::str::FromStr;

use serde::de::{Error as DeError, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::bit_value::{BitValue, ParseBitValueError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogicBit {
    Zero,
    One,
    X,
    Z,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub struct LogicBits {
    ones: BitValue,
    x_mask: BitValue,
    z_mask: BitValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LogicValue {
    bits: LogicBits,
    width: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LogicPattern {
    bits: LogicBits,
    wildcard_mask: BitValue,
    width: usize,
    explicit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseLogicValueError {
    message: String,
}

impl ParseLogicValueError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseLogicValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl StdError for ParseLogicValueError {}

impl LogicBits {
    pub fn zero() -> Self {
        Self::default()
    }

    pub fn from_bit_value(bits: BitValue) -> Self {
        Self {
            ones: bits,
            x_mask: BitValue::zero(),
            z_mask: BitValue::zero(),
        }
    }

    pub fn is_two_state(&self) -> bool {
        self.x_mask.is_zero() && self.z_mask.is_zero()
    }

    pub fn bit(&self, index: usize) -> LogicBit {
        if self.x_mask.get_bit(index) {
            LogicBit::X
        } else if self.z_mask.get_bit(index) {
            LogicBit::Z
        } else if self.ones.get_bit(index) {
            LogicBit::One
        } else {
            LogicBit::Zero
        }
    }

    pub fn to_bit_value_checked(&self) -> Option<BitValue> {
        self.is_two_state().then(|| self.ones.clone())
    }

    fn truncate_in_place(&mut self, width: usize) {
        self.ones.truncate_in_place(width);
        self.x_mask.truncate_in_place(width);
        self.z_mask.truncate_in_place(width);
    }

    fn set_bit(&mut self, index: usize, bit: LogicBit) {
        self.ones.set_bit(index, false);
        self.x_mask.set_bit(index, false);
        self.z_mask.set_bit(index, false);
        match bit {
            LogicBit::Zero => {}
            LogicBit::One => self.ones.set_bit(index, true),
            LogicBit::X => self.x_mask.set_bit(index, true),
            LogicBit::Z => self.z_mask.set_bit(index, true),
        }
    }
}

impl LogicValue {
    pub fn new(bits: LogicBits, width: usize) -> Self {
        let width = width.max(1);
        let mut bits = bits;
        bits.truncate_in_place(width);
        Self { bits, width }
    }

    pub fn zero(width: usize) -> Self {
        Self::new(LogicBits::zero(), width)
    }

    pub fn from_bit_value_with_width(bits: BitValue, width: usize) -> Self {
        Self::new(LogicBits::from_bit_value(bits), width)
    }

    pub fn from_logic_str(text: &str) -> Result<Self, ParseLogicValueError> {
        let (bits, width) = parse_logic_bits(text, false)?;
        Ok(Self::new(bits, width))
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn bit(&self, index: usize) -> LogicBit {
        if index >= self.width {
            LogicBit::Zero
        } else {
            self.bits.bit(index)
        }
    }

    pub fn bits(&self) -> &LogicBits {
        &self.bits
    }

    pub fn is_two_state(&self) -> bool {
        self.bits.is_two_state()
    }

    pub fn to_bit_value_checked(&self) -> Option<BitValue> {
        self.bits
            .to_bit_value_checked()
            .map(|bits| bits.truncate(self.width))
    }

    fn logic_string(&self) -> String {
        render_logic_string(&self.bits, &BitValue::zero(), self.width)
    }
}

impl LogicPattern {
    pub fn from_logic_str(text: &str) -> Result<Self, ParseLogicValueError> {
        let (bits, wildcard_mask, width) = parse_logic_pattern(text)?;
        Ok(Self {
            bits,
            wildcard_mask,
            width: width.max(1),
            explicit: true,
        })
    }

    pub fn matches(&self, actual: &LogicValue) -> bool {
        if !self.is_explicit() {
            let Some(expected_bits) = self.bits.to_bit_value_checked() else {
                return false;
            };
            let Some(actual_bits) = actual.to_bit_value_checked() else {
                return false;
            };
            return expected_bits.truncate(self.width) == actual_bits;
        }

        if self.width != actual.width {
            return false;
        }

        for index in 0..self.width {
            if self.wildcard_mask.get_bit(index) {
                continue;
            }
            if self.bits.bit(index) != actual.bit(index) {
                return false;
            }
        }
        true
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn is_explicit(&self) -> bool {
        self.explicit || !self.wildcard_mask.is_zero() || !self.bits.is_two_state()
    }

    fn logic_string(&self) -> String {
        render_logic_string(&self.bits, &self.wildcard_mask, self.width)
    }
}

impl Serialize for LogicValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(bits) = self.to_bit_value_checked() {
            serialize_bit_value(&bits, serializer)
        } else {
            serializer.serialize_str(&self.logic_string())
        }
    }
}

impl<'de> Deserialize<'de> for LogicValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(LogicValueVisitor)
    }
}

struct LogicValueVisitor;

impl Visitor<'_> for LogicValueVisitor {
    type Value = LogicValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a non-negative integer or a four-state logic string")
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        Ok(LogicValue::from(value))
    }

    fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        Ok(LogicValue::from(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        if value < 0 {
            return Err(E::custom("negative integers are not supported"));
        }
        Ok(LogicValue::from(value as u64))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        value.parse::<LogicValue>().map_err(E::custom)
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        self.visit_str(&value)
    }
}

impl Serialize for LogicPattern {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.is_explicit() {
            serializer.serialize_str(&self.logic_string())
        } else if let Some(bits) = self.bits.to_bit_value_checked() {
            serialize_bit_value(&bits.truncate(self.width), serializer)
        } else {
            serializer.serialize_str(&self.logic_string())
        }
    }
}

impl<'de> Deserialize<'de> for LogicPattern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(LogicPatternVisitor)
    }
}

struct LogicPatternVisitor;

impl Visitor<'_> for LogicPatternVisitor {
    type Value = LogicPattern;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a non-negative integer or a logic expectation string")
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        Ok(LogicPattern::from(value))
    }

    fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        Ok(LogicPattern::from(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        if value < 0 {
            return Err(E::custom("negative integers are not supported"));
        }
        Ok(LogicPattern::from(value as u64))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        value.parse::<LogicPattern>().map_err(E::custom)
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        self.visit_str(&value)
    }
}

impl fmt::Display for LogicValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(bits) = self.to_bit_value_checked() {
            write!(f, "{bits}")
        } else {
            f.write_str(&self.logic_string())
        }
    }
}

impl fmt::Display for LogicPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_explicit() {
            f.write_str(&self.logic_string())
        } else if let Some(bits) = self.bits.to_bit_value_checked() {
            write!(f, "{}", bits.truncate(self.width))
        } else {
            f.write_str(&self.logic_string())
        }
    }
}

impl FromStr for LogicValue {
    type Err = ParseLogicValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match BitValue::from_prefixed_str(s) {
            Ok(bits) => Ok(Self::from(bits)),
            Err(_) => Self::from_logic_str(s),
        }
    }
}

impl FromStr for LogicPattern {
    type Err = ParseLogicValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match BitValue::from_prefixed_str(s) {
            Ok(bits) => Ok(Self::from(bits)),
            Err(_) => Self::from_logic_str(s),
        }
    }
}

impl From<BitValue> for LogicValue {
    fn from(value: BitValue) -> Self {
        let width = value.bit_len().max(1);
        Self::from_bit_value_with_width(value, width)
    }
}

impl From<u64> for LogicValue {
    fn from(value: u64) -> Self {
        Self::from(BitValue::from(value))
    }
}

impl From<u128> for LogicValue {
    fn from(value: u128) -> Self {
        Self::from(BitValue::from(value))
    }
}

impl From<usize> for LogicValue {
    fn from(value: usize) -> Self {
        Self::from(value as u64)
    }
}

impl From<bool> for LogicValue {
    fn from(value: bool) -> Self {
        Self::from(BitValue::from(value))
    }
}

impl TryFrom<LogicValue> for BitValue {
    type Error = ParseLogicValueError;

    fn try_from(value: LogicValue) -> Result<Self, Self::Error> {
        value.to_bit_value_checked().ok_or_else(|| {
            ParseLogicValueError::new(format!(
                "logic value '{}' contains x/z and cannot be converted to a 2-state integer",
                value
            ))
        })
    }
}

impl TryFrom<&LogicValue> for BitValue {
    type Error = ParseLogicValueError;

    fn try_from(value: &LogicValue) -> Result<Self, Self::Error> {
        value.to_bit_value_checked().ok_or_else(|| {
            ParseLogicValueError::new(format!(
                "logic value '{}' contains x/z and cannot be converted to a 2-state integer",
                value
            ))
        })
    }
}

impl From<LogicValue> for LogicPattern {
    fn from(value: LogicValue) -> Self {
        Self {
            bits: value.bits,
            wildcard_mask: BitValue::zero(),
            width: value.width,
            explicit: false,
        }
    }
}

impl From<BitValue> for LogicPattern {
    fn from(value: BitValue) -> Self {
        LogicValue::from(value).into()
    }
}

impl From<u64> for LogicPattern {
    fn from(value: u64) -> Self {
        LogicValue::from(value).into()
    }
}

impl From<u128> for LogicPattern {
    fn from(value: u128) -> Self {
        LogicValue::from(value).into()
    }
}

impl From<usize> for LogicPattern {
    fn from(value: usize) -> Self {
        LogicValue::from(value).into()
    }
}

impl From<bool> for LogicPattern {
    fn from(value: bool) -> Self {
        LogicValue::from(value).into()
    }
}

fn parse_logic_bits(
    text: &str,
    allow_wildcard: bool,
) -> Result<(LogicBits, usize), ParseLogicValueError> {
    let cleaned = text.trim().replace('_', "");
    if cleaned.is_empty() {
        return Err(ParseLogicValueError::new(
            "empty string is not a valid logic value",
        ));
    }

    let mut bits = LogicBits::zero();
    let mut width = 0usize;
    for (index, ch) in cleaned.chars().rev().enumerate() {
        width = index + 1;
        let bit = match ch {
            '0' => LogicBit::Zero,
            '1' => LogicBit::One,
            'x' | 'X' => LogicBit::X,
            'z' | 'Z' => LogicBit::Z,
            '?' if allow_wildcard => LogicBit::Zero,
            _ => {
                return Err(ParseLogicValueError::new(format!(
                    "invalid logic digit '{}' in '{}'",
                    ch, text
                )));
            }
        };
        bits.set_bit(index, bit);
    }

    Ok((bits, width))
}

fn parse_logic_pattern(text: &str) -> Result<(LogicBits, BitValue, usize), ParseLogicValueError> {
    let cleaned = text.trim().replace('_', "");
    if cleaned.is_empty() {
        return Err(ParseLogicValueError::new(
            "empty string is not a valid logic pattern",
        ));
    }

    let mut bits = LogicBits::zero();
    let mut wildcard_mask = BitValue::zero();
    let mut width = 0usize;

    for (index, ch) in cleaned.chars().rev().enumerate() {
        width = index + 1;
        match ch {
            '0' => bits.set_bit(index, LogicBit::Zero),
            '1' => bits.set_bit(index, LogicBit::One),
            'x' | 'X' => bits.set_bit(index, LogicBit::X),
            'z' | 'Z' => bits.set_bit(index, LogicBit::Z),
            '?' => wildcard_mask.set_bit(index, true),
            _ => {
                return Err(ParseLogicValueError::new(format!(
                    "invalid logic pattern digit '{}' in '{}'",
                    ch, text
                )));
            }
        }
    }

    Ok((bits, wildcard_mask, width))
}

fn render_logic_string(bits: &LogicBits, wildcard_mask: &BitValue, width: usize) -> String {
    let mut rendered = String::with_capacity(width.max(1));
    for index in (0..width.max(1)).rev() {
        let ch = if wildcard_mask.get_bit(index) {
            '?'
        } else {
            match bits.bit(index) {
                LogicBit::Zero => '0',
                LogicBit::One => '1',
                LogicBit::X => 'x',
                LogicBit::Z => 'z',
            }
        };
        rendered.push(ch);
    }
    rendered
}

fn serialize_bit_value<S>(bits: &BitValue, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if let Some(value) = bits.to_u64_checked() {
        serializer.serialize_u64(value)
    } else {
        serializer.serialize_str(&bits.to_string())
    }
}

impl From<ParseBitValueError> for ParseLogicValueError {
    fn from(error: ParseBitValueError) -> Self {
        Self::new(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{LogicPattern, LogicValue};
    use crate::bit_value::BitValue;

    #[test]
    fn logic_value_deserializes_numeric_shorthand() {
        let value: LogicValue = serde_json::from_str("\"0x10\"").expect("parse");

        assert_eq!(value.to_bit_value_checked(), Some(BitValue::from(16_u64)));
        assert!(value.is_two_state());
    }

    #[test]
    fn logic_value_round_trips_four_state_strings() {
        let value: LogicValue = serde_json::from_str("\"10xz\"").expect("parse");
        let json = serde_json::to_string(&value).expect("serialize");

        assert_eq!(value.width(), 4);
        assert_eq!(json, "\"10xz\"");
        assert!(value.to_bit_value_checked().is_none());
    }

    #[test]
    fn logic_value_rejects_wildcards() {
        let error = "?".parse::<LogicValue>().expect_err("wildcard should fail");

        assert!(
            error.to_string().contains("invalid logic digit '?'"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn logic_pattern_matches_wildcards() {
        let pattern = "1?x".parse::<LogicPattern>().expect("pattern");

        assert!(pattern.matches(&"10x".parse::<LogicValue>().expect("match")));
        assert!(pattern.matches(&"11x".parse::<LogicValue>().expect("match")));
        assert!(!pattern.matches(&"11z".parse::<LogicValue>().expect("mismatch")));
    }

    #[test]
    fn logic_pattern_preserves_numeric_shorthand_serialization() {
        let pattern = LogicPattern::from(
            BitValue::from_str_radix("18446744073709551616", 10).expect("parse"),
        );
        let json = serde_json::to_string(&pattern).expect("serialize");

        assert_eq!(json, "\"18446744073709551616\"");
    }
}
