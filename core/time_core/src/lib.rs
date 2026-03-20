//! Pure time conversion utilities.
//!
//! No I/O, no system clock reads. All functions take epoch seconds as input.

/// Decompose Unix epoch seconds into (year, month, day, hour, minute, second).
/// Month and day are 1-based. Handles 1970–2099 correctly.
fn epoch_to_parts(epoch_secs: u64) -> (i64, u8, u8, u8, u8, u8) {
    let total_secs = epoch_secs;
    let hour = ((total_secs % 86400) / 3600) as u8;
    let minute = ((total_secs % 3600) / 60) as u8;
    let second = (total_secs % 60) as u8;

    let days = total_secs / 86400;
    let mut year = 1970i64;
    let mut remaining = days as i64;

    loop {
        let year_days = if is_leap(year) { 366 } else { 365 };
        if remaining < year_days {
            break;
        }
        remaining -= year_days;
        year += 1;
    }

    let month_days = month_lengths(is_leap(year));
    let mut month = 0u8;
    for md in &month_days {
        if remaining < *md as i64 {
            break;
        }
        remaining -= *md as i64;
        month += 1;
    }

    (year, month + 1, remaining as u8 + 1, hour, minute, second)
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn month_lengths(leap: bool) -> [u8; 12] {
    [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
}

/// Convert Unix epoch seconds to a `YYYY-MM-DD` civil date string (UTC).
pub fn civil_date(epoch_secs: u64) -> String {
    let (year, month, day, _, _, _) = epoch_to_parts(epoch_secs);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Convert Unix epoch seconds to an ISO 8601 Zulu timestamp: `YYYY-MM-DDTHH:MM:SSZ` (UTC).
pub fn iso_zulu(epoch_secs: u64) -> String {
    let (year, month, day, hour, minute, second) = epoch_to_parts(epoch_secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- civil_date ---

    #[test]
    fn civil_date_epoch_zero() {
        assert_eq!(civil_date(0), "1970-01-01");
    }

    #[test]
    fn civil_date_known_2024_01_01() {
        assert_eq!(civil_date(1704067200), "2024-01-01");
    }

    #[test]
    fn civil_date_leap_day_2024() {
        assert_eq!(civil_date(1709164800), "2024-02-29");
    }

    #[test]
    fn civil_date_year_boundary_2023_12_31() {
        // 2024-01-01 00:00:00 minus 1 second
        assert_eq!(civil_date(1704067199), "2023-12-31");
    }

    #[test]
    fn civil_date_2026_03_20() {
        // 2026-03-20 00:00:00 UTC
        assert_eq!(civil_date(1773964800), "2026-03-20");
    }

    #[test]
    fn civil_date_end_of_feb_non_leap() {
        // 2023-02-28 12:00:00 UTC = 1677585600
        assert_eq!(civil_date(1677585600), "2023-02-28");
    }

    #[test]
    fn civil_date_march_1_non_leap() {
        // 2023-03-01 00:00:00 UTC = 1677628800
        assert_eq!(civil_date(1677628800), "2023-03-01");
    }

    // --- iso_zulu ---

    #[test]
    fn iso_zulu_epoch_zero() {
        assert_eq!(iso_zulu(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn iso_zulu_known_timestamp() {
        // 2024-01-01 12:30:45 UTC = 1704067200 + 45045
        assert_eq!(iso_zulu(1704112245), "2024-01-01T12:30:45Z");
    }

    #[test]
    fn iso_zulu_end_of_day() {
        // 2024-01-01 23:59:59 UTC = 1704067200 + 86399
        assert_eq!(iso_zulu(1704153599), "2024-01-01T23:59:59Z");
    }

    #[test]
    fn iso_zulu_midnight_boundary() {
        // 2024-01-02 00:00:00 UTC = 1704067200 + 86400
        assert_eq!(iso_zulu(1704153600), "2024-01-02T00:00:00Z");
    }

    // --- is_leap ---

    #[test]
    fn leap_year_2000() {
        assert!(is_leap(2000)); // divisible by 400
    }

    #[test]
    fn not_leap_1900() {
        assert!(!is_leap(1900)); // divisible by 100 but not 400
    }

    #[test]
    fn leap_year_2024() {
        assert!(is_leap(2024)); // divisible by 4
    }

    #[test]
    fn not_leap_2023() {
        assert!(!is_leap(2023));
    }
}
