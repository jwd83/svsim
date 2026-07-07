//! Numeric and string literal lowering: sized/based literals, four-state
//! decimal digits, and string-literal byte expansion.

use super::*;

pub(super) fn lower_literal(
    syntax_tree: &SyntaxTree,
    literal: &sv_parser::PrimaryLiteral,
) -> LowerResult<Expr> {
    match literal {
        sv_parser::PrimaryLiteral::Number(number) => {
            Ok(Expr::Literal(lower_number(syntax_tree, number)?))
        }
        sv_parser::PrimaryLiteral::UnbasedUnsizedLiteral(literal) => {
            let text = symbol_text(syntax_tree, &literal.nodes.0)?;
            let bits = match text.as_str() {
                "'0" => LogicBits::from_bit_value(BitValue::zero()),
                "'1" => LogicBits::from_bit_value(BitValue::one()),
                "'x" | "'X" => LogicBits::filled(1, LogicBit::X),
                "'z" | "'Z" => LogicBits::filled(1, LogicBit::Z),
                _ => return Err(unsupported("unsupported unbased unsized literal", None)),
            };
            Ok(Expr::Literal(NumericLiteral { bits, width: None }))
        }
        sv_parser::PrimaryLiteral::StringLiteral(literal) => {
            let raw = syntax_tree
                .get_str(&literal.nodes.0)
                .ok_or_else(|| unsupported("failed to read string literal text", None))?;
            let bytes = parse_string_literal_bytes(raw)?;
            let width = (bytes.len() * 8).max(1);
            let mut bits = BitValue::zero();
            for byte in bytes {
                bits = bits.shift_left(8);
                bits = bits.bitor(&BitValue::from(byte as u64));
            }
            Ok(Expr::Literal(NumericLiteral {
                bits: LogicBits::from_bit_value(bits),
                width: Some(width),
            }))
        }
        _ => Err(unsupported(
            "literal is outside the current executable subset",
            None,
        )),
    }
}

pub(super) fn lower_number(
    syntax_tree: &SyntaxTree,
    number: &sv_parser::Number,
) -> LowerResult<NumericLiteral> {
    let sv_parser::Number::IntegralNumber(number) = number else {
        return Err(unsupported("real numbers are not supported", None));
    };
    match &**number {
        sv_parser::IntegralNumber::DecimalNumber(number) => match &**number {
            sv_parser::DecimalNumber::UnsignedNumber(number) => {
                let text = syntax_tree
                    .get_str(&number.nodes.0)
                    .ok_or_else(|| unsupported("failed to read numeric literal text", None))?;
                let bits = BitValue::from_str_radix(&text.replace('_', ""), 10)
                    .map_err(|_| unsupported("failed to parse numeric literal", None))?;
                Ok(NumericLiteral {
                    bits: LogicBits::from_bit_value(bits),
                    width: Some(32),
                })
            }
            sv_parser::DecimalNumber::BaseUnsigned(number) => {
                let width = number
                    .nodes
                    .0
                    .as_ref()
                    .map(|size| locate_usize(syntax_tree, &size.nodes.0.nodes.0))
                    .transpose()?;
                Ok(NumericLiteral {
                    bits: parse_based_value(syntax_tree, &number.nodes.2.nodes.0, 10, width)?,
                    width,
                })
            }
            _ => Err(unsupported("x/z decimal literals are not supported", None)),
        },
        sv_parser::IntegralNumber::BinaryNumber(number) => {
            let width = number
                .nodes
                .0
                .as_ref()
                .map(|size| locate_usize(syntax_tree, &size.nodes.0.nodes.0))
                .transpose()?;
            Ok(NumericLiteral {
                bits: parse_based_value(syntax_tree, &number.nodes.2.nodes.0, 2, width)?,
                width,
            })
        }
        sv_parser::IntegralNumber::OctalNumber(number) => {
            let width = number
                .nodes
                .0
                .as_ref()
                .map(|size| locate_usize(syntax_tree, &size.nodes.0.nodes.0))
                .transpose()?;
            Ok(NumericLiteral {
                bits: parse_based_value(syntax_tree, &number.nodes.2.nodes.0, 8, width)?,
                width,
            })
        }
        sv_parser::IntegralNumber::HexNumber(number) => {
            let width = number
                .nodes
                .0
                .as_ref()
                .map(|size| locate_usize(syntax_tree, &size.nodes.0.nodes.0))
                .transpose()?;
            Ok(NumericLiteral {
                bits: parse_based_value(syntax_tree, &number.nodes.2.nodes.0, 16, width)?,
                width,
            })
        }
    }
}

