use std::cmp::Ordering;
use std::error::Error as StdError;
use std::fmt;
use std::str::FromStr;

use serde::de::{Error as DeError, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const BIT_VALUE_LIMB_BITS: usize = u64::BITS as usize;

const DECIMAL_CHUNK_BASE: u64 = 10_000_000_000_000_000_000;
const DECIMAL_CHUNK_WIDTH: usize = 19;

/// Limb storage with an inline fast path. Values that fit in one limb — the
/// overwhelming majority of runtime signal values — never touch the heap.
///
/// Invariant: `Heap` always holds at least two limbs and its last limb is
/// non-zero; anything smaller is `Inline` (zero is `Inline(0)`). This keeps
/// the derived `PartialEq`/`Hash` consistent with numeric equality.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Limbs {
    Inline(u64),
    Heap(Vec<u64>),
}

impl Default for Limbs {
    fn default() -> Self {
        Limbs::Inline(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub struct BitValue {
    limbs: Limbs,
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
        matches!(self.limbs, Limbs::Inline(0))
    }

    /// The significant limbs, least significant first; empty when zero.
    fn limbs(&self) -> &[u64] {
        match &self.limbs {
            Limbs::Inline(0) => &[],
            Limbs::Inline(value) => std::slice::from_ref(value),
            Limbs::Heap(limbs) => limbs,
        }
    }

    fn limb_len(&self) -> usize {
        match &self.limbs {
            Limbs::Inline(0) => 0,
            Limbs::Inline(_) => 1,
            Limbs::Heap(limbs) => limbs.len(),
        }
    }

    /// Single-limb view: `Some(limb0)` when the value fits in one limb.
    fn inline(&self) -> Option<u64> {
        match &self.limbs {
            Limbs::Inline(value) => Some(*value),
            Limbs::Heap(_) => None,
        }
    }

    fn from_inline(value: u64) -> Self {
        Self {
            limbs: Limbs::Inline(value),
        }
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

        let mut limbs = Vec::new();
        for ch in cleaned.chars() {
            let digit = ch
                .to_digit(radix)
                .ok_or_else(|| ParseBitValueError::new("invalid digit for radix"))?;
            mul_small_assign(&mut limbs, radix as u64);
            add_small_assign(&mut limbs, digit as u64);
        }

        Ok(Self::new(limbs))
    }

    pub fn bit_len(&self) -> usize {
        let limbs = self.limbs();
        let Some(&last) = limbs.last() else {
            return 0;
        };
        (limbs.len() - 1) * BIT_VALUE_LIMB_BITS
            + (BIT_VALUE_LIMB_BITS - last.leading_zeros() as usize)
    }

    pub fn to_u64_checked(&self) -> Option<u64> {
        self.inline()
    }

    pub fn to_usize_checked(&self) -> Option<usize> {
        usize::try_from(self.to_u64_checked()?).ok()
    }

    pub fn cmp_unsigned(&self, other: &Self) -> Ordering {
        let left = self.limbs();
        let right = other.limbs();
        match left.len().cmp(&right.len()) {
            Ordering::Equal => {}
            ordering => return ordering,
        }

        for (left, right) in left.iter().rev().zip(right.iter().rev()) {
            match left.cmp(right) {
                Ordering::Equal => {}
                ordering => return ordering,
            }
        }

        Ordering::Equal
    }

    pub fn truncate(&self, width: usize) -> Self {
        if let Some(value) = self.inline() {
            if width == 0 {
                return Self::zero();
            }
            if width >= BIT_VALUE_LIMB_BITS {
                return Self::from_inline(value);
            }
            return Self::from_inline(value & low_mask(width));
        }
        let mut value = self.clone();
        value.truncate_in_place(width);
        value
    }

    pub fn truncate_in_place(&mut self, width: usize) {
        match &mut self.limbs {
            Limbs::Inline(value) => {
                if width == 0 {
                    *value = 0;
                } else if width < BIT_VALUE_LIMB_BITS {
                    *value &= low_mask(width);
                }
            }
            Limbs::Heap(limbs) => {
                let limb_count = limb_count(width);
                limbs.truncate(limb_count);
                if width == 0 {
                    limbs.clear();
                } else if let Some(last) = limbs.last_mut() {
                    *last &= low_mask(width % BIT_VALUE_LIMB_BITS);
                }
                self.canonicalize();
            }
        }
    }

    pub fn mask(width: usize) -> Self {
        let limb_count = limb_count(width);
        match limb_count {
            0 => Self::zero(),
            1 => Self::from_inline(low_mask(width % BIT_VALUE_LIMB_BITS)),
            _ => {
                let mut limbs = vec![u64::MAX; limb_count];
                if let Some(last) = limbs.last_mut() {
                    *last = low_mask(width % BIT_VALUE_LIMB_BITS);
                }
                Self::new(limbs)
            }
        }
    }

    pub fn bitand(&self, other: &Self) -> Self {
        if let (Some(left), Some(right)) = (self.inline(), other.inline()) {
            return Self::from_inline(left & right);
        }
        let len = self.limb_len().min(other.limb_len());
        let mut limbs = Vec::with_capacity(len);
        for index in 0..len {
            limbs.push(self.limb(index) & other.limb(index));
        }
        Self::new(limbs)
    }

    pub fn bitor(&self, other: &Self) -> Self {
        if let (Some(left), Some(right)) = (self.inline(), other.inline()) {
            return Self::from_inline(left | right);
        }
        self.combine(other, |left, right| left | right)
    }

    pub fn bitxor(&self, other: &Self) -> Self {
        if let (Some(left), Some(right)) = (self.inline(), other.inline()) {
            return Self::from_inline(left ^ right);
        }
        self.combine(other, |left, right| left ^ right)
    }

    pub fn bitnot_with_width(&self, width: usize) -> Self {
        if width <= BIT_VALUE_LIMB_BITS {
            if width == 0 {
                return Self::zero();
            }
            return Self::from_inline(!self.limb(0) & low_mask(width % BIT_VALUE_LIMB_BITS));
        }
        let mut limbs = self.limbs().to_vec();
        limbs.resize(limb_count(width), 0);
        for limb in &mut limbs {
            *limb = !*limb;
        }
        let mut value = Self::new(limbs);
        value.truncate_in_place(width);
        value
    }

    pub fn shift_left(&self, amount: usize) -> Self {
        if self.is_zero() || amount == 0 {
            return self.clone();
        }
        if let Some(value) = self.inline() {
            if amount + self.bit_len() <= BIT_VALUE_LIMB_BITS {
                return Self::from_inline(value << amount);
            }
        }

        let source = self.limbs();
        let limb_shift = amount / BIT_VALUE_LIMB_BITS;
        let bit_shift = amount % BIT_VALUE_LIMB_BITS;
        let extra = usize::from(bit_shift != 0);
        let mut limbs = vec![0; source.len() + limb_shift + extra];

        for (index, limb) in source.iter().enumerate() {
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
        if let Some(value) = self.inline() {
            if amount >= BIT_VALUE_LIMB_BITS {
                return Self::zero();
            }
            return Self::from_inline(value >> amount);
        }

        let source = self.limbs();
        let limb_shift = amount / BIT_VALUE_LIMB_BITS;
        let bit_shift = amount % BIT_VALUE_LIMB_BITS;
        if limb_shift >= source.len() {
            return Self::zero();
        }

        let mut limbs = vec![0; source.len() - limb_shift];
        for src in limb_shift..source.len() {
            let dest = src - limb_shift;
            limbs[dest] |= source[src] >> bit_shift;
            if bit_shift != 0 && src + 1 < source.len() {
                limbs[dest] |= source[src + 1] << (BIT_VALUE_LIMB_BITS - bit_shift);
            }
        }

        Self::new(limbs)
    }

    pub fn wrapping_add(&self, other: &Self, width: usize) -> Self {
        if width == 0 {
            return Self::zero();
        }
        if width <= BIT_VALUE_LIMB_BITS {
            let sum = self.limb(0).wrapping_add(other.limb(0));
            return Self::from_inline(sum & low_mask(width % BIT_VALUE_LIMB_BITS));
        }

        let limit = limb_count(width);
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

    pub fn wrapping_mul(&self, other: &Self, width: usize) -> Self {
        if width == 0 || self.is_zero() || other.is_zero() {
            return Self::zero();
        }
        if width <= BIT_VALUE_LIMB_BITS {
            let product = self.limb(0).wrapping_mul(other.limb(0));
            return Self::from_inline(product & low_mask(width % BIT_VALUE_LIMB_BITS));
        }

        let limit = limb_count(width);
        let mut limbs = vec![0u64; limit + 1];
        for i in 0..limit {
            let a = self.limb(i) as u128;
            if a == 0 {
                continue;
            }
            let mut carry = 0_u128;
            for j in 0..limit {
                if i + j >= limbs.len() {
                    break;
                }
                let product = a * other.limb(j) as u128 + limbs[i + j] as u128 + carry;
                limbs[i + j] = product as u64;
                carry = product >> BIT_VALUE_LIMB_BITS;
            }
        }

        let mut value = Self::new(limbs);
        value.truncate_in_place(width);
        value
    }

    pub fn wrapping_sub(&self, other: &Self, width: usize) -> Self {
        if width == 0 {
            return Self::zero();
        }
        if width <= BIT_VALUE_LIMB_BITS {
            let difference = self.limb(0).wrapping_sub(other.limb(0));
            return Self::from_inline(difference & low_mask(width % BIT_VALUE_LIMB_BITS));
        }

        let limit = limb_count(width);
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
        if let Limbs::Inline(current) = &mut self.limbs {
            if index < BIT_VALUE_LIMB_BITS {
                if value {
                    *current |= 1_u64 << index;
                } else {
                    *current &= !(1_u64 << index);
                }
                return;
            }
            if !value {
                return;
            }
        }

        let mut limbs = self.limbs().to_vec();
        let limb_index = index / BIT_VALUE_LIMB_BITS;
        let bit_index = index % BIT_VALUE_LIMB_BITS;
        if value {
            if limbs.len() <= limb_index {
                limbs.resize(limb_index + 1, 0);
            }
            limbs[limb_index] |= 1_u64 << bit_index;
        } else if limb_index < limbs.len() {
            limbs[limb_index] &= !(1_u64 << bit_index);
        }
        *self = Self::new(limbs);
    }

    pub fn slice(&self, lsb: usize, width: usize) -> Self {
        self.shift_right(lsb).truncate(width)
    }

    fn new(mut limbs: Vec<u64>) -> Self {
        while limbs.last() == Some(&0) {
            limbs.pop();
        }
        match limbs.as_slice() {
            [] => Self::from_inline(0),
            [value] => Self::from_inline(*value),
            _ => Self {
                limbs: Limbs::Heap(limbs),
            },
        }
    }

    fn limb(&self, index: usize) -> u64 {
        self.limbs().get(index).copied().unwrap_or(0)
    }

    /// Restores the `Inline`/`Heap` invariant after in-place heap edits.
    fn canonicalize(&mut self) {
        if let Limbs::Heap(limbs) = &mut self.limbs {
            while limbs.last() == Some(&0) {
                limbs.pop();
            }
            match limbs.as_slice() {
                [] => self.limbs = Limbs::Inline(0),
                [value] => self.limbs = Limbs::Inline(*value),
                _ => {}
            }
        }
    }

    fn combine(&self, other: &Self, op: impl Fn(u64, u64) -> u64) -> Self {
        let len = self.limb_len().max(other.limb_len());
        let mut limbs = Vec::with_capacity(len);
        for index in 0..len {
            limbs.push(op(self.limb(index), other.limb(index)));
        }
        Self::new(limbs)
    }
}

fn add_small_assign(limbs: &mut Vec<u64>, value: u64) {
    if value == 0 {
        return;
    }

    let mut carry = value as u128;
    let mut index = 0;
    while carry != 0 {
        if index == limbs.len() {
            limbs.push(0);
        }
        let sum = limbs[index] as u128 + carry;
        limbs[index] = sum as u64;
        carry = sum >> BIT_VALUE_LIMB_BITS;
        index += 1;
    }
}

fn mul_small_assign(limbs: &mut Vec<u64>, factor: u64) {
    if limbs.is_empty() || factor == 1 {
        return;
    }
    if factor == 0 {
        limbs.clear();
        return;
    }

    let mut carry = 0_u128;
    for limb in limbs.iter_mut() {
        let product = *limb as u128 * factor as u128 + carry;
        *limb = product as u64;
        carry = product >> BIT_VALUE_LIMB_BITS;
    }
    if carry != 0 {
        limbs.push(carry as u64);
    }
}

fn div_rem_small(limbs: &mut Vec<u64>, divisor: u64) -> u64 {
    debug_assert!(divisor != 0);
    let mut remainder = 0_u128;
    for limb in limbs.iter_mut().rev() {
        let value = (remainder << BIT_VALUE_LIMB_BITS) | *limb as u128;
        *limb = (value / divisor as u128) as u64;
        remainder = value % divisor as u128;
    }
    while limbs.last() == Some(&0) {
        limbs.pop();
    }
    remainder as u64
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

        let mut limbs = self.limbs().to_vec();
        let mut chunks = Vec::new();
        while !limbs.is_empty() {
            chunks.push(div_rem_small(&mut limbs, DECIMAL_CHUNK_BASE));
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
        Self::from_inline(value)
    }
}

impl From<u128> for BitValue {
    fn from(value: u128) -> Self {
        let low = value as u64;
        let high = (value >> BIT_VALUE_LIMB_BITS) as u64;
        if high == 0 {
            Self::from_inline(low)
        } else {
            Self {
                limbs: Limbs::Heap(vec![low, high]),
            }
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

    #[test]
    fn inline_and_heap_values_compare_consistently_across_transitions() {
        // Heap results that shrink to one limb must equal (and hash like)
        // values built inline.
        let wide = BitValue::from(1_u128 << 64);
        let narrowed = wide.truncate(64);
        assert_eq!(narrowed, BitValue::zero());

        let one_again = wide.shift_right(64);
        assert_eq!(one_again, BitValue::one());

        let mut bit_cleared = BitValue::from(1_u128 << 64);
        bit_cleared.set_bit(64, false);
        assert_eq!(bit_cleared, BitValue::zero());

        let mut grown = BitValue::one();
        grown.set_bit(64, true);
        assert_eq!(grown, BitValue::from((1_u128 << 64) | 1));
        assert_eq!(grown.to_u64_checked(), None);
    }

    #[test]
    fn single_limb_arithmetic_matches_wide_paths_at_boundaries() {
        let max = BitValue::from(u64::MAX);
        assert_eq!(max.wrapping_add(&BitValue::one(), 64), BitValue::zero());
        assert_eq!(
            max.wrapping_add(&BitValue::one(), 65),
            BitValue::from(1_u128 << 64)
        );
        assert_eq!(
            BitValue::zero().wrapping_sub(&BitValue::one(), 64),
            BitValue::from(u64::MAX)
        );
        assert_eq!(
            max.wrapping_mul(&BitValue::from(2_u64), 64),
            BitValue::from(u64::MAX - 1)
        );
        assert_eq!(BitValue::mask(64), BitValue::from(u64::MAX));
        assert_eq!(BitValue::mask(1), BitValue::one());
        assert_eq!(
            BitValue::from(0b1010_u64).bitnot_with_width(4),
            BitValue::from(0b0101_u64)
        );
    }
}
