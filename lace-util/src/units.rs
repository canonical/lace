// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
//! Unit types for formatted display of common values.

use core::fmt::{self, Display};

/// A Unix timestamp (seconds since 1970-01-01 00:00:00 UTC)
///
/// Formats as "YYYY-MM-DD  HH:MM:SS UTC" (note the double space between
/// date and time).
///
/// # Example
///
/// ```
/// use lace_util::units::Timestamp;
///
/// let ts = Timestamp(1234567890);
/// assert_eq!(format!("{}", ts), "2009-02-13  23:31:30 UTC");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Timestamp(pub i64);

/// A size in bytes
///
/// Formats as "N Bytes = X.Y UiB", showing the byte count followed by the
/// most appropriate binary unit (KiB, MiB, GiB, etc.).
///
/// # Example
///
/// ```
/// use lace_util::units::Size;
///
/// let size = Size(3891);
/// assert_eq!(format!("{}", size), "3891 Bytes = 3.8 KiB");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size(pub u64);

impl Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let timestamp = self.0;
        let (days, remaining_secs) = {
            let days = timestamp / 86400;
            let remaining_secs = timestamp % 86400;

            // Handle negative timestamps (before 1970)
            if remaining_secs < 0 {
                (days - 1, remaining_secs + 86400)
            } else {
                (days, remaining_secs)
            }
        };

        let hours = remaining_secs / 3600;
        let minutes = (remaining_secs % 3600) / 60;
        let seconds = remaining_secs % 60;

        // Calculate year, month, day from days since epoch
        let (year, month, day) = days_to_ymd(days);

        write!(
            f,
            "{:4}-{:02}-{:02}  {:2}:{:02}:{:02} UTC",
            year, month, day, hours, minutes, seconds
        )
    }
}

impl Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const UNITS: &[(u64, &str)] = &[
            (1 << 60, "EiB"),
            (1 << 50, "PiB"),
            (1 << 40, "TiB"),
            (1 << 30, "GiB"),
            (1 << 20, "MiB"),
            (1 << 10, "KiB"),
        ];

        let size = self.0;
        write!(f, "{} Bytes = ", size)?;

        for &(threshold, unit) in UNITS {
            if size >= threshold {
                let whole = size / threshold;
                let frac = ((size % threshold) * 10 + threshold / 2) / threshold;

                // Handle rounding overflow
                let (whole, frac) = if frac >= 10 {
                    (whole + 1, frac - 10)
                } else {
                    (whole, frac)
                };

                if frac > 0 {
                    return write!(f, "{}.{} {}", whole, frac, unit);
                } else {
                    return write!(f, "{} {}", whole, unit);
                }
            }
        }

        // Less than 1 KiB, just show bytes again
        write!(f, "{} Bytes", size)
    }
}

/// Convert days since Unix epoch to (year, month, day)
///
/// Uses the Howard Hinnant algorithm for civil calendar calculations.
/// See: http://howardhinnant.github.io/date_algorithms.html
fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    // Days from year 0 to 1970-01-01
    const DAYS_TO_1970: i64 = 719468;

    let days = days + DAYS_TO_1970;

    // Era (400-year period)
    let era = if days >= 0 {
        days / 146097
    } else {
        (days - 146096) / 146097
    };
    let doe = (days - era * 146097) as u32; // Day of era [0, 146096]

    // Year of era [0, 399]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // Day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // Month [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // Day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // Month [1, 12]
    let y = if m <= 2 { y + 1 } else { y };

    (y as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate alloc;
    use alloc::format;

    #[test]
    fn test_timestamp_epoch() {
        assert_eq!(format!("{}", Timestamp(0)), "1970-01-01   0:00:00 UTC");
    }

    #[test]
    fn test_timestamp_known_value() {
        // 1234567890 = 2009-02-13 23:31:30 UTC (a famous Unix timestamp)
        assert_eq!(
            format!("{}", Timestamp(1234567890)),
            "2009-02-13  23:31:30 UTC"
        );
    }

    #[test]
    fn test_timestamp_y2k() {
        // 946684800 = 2000-01-01 00:00:00 UTC
        assert_eq!(
            format!("{}", Timestamp(946684800)),
            "2000-01-01   0:00:00 UTC"
        );
    }

    #[test]
    fn test_timestamp_leap_year() {
        // 2020-02-29 12:00:00 UTC = 1582977600
        assert_eq!(
            format!("{}", Timestamp(1582977600)),
            "2020-02-29  12:00:00 UTC"
        );
    }

    #[test]
    fn test_timestamp_end_of_year() {
        // 2023-12-31 23:59:59 UTC = 1704067199
        assert_eq!(
            format!("{}", Timestamp(1704067199)),
            "2023-12-31  23:59:59 UTC"
        );
    }

    #[test]
    fn test_size_small() {
        assert_eq!(format!("{}", Size(327)), "327 Bytes = 327 Bytes");
    }

    #[test]
    fn test_size_exactly_one_kib() {
        assert_eq!(format!("{}", Size(1024)), "1024 Bytes = 1 KiB");
    }

    #[test]
    fn test_size_kib() {
        assert_eq!(format!("{}", Size(3891)), "3891 Bytes = 3.8 KiB");
    }

    #[test]
    fn test_size_one_mib() {
        assert_eq!(format!("{}", Size(1 << 20)), "1048576 Bytes = 1 MiB");
    }

    #[test]
    fn test_size_mib_with_frac() {
        // 1.5 MiB
        assert_eq!(
            format!("{}", Size((1 << 20) + (512 << 10))),
            "1572864 Bytes = 1.5 MiB"
        );
    }

    #[test]
    fn test_size_zero() {
        assert_eq!(format!("{}", Size(0)), "0 Bytes = 0 Bytes");
    }

    #[test]
    fn test_size_gib() {
        // 2.5 GiB
        assert_eq!(
            format!("{}", Size((2 << 30) + (512 << 20))),
            "2684354560 Bytes = 2.5 GiB"
        );
    }
}
