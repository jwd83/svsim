use std::cmp::Ordering;
use std::error::Error as StdError;
use std::fmt;
use std::str::FromStr;

use serde::de::{Error as DeError, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const BIT_VALUE_LIMB_BITS: usize = u64::BITS as usize;

const DECIMAL_CHUNK_BASE: u64 = 10_000_000_000_000_000_000;
const DECIMAL_CHUNK_WIDTH: usize = 19;

#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub struct BitValue {
    limbs: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseBitValueError {
    message: &'static str,
}

impl ParseBitValueError {
    fn new(message: &'static str) -> Self {
        Self { message }
    }
}

impl fmt::Display for ParseBitValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message)
    }
}

impl StdError for ParseBitValueError {}

impl BitValue {
    pub fn zero() -> Self {
        Self::default()
    }

    pub fn one() -> Self {
        Self::from(1_u64)
    }

    pub fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    pub fn from_prefixed_str(text: &str) -> Result<Self, ParseBitValueError> {
        let cleaned = text.trim().replace('_', "");
        if cleaned.is_empty() {
            return Err(ParseBitValueError::new(
                "empty string is not a valid integer",
            ));
        }
        if cleaned.starts_with('-') {
            return Err(ParseBitValueError::new(
                "negative integers are not supported for bit values",
            ));
        }
        if let Some(rest) = cleaned
            .strip_prefix("0x")
            .or_else(|| cleaned.strip_prefix("0X"))
        {
            return Self::from_str_radix(rest, 16);
        }
        if let Some(rest) = cleaned
            .strip_prefix("0b")
            .or_else(|| cleaned.strip_prefix("0B"))
        {
            return Self::from_str_radix(rest, 2);
        }
        if let Some(rest) = cleaned
            .strip_prefix("0o")
            .or_else(|| cleaned.strip_prefix("0O"))
        {
            return Self::from_str_radix(rest, 8);
        }

        Self::from_str_radix(&cleaned, 10)
    }

    pub fn from_str_radix(text: &str, radix: u32) -> Result<Self, ParseBitValueError> {
        if !(2..=36).contains(&radix) {
            return Err(ParseBitValueError::new("radix must be in 2..=36"));
        }

        let cleaned = text.trim();
        if cleaned.is_empty() {
            return Err(ParseBitValueError::new(
                "empty string is not a valid integer",
            ));
        }

        let mut value = Self::zero();
        for ch in cleaned.chars() {
            let digit = ch
                .to_digit(radix)
                .ok_or_else(|| ParseBitValueError::new("invalid digit for radix"))?;
            value.mul_small_assign(radix as u64);
            value.add_small_assign(digit as u64);
        }

        Ok(value)
    }

    pub fn bit_len(&self) -> usize {
        let Some(&last) = self.limbs.last() else {
            return 0;
        };
        (self.limbs.len() - 1) * BIT_VALUE_LIMB_BITS
            + (BIT_VALUE_LIMB_BITS - last.leading_zeros() as usize)
    }

    pub fn to_u64_checked(&self) -> Option<u64> {
        match self.limbs.as_slice() {
            [] => Some(0),
            [value] => Some(*value),
            _ => None,
        }
    }

    pub fn to_usize_checked(&self) -> Option<usize> {
        usize::try_from(self.to_u64_checked()?).ok()
    }

    pub fn cmp_unsigned(&self, other: &Self) -> Ordering {
        match self.limbs.len().cmp(&other.limbs.len()) {
            Ordering::Equal => {}
            ordering => return ordering,
        }

        for (left, right) in self.limbs.iter().rev().zip(other.limbs.iter().rev()) {
            match left.cmp(right) {
                Ordering::Equal => {}
                ordering => return ordering,
            }
        }

        Ordering::Equal
    }

    pub fn truncate(&self, width: usize) -> Self {
        let mut value = self.clone();
        value.truncate_in_place(width);
        value
    }

