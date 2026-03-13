//! Unit conversion tables and logic for .tomlx.
//!
//! All conversions go through a canonical base (seconds for time, bytes for size).

use std::collections::HashMap;
use std::sync::LazyLock;

use super::types::{TargetFamily, TimeBase};

/// Unit family (for cross-family detection).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnitFamily {
    Time,
    Size,
}

/// Unit information.
#[derive(Debug, Clone)]
pub struct UnitInfo {
    pub canonical: &'static str,
    pub family: UnitFamily,
    pub to_base: f64,
}

/// Static unit lookup table.
static UNITS: LazyLock<HashMap<&'static str, UnitInfo>> = LazyLock::new(|| {
    let mut m = HashMap::new();

    let time_units = [
        ("ms", 0.001), ("millisecond", 0.001), ("milliseconds", 0.001), ("millis", 0.001),
        ("s", 1.0), ("sec", 1.0), ("secs", 1.0), ("second", 1.0), ("seconds", 1.0),
        ("m", 60.0), ("min", 60.0), ("mins", 60.0), ("minute", 60.0), ("minutes", 60.0),
        ("h", 3600.0), ("hr", 3600.0), ("hrs", 3600.0), ("hour", 3600.0), ("hours", 3600.0),
        ("d", 86400.0), ("day", 86400.0), ("days", 86400.0),
        ("w", 604800.0), ("wk", 604800.0), ("week", 604800.0), ("weeks", 604800.0),
    ];

    for (alias, base) in time_units {
        m.insert(alias, UnitInfo { canonical: alias, family: UnitFamily::Time, to_base: base });
    }

    let size_decimal = [
        ("b", 1.0), ("byte", 1.0), ("bytes", 1.0),
        ("kb", 1_000.0), ("kilobyte", 1_000.0), ("kilobytes", 1_000.0),
        ("mb", 1_000_000.0), ("megabyte", 1_000_000.0), ("megabytes", 1_000_000.0),
        ("gb", 1_000_000_000.0), ("gigabyte", 1_000_000_000.0), ("gigabytes", 1_000_000_000.0),
        ("tb", 1_000_000_000_000.0), ("terabyte", 1_000_000_000_000.0), ("terabytes", 1_000_000_000_000.0),
    ];

    for (alias, base) in size_decimal {
        m.insert(alias, UnitInfo { canonical: alias, family: UnitFamily::Size, to_base: base });
    }

    let size_binary = [
        ("kib", 1_024.0), ("kibibyte", 1_024.0), ("kibibytes", 1_024.0),
        ("mib", 1_048_576.0), ("mebibyte", 1_048_576.0), ("mebibytes", 1_048_576.0),
        ("gib", 1_073_741_824.0), ("gibibyte", 1_073_741_824.0), ("gibibytes", 1_073_741_824.0),
        ("tib", 1_099_511_627_776.0), ("tebibyte", 1_099_511_627_776.0), ("tebibytes", 1_099_511_627_776.0),
    ];

    for (alias, base) in size_binary {
        m.insert(alias, UnitInfo { canonical: alias, family: UnitFamily::Size, to_base: base });
    }

    m
});

/// Look up a unit by name.
pub fn lookup_unit(name: &str) -> Option<&'static UnitInfo> {
    UNITS.get(name.to_lowercase().as_str())
}

/// Get suggestions for an unknown unit.
pub fn suggest_units(unknown: &str) -> Vec<String> {
    let unknown = unknown.to_lowercase();
    let mut suggestions: Vec<(usize, String)> = UNITS
        .keys()
        .filter_map(|&u| {
            let distance = levenshtein(&unknown, u);
            if distance <= 2 {
                Some((distance, u.to_string()))
            } else {
                None
            }
        })
        .collect();

    suggestions.sort_by_key(|(d, _)| *d);
    suggestions.into_iter().take(3).map(|(_, s)| s).collect()
}

fn levenshtein(source: &str, target: &str) -> usize {
    let source_chars: Vec<char> = source.chars().collect();
    let target_chars: Vec<char> = target.chars().collect();
    let source_len = source_chars.len();
    let target_len = target_chars.len();
    let mut dp = vec![vec![0; target_len + 1]; source_len + 1];

    for i in 0..=source_len { dp[i][0] = i; }
    for j in 0..=target_len { dp[0][j] = j; }

    for i in 1..=source_len {
        for j in 1..=target_len {
            let cost = if source_chars[i - 1] == target_chars[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }

    dp[source_len][target_len]
}

/// Get the expected family for a target.
pub fn target_family(target: &TargetFamily) -> Option<UnitFamily> {
    match target {
        TargetFamily::Time(_) => Some(UnitFamily::Time),
        TargetFamily::Size => Some(UnitFamily::Size),
        TargetFamily::Path => None,
    }
}

/// Convert a value from source unit to target.
pub fn convert_unit(value: f64, source_unit: &str, target: &TargetFamily) -> Result<f64, String> {
    let unit_info = lookup_unit(source_unit).ok_or_else(|| {
        let suggestions = suggest_units(source_unit);
        if suggestions.is_empty() {
            format!("Unknown unit '{}'", source_unit)
        } else {
            format!("Unknown unit '{}'. Did you mean: {}?", source_unit, suggestions.join(", "))
        }
    })?;

    let expected_family = target_family(target).ok_or_else(|| {
        "Cannot convert to path target - path sections don't use units".to_string()
    })?;

    if unit_info.family != expected_family {
        let source_family = match unit_info.family {
            UnitFamily::Time => "time",
            UnitFamily::Size => "size",
        };
        let target_family_str = match expected_family {
            UnitFamily::Time => "time",
            UnitFamily::Size => "size",
        };
        return Err(format!(
            "{} unit '{}' cannot convert to {} target",
            source_family.to_uppercase(), source_unit, target_family_str
        ));
    }

    let base_value = value * unit_info.to_base;

    let result = match target {
        TargetFamily::Time(time_base) => base_value * time_base.from_seconds_multiplier(),
        TargetFamily::Size => base_value,
        TargetFamily::Path => unreachable!(),
    };

    Ok(result)
}

/// Get the target unit name for output metadata.
pub fn target_unit_name(target: &TargetFamily) -> &'static str {
    match target {
        TargetFamily::Time(TimeBase::Seconds) => "seconds",
        TargetFamily::Time(TimeBase::Milliseconds) => "milliseconds",
        TargetFamily::Size => "bytes",
        TargetFamily::Path => "path",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_time() {
        let info = lookup_unit("minutes").unwrap();
        assert_eq!(info.family, UnitFamily::Time);
        assert_eq!(info.to_base, 60.0);
    }

    #[test]
    fn test_lookup_size() {
        let info = lookup_unit("mb").unwrap();
        assert_eq!(info.family, UnitFamily::Size);
        assert_eq!(info.to_base, 1_000_000.0);
    }

    #[test]
    fn test_convert_minutes_to_seconds() {
        let target = TargetFamily::Time(TimeBase::Seconds);
        assert_eq!(convert_unit(15.0, "minutes", &target).unwrap(), 900.0);
    }

    #[test]
    fn test_convert_mb_to_bytes() {
        let target = TargetFamily::Size;
        assert_eq!(convert_unit(500.0, "mb", &target).unwrap(), 500_000_000.0);
    }

    #[test]
    fn test_cross_family_error() {
        let target = TargetFamily::Time(TimeBase::Seconds);
        assert!(convert_unit(100.0, "mb", &target).is_err());
    }

    #[test]
    fn test_suggest_units() {
        let suggestions = suggest_units("minuts");
        assert!(!suggestions.is_empty());
    }
}