pub(super) fn parse_based_value(
    syntax_tree: &SyntaxTree,
    locate: &Locate,
    radix: u32,
    explicit_width: Option<usize>,
) -> LowerResult<LogicBits> {
    let text = syntax_tree
        .get_str(locate)
        .ok_or_else(|| unsupported("failed to read numeric literal text", None))?;
    let cleaned: String = text.chars().filter(|ch| *ch != '_').collect();
    if cleaned.is_empty() {
        return Err(unsupported("numeric literal has no digits", None));
    }

    let bits_per_digit = match radix {
        2 => 1,
        8 => 3,
        16 => 4,
        10 => return parse_decimal_logic_bits(&cleaned, explicit_width),
        _ => return Err(unsupported("unsupported numeric literal radix", None)),
    };

    let natural_width = cleaned.chars().count() * bits_per_digit;
    let mut bits = LogicBits::zero();
    for (digit_index, ch) in cleaned.chars().rev().enumerate() {
        let base = digit_index * bits_per_digit;
        match ch {
            'x' | 'X' => {
                for offset in 0..bits_per_digit {
                    bits.set_bit(base + offset, LogicBit::X);
                }
            }
            'z' | 'Z' | '?' => {
                for offset in 0..bits_per_digit {
                    bits.set_bit(base + offset, LogicBit::Z);
                }
            }
            other => {
                let digit = other.to_digit(radix).ok_or_else(|| {
                    unsupported(
                        format!("invalid digit '{}' in numeric literal", other),
                        None,
                    )
                })?;
                for offset in 0..bits_per_digit {
                    if (digit >> offset) & 1 == 1 {
                        bits.set_bit(base + offset, LogicBit::One);
                    }
                }
            }
        }
    }

    if let Some(target) = explicit_width {
        if target > natural_width && natural_width > 0 {
            let top = bits.bit(natural_width - 1);
            if matches!(top, LogicBit::X | LogicBit::Z) {
                for index in natural_width..target {
                    bits.set_bit(index, top);
                }
            }
        }
        Ok(bits.truncate(target))
    } else {
        Ok(bits)
    }
}

pub(super) fn parse_decimal_logic_bits(
    cleaned: &str,
    explicit_width: Option<usize>,
) -> LowerResult<LogicBits> {
    for ch in cleaned.chars() {
        if matches!(ch, 'x' | 'X' | 'z' | 'Z' | '?') {
            return Err(unsupported(
                "x/z digits are not supported in decimal literals",
                None,
            ));
        }
    }
    let bits = BitValue::from_str_radix(cleaned, 10)
        .map_err(|_| unsupported("failed to parse numeric literal", None))?;
    let bits = LogicBits::from_bit_value(bits);
    Ok(match explicit_width {
        Some(target) => bits.truncate(target),
        None => bits,
    })
}

pub(super) fn parse_string_literal_bytes(text: &str) -> LowerResult<Vec<u8>> {
    if !(text.starts_with('"') && text.ends_with('"')) {
        return Err(unsupported("string literal is malformed", None));
    }

    let mut bytes = Vec::new();
    let mut chars = text[1..text.len() - 1].chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            if !ch.is_ascii() {
                return Err(unsupported(
                    "non-ASCII string literals are not supported yet",
                    None,
                ));
            }
            bytes.push(ch as u8);
            continue;
        }

        let escaped = chars
            .next()
            .ok_or_else(|| unsupported("string literal ends with a dangling escape", None))?;
        let byte = match escaped {
            'n' => b'\n',
            'r' => b'\r',
            't' => b'\t',
            '\\' => b'\\',
            '"' => b'"',
            '0' => b'\0',
            other if other.is_ascii() => other as u8,
            _ => {
                return Err(unsupported(
                    "non-ASCII string literals are not supported yet",
                    None,
                ));
            }
        };
        bytes.push(byte);
    }

    Ok(bytes)
}