    pub fn truncate_in_place(&mut self, width: usize) {
        let limb_count = limb_count(width);
        self.limbs.truncate(limb_count);
        if width == 0 {
            self.limbs.clear();
            return;
        }
        if let Some(last) = self.limbs.last_mut() {
            *last &= low_mask(width % BIT_VALUE_LIMB_BITS);
        }
        self.normalize();
    }

    pub fn mask(width: usize) -> Self {
        let limb_count = limb_count(width);
        if limb_count == 0 {
            return Self::zero();
        }

        let mut limbs = vec![u64::MAX; limb_count];
        if let Some(last) = limbs.last_mut() {
            *last = low_mask(width % BIT_VALUE_LIMB_BITS);
        }
        Self::new(limbs)
    }

    pub fn bitand(&self, other: &Self) -> Self {
        let len = self.limbs.len().min(other.limbs.len());
        let mut limbs = Vec::with_capacity(len);
        for index in 0..len {
            limbs.push(self.limb(index) & other.limb(index));
        }
        Self::new(limbs)
    }

    pub fn bitor(&self, other: &Self) -> Self {
        self.combine(other, |left, right| left | right)
    }

    pub fn bitxor(&self, other: &Self) -> Self {
        self.combine(other, |left, right| left ^ right)
    }

    pub fn bitnot_with_width(&self, width: usize) -> Self {
        let mut value = self.truncate(width);
        let required_limbs = limb_count(width);
        if value.limbs.len() < required_limbs {
            value.limbs.resize(required_limbs, 0);
        }
        for limb in &mut value.limbs {
            *limb = !*limb;
        }
        value.truncate_in_place(width);
        value
    }

    pub fn shift_left(&self, amount: usize) -> Self {
        if self.is_zero() || amount == 0 {
            return self.clone();
        }

        let limb_shift = amount / BIT_VALUE_LIMB_BITS;
        let bit_shift = amount % BIT_VALUE_LIMB_BITS;
        let extra = usize::from(bit_shift != 0);
        let mut limbs = vec![0; self.limbs.len() + limb_shift + extra];

        for (index, limb) in self.limbs.iter().enumerate() {
            let dest = index + limb_shift;
            limbs[dest] |= *limb << bit_shift;
            if bit_shift != 0 {
                limbs[dest + 1] |= *limb >> (BIT_VALUE_LIMB_BITS - bit_shift);
            }
        }

        Self::new(limbs)
    }

    pub fn shift_right(&self, amount: usize) -> Self {
        if self.is_zero() || amount == 0 {
            return self.clone();
        }

        let limb_shift = amount / BIT_VALUE_LIMB_BITS;
        let bit_shift = amount % BIT_VALUE_LIMB_BITS;
        if limb_shift >= self.limbs.len() {
            return Self::zero();
        }

        let mut limbs = vec![0; self.limbs.len() - limb_shift];
        for src in limb_shift..self.limbs.len() {
            let dest = src - limb_shift;
            limbs[dest] |= self.limbs[src] >> bit_shift;
            if bit_shift != 0 && src + 1 < self.limbs.len() {
                limbs[dest] |= self.limbs[src + 1] << (BIT_VALUE_LIMB_BITS - bit_shift);
            }
        }

        Self::new(limbs)
    }

    pub fn wrapping_add(&self, other: &Self, width: usize) -> Self {
        let limit = limb_count(width);
        if limit == 0 {
            return Self::zero();
        }

        let mut limbs = Vec::with_capacity(limit + 1);
        let mut carry = 0_u128;
        for index in 0..limit {
            let sum = self.limb(index) as u128 + other.limb(index) as u128 + carry;
            limbs.push(sum as u64);
            carry = sum >> BIT_VALUE_LIMB_BITS;
        }
        if carry != 0 {
            limbs.push(carry as u64);
        }

        let mut value = Self::new(limbs);
        value.truncate_in_place(width);
        value
    }

