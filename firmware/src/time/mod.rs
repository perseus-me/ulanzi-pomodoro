//! Wall-clock time. Anchored once at boot (from NTP if reachable, otherwise
//! from `Settings::last_seen_unix`) and then derived from the monotonic
//! counter for the rest of the session.

pub mod clock;
pub mod ntp;

pub use clock::{CivilDate, Clock, civil_from_unix, yyyymmdd};
pub use ntp::{NtpRequest, parse_ntp_response};
