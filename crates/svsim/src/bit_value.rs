/// Central type alias for signal bit storage.
/// Change this single line to benchmark different backing types (u64, u128, etc.).
pub type BitValue = u128;

/// Number of bits in the current BitValue type.
pub const BIT_VALUE_BITS: usize = BitValue::BITS as usize;