    pub fn wrapping_sub(&self, other: &Self, width: usize) -> Self {
        let limit = limb_count(width);
        if limit == 0 {
            return Self::zero();
        }

        let mut limbs = Vec::with_capacity(limit);
        let mut borrow = 0_u128;
        for index in 0..limit {
            let left = self.limb(index) as u128;
            let right = other.limb(index) as u128 + borrow;
            if left >= right {
                limbs.push((left - right) as u64);
                borrow = 0;
            } else {
                limbs.push(((1_u128 << BIT_VALUE_LIMB_BITS) + left - right) as u64);
                borrow = 1;
            }
        }

        let mut value = Self::new(limbs);
        value.truncate_in_place(width);
        value
    }

    pub fn get_bit(&self, index: usize) -> bool {
        let limb_index = index / BIT_VALUE_LIMB_BITS;
        let bit_index = index % BIT_VALUE_LIMB_BITS;
        ((self.limb(limb_index) >> bit_index) & 1) == 1
    }

    pub fn set_bit(&mut self, index: usize, value: bool) {
        let limb_index = index / BIT_VALUE_LIMB_BITS;
        let bit_index = index % BIT_VALUE_LIMB_BITS;
        if value {
            if self.limbs.len() <= limb_index {
                self.limbs.resize(limb_index + 1, 0);
            }
            self.limbs[limb_index] |= 1_u64 << bit_index;
        } else if limb_index < self.limbs.len() {
            self.limbs[limb_index] &= !(1_u64 << bit_index);
            self.normalize();
        }
    }

    pub fn slice(&self, lsb: usize, width: usize) -> Self {
        self.shift_right(lsb).truncate(width)
    }

    fn new(mut limbs: Vec<u64>) -> Self {
        while limbs.last() == Some(&0) {
            limbs.pop();
        }
        Self { limbs }
    }

    fn limb(&self, index: usize) -> u64 {
        self.limbs.get(index).copied().unwrap_or(0)
    }

    fn normalize(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }

    fn combine(&self, other: &Self, op: impl Fn(u64, u64) -> u64) -> Self {
        let len = self.limbs.len().max(other.limbs.len());
        let mut limbs = Vec::with_capacity(len);
        for index in 0..len {
            limbs.push(op(self.limb(index), other.limb(index)));
        }
        Self::new(limbs)
    }

    fn add_small_assign(&mut self, value: u64) {
        if value == 0 {
            return;
        }

        let mut carry = value as u128;
        let mut index = 0;
        while carry != 0 {
            if index == self.limbs.len() {
                self.limbs.push(0);
            }
            let sum = self.limbs[index] as u128 + carry;
            self.limbs[index] = sum as u64;
            carry = sum >> BIT_VALUE_LIMB_BITS;
            index += 1;
        }
    }

    fn mul_small_assign(&mut self, factor: u64) {
        if self.is_zero() || factor == 1 {
            return;
        }
        if factor == 0 {
            self.limbs.clear();
            return;
        }

        let mut carry = 0_u128;
        for limb in &mut self.limbs {
            let product = *limb as u128 * factor as u128 + carry;
            *limb = product as u64;
            carry = product >> BIT_VALUE_LIMB_BITS;
        }
        if carry != 0 {
            self.limbs.push(carry as u64);
        }
    }

    fn div_rem_small(&mut self, divisor: u64) -> u64 {
        debug_assert!(divisor != 0);
        let mut remainder = 0_u128;
        for limb in self.limbs.iter_mut().rev() {
            let value = (remainder << BIT_VALUE_LIMB_BITS) | *limb as u128;
            *limb = (value / divisor as u128) as u64;
            remainder = value % divisor as u128;
        }
        self.normalize();
        remainder as u64
    }
}

impl Serialize for BitValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(value) = self.to_u64_checked() {
            serializer.serialize_u64(value)
        } else {
            serializer.serialize_str(&self.to_string())
        }
    }
}

