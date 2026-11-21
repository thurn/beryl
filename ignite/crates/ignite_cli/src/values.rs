//! Parsers for flag values that `clap` cannot read by itself.

/// An interval in whole hours, from `6`, `6h` or `2d`.
pub fn parse_every(text: &str) -> Result<u32, String> {
    let (number, per) = if let Some(days) = text.strip_suffix(['d', 'D']) {
        (days, 24)
    } else {
        (text.strip_suffix(['h', 'H']).unwrap_or(text), 1)
    };
    match number.parse::<u32>().ok().and_then(|n| n.checked_mul(per)) {
        Some(hours) if hours > 0 => Ok(hours),
        _ => Err(format!(
            "{text:?} is not an interval such as 6, 6h or 2d (whole hours or days, at least 1)"
        )),
    }
}

/// A weekly saving, from `<unit>:<amount>` such as `agent-hours:12`: the amount and the unit.
pub fn parse_saving(text: &str) -> Result<(f64, String), String> {
    let parsed = text.split_once(':').and_then(|(unit, amount)| {
        let amount = amount.parse::<f64>().ok()?;
        let known = ignite_core::shares::Unit::parse(unit).is_some();
        (known && amount.is_finite() && amount > 0.0).then(|| (amount, unit.to_owned()))
    });
    parsed.ok_or_else(|| {
        format!(
            "{text:?} is not a saving such as agent-hours:12 (dollars, agent-hours or \
             core-hours a week, above 0)"
        )
    })
}

pub fn parse_priority(text: &str) -> Result<u8, String> {
    let digits = text.strip_prefix(['P', 'p']).unwrap_or(text);
    match digits.parse::<u8>() {
        Ok(priority) if priority <= 4 => Ok(priority),
        _ => Err(format!("{text:?} is not a priority from P0 to P4")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals_are_hours_or_days() {
        assert_eq!(parse_every("6"), Ok(6));
        assert_eq!(parse_every("6h"), Ok(6));
        assert_eq!(parse_every("2d"), Ok(48));
        assert!(parse_every("0").is_err());
        assert!(parse_every("1.5h").is_err());
        assert!(parse_every("weekly").is_err());
    }

    #[test]
    fn a_saving_is_a_known_unit_and_an_amount_above_zero() {
        assert_eq!(
            parse_saving("agent-hours:12.5"),
            Ok((12.5, "agent-hours".to_owned()))
        );
        assert_eq!(parse_saving("dollars:40"), Ok((40.0, "dollars".to_owned())));
        for refused in [
            "12",
            "minutes:12",
            "agent-hours:0",
            "agent-hours:-3",
            "core-hours:nan",
        ] {
            assert!(parse_saving(refused).is_err(), "{refused}");
        }
    }
}
