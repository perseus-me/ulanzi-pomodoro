//! Persistent storage backed by the raw `nvs` flash partition.
//!
//! We deliberately do *not* use the ESP-IDF NVS key-value format. Instead the
//! 24 KB partition is sliced into 4 KB sectors that we manage ourselves:
//!
//! ```text
//!   0x9000  Settings copy A   (sector)
//!   0xA000  Settings copy B   (sector)
//!   0xB000  Stats copy A      (sector)
//!   0xC000  Stats copy B      (sector)
//!   0xD000  reserved          (sector)
//!   0xE000  reserved          (sector)
//! ```
//!
//! For each kind we keep two slots (A and B) with a monotonic sequence number
//! and a CRC. On load we pick the slot with the highest sequence whose CRC
//! verifies; on save we overwrite the *other* slot — that way an interrupted
//! write never destroys the previously good copy.

pub mod nvs;
pub mod settings;
pub mod stats;

pub use nvs::{
    SETTINGS_OFFSET_A, SETTINGS_OFFSET_B, SLOT_SIZE, STATS_OFFSET_A, STATS_OFFSET_B, StorageError,
};
pub use settings::Settings;
pub use stats::{DayStats, MAX_STATS_DAYS, Stats};