impl<'de> Deserialize<'de> for BitValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(BitValueVisitor)
    }
}

struct BitValueVisitor;

impl Visitor<'_> for BitValueVisitor {
    type Value = BitValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a non-negative integer as a number or string")
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        Ok(BitValue::from(value))
    }

    fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        Ok(BitValue::from(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        if value < 0 {
            return Err(E::custom("negative integers are not supported"));
        }
        Ok(BitValue::from(value as u64))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        BitValue::from_prefixed_str(value).map_err(E::custom)
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        self.visit_str(&value)
    }
}

impl fmt::Display for BitValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_zero() {
            return f.write_str("0");
        }

        let mut value = self.clone();
        let mut chunks = Vec::new();
        while !value.is_zero() {
            chunks.push(value.div_rem_small(DECIMAL_CHUNK_BASE));
        }

        let Some(first) = chunks.pop() else {
            return f.write_str("0");
        };
        write!(f, "{first}")?;
        for chunk in chunks.iter().rev() {
            write!(f, "{chunk:0DECIMAL_CHUNK_WIDTH$}")?;
        }
        Ok(())
    }
}

impl FromStr for BitValue {
    type Err = ParseBitValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str_radix(s, 10)
    }
}

impl From<u64> for BitValue {
    fn from(value: u64) -> Self {
        if value == 0 {
            Self::zero()
        } else {
            Self { limbs: vec![value] }
        }
    }
}

impl From<u128> for BitValue {
    fn from(value: u128) -> Self {
        let low = value as u64;
        let high = (value >> BIT_VALUE_LIMB_BITS) as u64;
        match (low, high) {
            (0, 0) => Self::zero(),
            (_, 0) => Self::from(low),
            _ => Self {
                limbs: vec![low, high],
            },
        }
    }
}

impl From<usize> for BitValue {
    fn from(value: usize) -> Self {
        Self::from(value as u64)
    }
}

impl From<bool> for BitValue {
    fn from(value: bool) -> Self {
        if value { Self::one() } else { Self::zero() }
    }
}

impl PartialOrd for BitValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BitValue {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cmp_unsigned(other)
    }
}

fn limb_count(width: usize) -> usize {
    width.div_ceil(BIT_VALUE_LIMB_BITS)
}

fn low_mask(bits: usize) -> u64 {
    match bits {
        0 => u64::MAX,
        BIT_VALUE_LIMB_BITS => u64::MAX,
        _ => (1_u64 << bits) - 1,
    }
}

#[cfg(test)]
mod tests {
    use super::BitValue;

    #[test]
    fn parses_large_decimal_values() {
        let value =
            BitValue::from_str_radix("340282366920938463463374607431768211455", 10).expect("parse");

        assert_eq!(value.to_string(), "340282366920938463463374607431768211455");
    }

    #[test]
    fn serializes_large_values_as_strings() {
        let value = BitValue::from_str_radix("18446744073709551616", 10).expect("parse");
        let json = serde_json::to_string(&value).expect("serialize");

        assert_eq!(json, "\"18446744073709551616\"");
    }

    #[test]
    fn deserializes_small_and_large_values() {
        let small: BitValue = serde_json::from_str("42").expect("small");
        let large: BitValue = serde_json::from_str("\"18446744073709551616\"").expect("large");

        assert_eq!(small.to_u64_checked(), Some(42));
        assert_eq!(large.to_string(), "18446744073709551616");
    }

    #[test]
    fn supports_limb_shifts_and_slices() {
        let value = BitValue::from_str_radix("18446744073709551617", 10).expect("parse");

        assert_eq!(value.shift_left(1).to_string(), "36893488147419103234");
        assert_eq!(value.shift_right(64).to_u64_checked(), Some(1));
        assert_eq!(value.slice(64, 1).to_u64_checked(), Some(1));
    }
}
