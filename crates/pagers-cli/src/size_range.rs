use std::str::FromStr;

use thiserror::Error;

#[derive(Copy, Clone, PartialEq, Eq, Debug, Error)]
pub enum RangeError {
    #[error("cannot parse size from empty string")]
    Empty,
    #[error("invalid digit found in string")]
    InvalidDigit,
    #[error("spaces are not allowed in size strings")]
    SpaceNotAllowed,
    #[error("number too large to fit in target type")]
    PosOverflow,
    #[error("range end must be greater than start")]
    RangeInverted,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum UnitSystem {
    Decimal,
    Binary,
}

impl UnitSystem {
    fn factor(self, prefix: u8) -> Option<u64> {
        Some(match (self, prefix) {
            (Self::Decimal, b'k' | b'K') => 1_000,
            (Self::Decimal, b'm' | b'M') => 1_000_000,
            (Self::Decimal, b'g' | b'G') => 1_000_000_000,
            (Self::Decimal, b't' | b'T') => 1_000_000_000_000,
            (Self::Decimal, b'p' | b'P') => 1_000_000_000_000_000,
            (Self::Decimal, b'e' | b'E') => 1_000_000_000_000_000_000,
            (Self::Binary, b'k' | b'K') => 1_u64 << 10,
            (Self::Binary, b'm' | b'M') => 1_u64 << 20,
            (Self::Binary, b'g' | b'G') => 1_u64 << 30,
            (Self::Binary, b't' | b'T') => 1_u64 << 40,
            (Self::Binary, b'p' | b'P') => 1_u64 << 50,
            (Self::Binary, b'e' | b'E') => 1_u64 << 60,
            _ => return None,
        })
    }
}

pub fn parse_size(src: &str) -> Result<u64, RangeError> {
    let mut src = src.as_bytes();

    let mut multiply = if let [init @ .., b'b' | b'B'] = src {
        src = init;
        1
    } else {
        1
    };

    let unit_system = if let [init @ .., b'i' | b'I'] = src {
        src = init;
        UnitSystem::Binary
    } else {
        UnitSystem::Decimal
    };

    if let [init @ .., prefix] = src
        && let Some(f) = unit_system.factor(*prefix)
    {
        multiply = f;
        src = init;
    }

    parse_with_multiply(src, multiply)
}

fn parse_with_multiply(src: &[u8], multiply: u64) -> Result<u64, RangeError> {
    #[derive(Copy, Clone, PartialEq)]
    enum S {
        Empty,
        Integer,
        IntegerOverflow,
        Fraction,
        FractionOverflow,
        PosExponent,
        NegExponent,
    }

    let mut mantissa = 0_u64;
    let mut fractional_exponent = 0;
    let mut exponent = 0_i32;
    let mut state = S::Empty;

    for &b in src {
        match (state, b) {
            (S::Integer | S::Empty, b'0'..=b'9') => {
                if let Some(m) = mantissa
                    .checked_mul(10)
                    .and_then(|v| v.checked_add((b - b'0').into()))
                {
                    mantissa = m;
                    state = S::Integer;
                } else {
                    if b >= b'5' {
                        mantissa += 1;
                    }
                    state = S::IntegerOverflow;
                    fractional_exponent += 1;
                }
            }
            (S::IntegerOverflow, b'0'..=b'9') => {
                fractional_exponent += 1;
            }
            (S::Fraction, b'0'..=b'9') => {
                if let Some(m) = mantissa
                    .checked_mul(10)
                    .and_then(|v| v.checked_add((b - b'0').into()))
                {
                    mantissa = m;
                    fractional_exponent -= 1;
                } else {
                    if b >= b'5' {
                        mantissa += 1;
                    }
                    state = S::FractionOverflow;
                }
            }
            (S::PosExponent, b'0'..=b'9') => {
                if let Some(e) = exponent
                    .checked_mul(10)
                    .and_then(|v| v.checked_add((b - b'0').into()))
                {
                    exponent = e;
                } else {
                    return Err(RangeError::PosOverflow);
                }
            }
            (S::NegExponent, b'0'..=b'9') => {
                if let Some(e) = exponent
                    .checked_mul(10)
                    .and_then(|v| v.checked_sub((b - b'0').into()))
                {
                    exponent = e;
                }
            }
            (_, b' ') => return Err(RangeError::SpaceNotAllowed),
            (_, b'_') | (S::PosExponent, b'+') | (S::FractionOverflow, b'0'..=b'9') => {}
            (S::Integer | S::Fraction | S::IntegerOverflow | S::FractionOverflow, b'e' | b'E') => {
                state = S::PosExponent;
            }
            (S::PosExponent, b'-') => state = S::NegExponent,
            (S::Integer, b'.') => state = S::Fraction,
            (S::IntegerOverflow, b'.') => state = S::FractionOverflow,
            _ => return Err(RangeError::InvalidDigit),
        }
    }

    if state == S::Empty {
        return Err(RangeError::Empty);
    }

    let exponent = exponent.saturating_add(fractional_exponent);
    let abs_exponent = exponent.unsigned_abs();
    if exponent >= 0 {
        let power = 10_u64
            .checked_pow(abs_exponent)
            .ok_or(RangeError::PosOverflow)?;
        let multiply = multiply.checked_mul(power).ok_or(RangeError::PosOverflow)?;
        mantissa
            .checked_mul(multiply)
            .ok_or(RangeError::PosOverflow)
    } else if exponent >= -38 {
        let power = 10_u128.pow(abs_exponent);
        let result = (u128::from(mantissa) * u128::from(multiply) + power / 2) / power;
        u64::try_from(result).map_err(|_| RangeError::PosOverflow)
    } else {
        Ok(0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SizeRange {
    pub start_b: u64,
    pub end_b: Option<u64>,
}

impl FromStr for SizeRange {
    type Err = RangeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        if let Some((left, right)) = try_split(s) {
            let start_b = if left.is_empty() {
                0
            } else {
                parse_size(left)?
            };

            let end_b = if right.is_empty() {
                None
            } else {
                Some(parse_size(right)?)
            };

            if let Some(end) = end_b
                && end <= start_b
            {
                return Err(RangeError::RangeInverted);
            }

            Ok(SizeRange { start_b, end_b })
        } else {
            let size = parse_size(s)?;
            Ok(SizeRange {
                start_b: 0,
                end_b: Some(size),
            })
        }
    }
}

fn try_split(s: &str) -> Option<(&str, &str)> {
    if let Some(pos) = s.find("..") {
        return Some((&s[..pos], &s[pos + 2..]));
    }

    if let Some(pos) = s.find(',') {
        return Some((&s[..pos], &s[pos + 1..]));
    }

    // A leading dash means open-start range: "-20G" => ("", "20G")
    if let Some(stripped) = s.strip_prefix('-') {
        return Some(("", stripped));
    }

    find_dash_delimiter(s)
}

fn find_dash_delimiter(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    for i in (1..bytes.len()).rev() {
        if bytes[i] == b'-' {
            let prev = bytes[i - 1];
            // A delimiter dash follows a digit or a unit letter (b/B/i/I/k/K/m/M/g/G/t/T/p/P/e/E)
            // but NOT after 'e'/'E' which would be scientific notation
            if prev.is_ascii_digit()
                || matches!(
                    prev,
                    b'b' | b'B'
                        | b'i'
                        | b'I'
                        | b'k'
                        | b'K'
                        | b'm'
                        | b'M'
                        | b'g'
                        | b'G'
                        | b't'
                        | b'T'
                        | b'p'
                        | b'P'
                )
            {
                return Some((&s[..i], &s[i + 1..]));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size_basic() {
        assert_eq!(parse_size("0"), Ok(0));
        assert_eq!(parse_size("3"), Ok(3));
        assert_eq!(parse_size("32"), Ok(32));
        assert_eq!(parse_size("_5_"), Ok(5));
    }

    #[test]
    fn test_parse_size_units() {
        assert_eq!(parse_size("1kB"), Ok(1_000));
        assert_eq!(parse_size("2MB"), Ok(2_000_000));
        assert_eq!(parse_size("3GB"), Ok(3_000_000_000));
        assert_eq!(parse_size("4TB"), Ok(4_000_000_000_000));
        assert_eq!(parse_size("5PB"), Ok(5_000_000_000_000_000));
        assert_eq!(parse_size("6EB"), Ok(6_000_000_000_000_000_000));
    }

    #[test]
    fn test_parse_size_binary() {
        assert_eq!(parse_size("7KiB"), Ok(7 << 10));
        assert_eq!(parse_size("8MiB"), Ok(8 << 20));
        assert_eq!(parse_size("9GiB"), Ok(9 << 30));
        assert_eq!(parse_size("10TiB"), Ok(10 << 40));
        assert_eq!(parse_size("11PiB"), Ok(11 << 50));
        assert_eq!(parse_size("12EiB"), Ok(12 << 60));
        assert_eq!(parse_size("1mib"), Ok(1_048_576));
    }

    #[test]
    fn test_parse_size_fractional() {
        assert_eq!(parse_size("1.1K"), Ok(1100));
        assert_eq!(parse_size("1.2345K"), Ok(1235));
        assert_eq!(parse_size("1.2345m"), Ok(1_234_500));
        assert_eq!(parse_size("5.k"), Ok(5000));
        assert_eq!(parse_size("0.0025KB"), Ok(3));
    }

    #[test]
    fn test_parse_size_scientific() {
        assert_eq!(parse_size("1e2KIB"), Ok(102_400));
        assert_eq!(parse_size("1E+6"), Ok(1_000_000));
        assert_eq!(parse_size("1e-4MiB"), Ok(105));
        assert_eq!(parse_size("1.1e2"), Ok(110));
        assert_eq!(parse_size("5.7E3"), Ok(5700));
    }

    #[test]
    fn test_parse_size_no_spaces() {
        assert_eq!(parse_size("1 K"), Err(RangeError::SpaceNotAllowed));
        assert_eq!(parse_size("1 234 567"), Err(RangeError::SpaceNotAllowed));
    }

    #[test]
    fn test_parse_size_errors() {
        assert_eq!(parse_size(""), Err(RangeError::Empty));
        assert_eq!(parse_size(".5k"), Err(RangeError::InvalidDigit));
        assert_eq!(parse_size("k"), Err(RangeError::Empty));
        assert_eq!(parse_size("-1"), Err(RangeError::InvalidDigit));
    }

    #[test]
    fn test_size_range_single_value() {
        assert_eq!(
            SizeRange::from_str("100K"),
            Ok(SizeRange {
                start_b: 0,
                end_b: Some(100_000)
            })
        );
    }

    #[test]
    fn test_size_range_dash() {
        assert_eq!(
            SizeRange::from_str("10K-20G"),
            Ok(SizeRange {
                start_b: 10_000,
                end_b: Some(20_000_000_000)
            })
        );
    }

    #[test]
    fn test_size_range_dotdot() {
        assert_eq!(
            SizeRange::from_str("10K..20G"),
            Ok(SizeRange {
                start_b: 10_000,
                end_b: Some(20_000_000_000)
            })
        );
    }

    #[test]
    fn test_size_range_comma() {
        assert_eq!(
            SizeRange::from_str("10K,20G"),
            Ok(SizeRange {
                start_b: 10_000,
                end_b: Some(20_000_000_000)
            })
        );
    }

    #[test]
    fn test_size_range_open_end() {
        assert_eq!(
            SizeRange::from_str("10K-"),
            Ok(SizeRange {
                start_b: 10_000,
                end_b: None
            })
        );
        assert_eq!(
            SizeRange::from_str("10K.."),
            Ok(SizeRange {
                start_b: 10_000,
                end_b: None
            })
        );
    }

    #[test]
    fn test_size_range_open_start() {
        assert_eq!(
            SizeRange::from_str("-20G"),
            Ok(SizeRange {
                start_b: 0,
                end_b: Some(20_000_000_000)
            })
        );
        assert_eq!(
            SizeRange::from_str("..20G"),
            Ok(SizeRange {
                start_b: 0,
                end_b: Some(20_000_000_000)
            })
        );
    }

    #[test]
    fn test_size_range_invalid_order() {
        assert!(SizeRange::from_str("20G-10K").is_err());
    }

    #[test]
    fn test_size_range_with_scientific_and_dash() {
        // 1e2K = 100K = 100_000, should not confuse e- with delimiter
        assert_eq!(
            SizeRange::from_str("1e2K-20G"),
            Ok(SizeRange {
                start_b: 100_000,
                end_b: Some(20_000_000_000)
            })
        );
    }
}
