//! Persisted user-tunable settings (presets, brightness, time zone).

use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};

use crate::{
    pomodoro::Preset,
    storage::nvs::{
        self, LoadedRecord, Record, SETTINGS_OFFSET_A, SETTINGS_OFFSET_B, StorageError,
    },
};

/// Encoded payload layout (24 bytes):
///
/// ```text
///   0..2    preset0.work_min   (u16 LE)
///   2..4    preset0.rest_min   (u16 LE)
///   4..6    preset1.work_min
///   6..8    preset1.rest_min
///   8..10   preset2.work_min
///   10..12  preset2.rest_min
///   12      brightness          (u8)
///   13      reserved            (0xFF)
///   14..16  tz_offset_min       (i16 LE)
///   16..24  last_seen_unix      (i64 LE)
/// ```
const ENCODED_LEN: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub presets: [Preset; 3],
    pub brightness: u8,
    pub tz_offset_min: i16,
    /// Last unix timestamp we managed to observe. Used as the starting clock
    /// when NTP is unavailable so the device doesn't snap back to the build
    /// date after every cold boot.
    pub last_seen_unix: i64,
}

impl Settings {
    pub const fn defaults() -> Self {
        Self {
            presets: crate::pomodoro::presets::DEFAULT_PRESETS,
            brightness: 64,
            tz_offset_min: 0,
            last_seen_unix: 0,
        }
    }

    /// Load the freshest valid copy from flash, or [`Settings::defaults`] if
    /// neither slot is usable (first boot, corrupted flash, etc.).
    pub fn load<F: ReadNorFlash>(flash: &mut F) -> (Self, SettingsStore) {
        match nvs::load_latest::<F, Settings>(flash, SETTINGS_OFFSET_A, SETTINGS_OFFSET_B) {
            Some(LoadedRecord {
                record,
                seq,
                from_offset,
            }) => (
                record,
                SettingsStore {
                    seq,
                    last_offset: Some(from_offset),
                },
            ),
            None => (
                Self::defaults(),
                SettingsStore {
                    seq: 0,
                    last_offset: None,
                },
            ),
        }
    }
}

/// Bookkeeping for atomic A/B writes. Returned from [`Settings::load`] and
/// fed back into [`SettingsStore::save`].
pub struct SettingsStore {
    seq: u64,
    last_offset: Option<u32>,
}

impl SettingsStore {
    pub fn save<F: NorFlash>(
        &mut self,
        flash: &mut F,
        settings: &Settings,
    ) -> Result<(), StorageError> {
        let next_seq = self.seq.wrapping_add(1);
        let written = nvs::write_alternate::<F, Settings>(
            flash,
            SETTINGS_OFFSET_A,
            SETTINGS_OFFSET_B,
            self.last_offset,
            settings,
            next_seq,
        )?;
        self.seq = next_seq;
        self.last_offset = Some(written);
        Ok(())
    }
}

impl Record for Settings {
    const MAGIC: [u8; 4] = *b"POSE";
    const VERSION: u8 = 1;
    const PAYLOAD_MAX: usize = ENCODED_LEN;

    fn encode(&self, out: &mut [u8]) -> usize {
        debug_assert!(out.len() >= ENCODED_LEN);
        for (i, preset) in self.presets.iter().enumerate() {
            let base = i * 4;
            out[base..base + 2].copy_from_slice(&preset.work_min.to_le_bytes());
            out[base + 2..base + 4].copy_from_slice(&preset.rest_min.to_le_bytes());
        }
        out[12] = self.brightness;
        out[13] = 0xFF;
        out[14..16].copy_from_slice(&self.tz_offset_min.to_le_bytes());
        out[16..24].copy_from_slice(&self.last_seen_unix.to_le_bytes());
        ENCODED_LEN
    }

    fn decode(payload: &[u8]) -> Option<Self> {
        if payload.len() != ENCODED_LEN {
            return None;
        }
        let mut presets = [Preset::new(25, 5); 3];
        for (i, preset) in presets.iter_mut().enumerate() {
            let base = i * 4;
            preset.work_min = u16::from_le_bytes([payload[base], payload[base + 1]]);
            preset.rest_min = u16::from_le_bytes([payload[base + 2], payload[base + 3]]);
        }
        let brightness = payload[12];
        let tz_offset_min = i16::from_le_bytes([payload[14], payload[15]]);
        let last_seen_unix = i64::from_le_bytes([
            payload[16],
            payload[17],
            payload[18],
            payload[19],
            payload[20],
            payload[21],
            payload[22],
            payload[23],
        ]);
        Some(Self {
            presets,
            brightness,
            tz_offset_min,
            last_seen_unix,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let s = Settings {
            presets: [Preset::new(25, 5), Preset::new(50, 10), Preset::new(90, 20)],
            brightness: 128,
            tz_offset_min: 180,
            last_seen_unix: 1_761_500_000,
        };
        let mut buf = [0u8; 24];
        let len = s.encode(&mut buf);
        assert_eq!(len, 24);
        let back = Settings::decode(&buf).unwrap();
        assert_eq!(s, back);
    }
}
