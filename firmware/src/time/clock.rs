//! Unix-time / civil-date conversions.
//!
//! Implements Howard Hinnant's [civil_from_days] algorithm, accurate from
//! year -32767 to +32767 (well past anything we care about). The code is
//! `const`-friendly and depends only on integer math, so it works under
//! `no_std` and is easy to unit-test on the host.
//!
//! [civil_from_days]: https://howardhinnant.github.io/date_algorithms.html#civil_from_days

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CivilDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// Convert a unix timestamp (seconds since 1970-01-01 UTC) to a civil date in
/// a given timezone (`tz_offset_min` is added to UTC before the split).
pub fn civil_from_unix(unix: i64, tz_offset_min: i16) -> CivilDate {
    let secs = unix + (tz_offset_min as i64) * 60;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400) as u32;

    // Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u8;
    let year = (y + if month <= 2 { 1 } else { 0 }) as i32;

    let hour = (rem / 3600) as u8;
    let minute = ((rem / 60) % 60) as u8;
    let second = (rem % 60) as u8;

    CivilDate {
        year,
        month,
        day,
        hour,
        minute,
        second,
    }
}

/// Pack a civil date into `YYYY * 10000 + MM * 100 + DD`.
pub fn yyyymmdd(date: CivilDate) -> u32 {
    let y = if date.year < 0 { 0 } else { date.year as u32 };
    y * 10_000 + date.month as u32 * 100 + date.day as u32
}

/// Anchored wall-clock.
///
/// Internally stores the unix-second base together with the monotonic ms at
/// the time the anchor was set. Calling [`Clock::now_unix`] with the current
/// monotonic ms returns a `i64` second-precision unix timestamp.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    base_unix: i64,
    base_monotonic_ms: u64,
    pub synced: bool,
}

impl Clock {
    /// Create a clock with no anchoring yet (`now_unix` will return `None`).
    pub const fn unset() -> Self {
        Self {
            base_unix: 0,
            base_monotonic_ms: 0,
            synced: false,
        }
    }

    /// Initialise from `last_seen_unix` (or any other "best guess" value).
    /// The `synced` flag stays `false`, marking the time as approximate.
    pub fn from_last_seen(last_seen_unix: i64, now_monotonic_ms: u64) -> Self {
        Self {
            base_unix: last_seen_unix,
            base_monotonic_ms: now_monotonic_ms,
            synced: false,
        }
    }

    /// Anchor to a known-authoritative source (typically NTP). Subsequent
    /// `now_unix` calls will be accurate to within the monotonic counter
    /// drift (~20 ppm for a free-running ESP32).
    pub fn anchor(&mut self, unix_time: i64, now_monotonic_ms: u64) {
        self.base_unix = unix_time;
        self.base_monotonic_ms = now_monotonic_ms;
        self.synced = true;
    }

    /// Returns the current unix time in seconds, or `None` if no anchor has
    /// ever been established.
    pub fn now_unix(&self, now_monotonic_ms: u64) -> Option<i64> {
        if self.base_unix <= 0 && !self.synced {
            // base_unix == 0 with !synced means we've never been told anything
            // useful (no NVS history and no NTP).
            return None;
        }
        let delta_ms = now_monotonic_ms.saturating_sub(self.base_monotonic_ms);
        Some(self.base_unix + (delta_ms / 1000) as i64)
    }

    pub fn today_yyyymmdd(&self, now_monotonic_ms: u64, tz_offset_min: i16) -> Option<u32> {
        let unix = self.now_unix(now_monotonic_ms)?;
        Some(yyyymmdd(civil_from_unix(unix, tz_offset_min)))
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::unset()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_epoch_is_1970_01_01() {
        let date = civil_from_unix(0, 0);
        assert_eq!(date.year, 1970);
        assert_eq!(date.month, 1);
        assert_eq!(date.day, 1);
        assert_eq!(date.hour, 0);
    }

    #[test]
    fn known_timestamp_may_2026() {
        // 2026-05-12 00:14 UTC corresponds to unix 1778285640
        let date = civil_from_unix(1_778_285_640, 0);
        assert_eq!(date.year, 2026);
        assert_eq!(date.month, 5);
        assert_eq!(date.day, 12);
        assert_eq!(date.hour, 0);
        assert_eq!(date.minute, 14);
    }

    #[test]
    fn tz_offset_shifts_date() {
        // 2026-05-12 00:14 UTC + 3h = 2026-05-12 03:14 Moscow time, still
        // the same date — but going backwards by 3h flips the date.
        let plus3 = civil_from_unix(1_778_285_640, 180);
        assert_eq!(plus3.hour, 3);
        assert_eq!(plus3.day, 12);

        // 23:14 UTC → 19:14 UTC-4 (still 2026-05-11).
        let minus4 = civil_from_unix(1_778_285_640 + 23 * 3600, -240);
        assert_eq!(minus4.day, 12);
        assert_eq!(minus4.hour, 19);
    }

    #[test]
    fn yyyymmdd_format() {
        let date = CivilDate {
            year: 2026,
            month: 5,
            day: 12,
            hour: 0,
            minute: 0,
            second: 0,
        };
        assert_eq!(yyyymmdd(date), 20_260_512);
    }

    #[test]
    fn clock_advances_with_monotonic_ms() {
        let mut clock = Clock::unset();
        clock.anchor(1_778_285_640, 100);
        assert_eq!(clock.now_unix(100), Some(1_778_285_640));
        assert_eq!(clock.now_unix(100 + 2_500), Some(1_778_285_642));
    }
}
