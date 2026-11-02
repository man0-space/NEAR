use crate::error::Man0Error;

pub const MAX_DECIMALS: u8 = 18;
pub const WAD: i128 = 1_000_000_000_000_000_000;
pub const MAX_FEEDBACK_ABS: i128 = 100_000_000_000_000_000_000_000_000_000_000_000_000; // 1e38

pub fn validate_feedback_value(value: i128, decimals: u8) -> Result<(), Man0Error> {
    if decimals > MAX_DECIMALS {
        return Err(Man0Error::InvalidDecimals);
    }
    if value == i128::MIN || value.abs() > MAX_FEEDBACK_ABS {
        return Err(Man0Error::ValueOutOfBounds);
    }
    Ok(())
}

pub fn pow10(exp: u8) -> Result<i128, Man0Error> {
    if exp > MAX_DECIMALS {
        return Err(Man0Error::InvalidDecimals);
    }
    10i128
        .checked_pow(exp as u32)
        .ok_or(Man0Error::ArithmeticOverflow)
}

/// Normalize a signed value with `decimals` (0..=18) into 18-decimal WAD representation.
pub fn to_wad(value: i128, decimals: u8) -> Result<i128, Man0Error> {
    validate_feedback_value(value, decimals)?;
    let scale = pow10(MAX_DECIMALS - decimals)?;
    value
        .checked_mul(scale)
        .ok_or(Man0Error::ArithmeticOverflow)
}

/// Convert an 18-decimal WAD value into `target_decimals` (0..=18).
pub fn from_wad(wad: i128, target_decimals: u8) -> Result<i128, Man0Error> {
    if target_decimals > MAX_DECIMALS {
        return Err(Man0Error::InvalidDecimals);
    }
    let scale = pow10(MAX_DECIMALS - target_decimals)?;
    wad.checked_div(scale).ok_or(Man0Error::ArithmeticOverflow)
}

/// Aggregate a slice of `(value, value_decimals)` feedback entries into:
/// `(count, average_value_in_mode_decimals, mode_decimals)` matching ERC-8004 Reputation rules.
pub fn aggregate_summary(entries: &[(i128, u8)]) -> Result<(u64, i128, u8), Man0Error> {
    if entries.is_empty() {
        return Ok((0, 0, 0));
    }

    let mut total_wad: i128 = 0;
    let mut decimal_counts = [0u64; (MAX_DECIMALS as usize) + 1];

    for &(val, dec) in entries {
        let wad = to_wad(val, dec)?;
        total_wad = total_wad
            .checked_add(wad)
            .ok_or(Man0Error::ArithmeticOverflow)?;
        decimal_counts[dec as usize] = decimal_counts[dec as usize]
            .checked_add(1)
            .ok_or(Man0Error::ArithmeticOverflow)?;
    }

    let mut mode_decimals: u8 = 0;
    let mut max_count: u64 = 0;
    for (dec, &cnt) in decimal_counts.iter().enumerate() {
        if cnt > max_count {
            max_count = cnt;
            mode_decimals = dec as u8;
        }
    }

    let count = entries.len() as u64;
    let avg_wad = total_wad
        .checked_div(count as i128)
        .ok_or(Man0Error::ArithmeticOverflow)?;
    let summary_value = from_wad(avg_wad, mode_decimals)?;

    Ok((count, summary_value, mode_decimals))
}
