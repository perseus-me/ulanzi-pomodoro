//! Rolling 32-day Pomodoro statistics.
//!
//! Storage is a circular buffer of `[DayStats; 32]` indexed by `head`, which
//! always points at the most-recently-touched slot. When a new day starts we
//! advance `head` (mod 32) and overwrite whatever was previously in that
//! slot — which naturally evicts the oldest day, giving us ~1 month of
//! history in 256 bytes.

use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};

use crate::storage::nvs::{
    self, LoadedRecord, Record, STATS_OFFSET_A, STATS_OFFSET_B, StorageError,
};

pub const MAX_STATS_DAYS: usize = 32;
const ENCODED_LEN: usize = MAX_STATS_DAYS * 8 + 4; // 256 + head + padding
const UNDATED_DATE: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DayStats {
    /// Calendar date as `YYYY * 10000 + MM * 100 + DD`. Zero means the slot
    /// has never been touched.
    pub date_yyyymmdd: u32,
    pub completed_pomodoros: u16,
    pub focus_minutes: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stats {
    days: [DayStats; MAX_STATS_DAYS],
    /// Index of the most recent entry. Initially `0`; the first call to
    /// [`Stats::record_completed_work`] will populate `days[0]` directly.
    head: u8,
    /// `true` until the first successful record — disambiguates "we just
    /// booted and `days[head]` is uninitialised" from "today is recorded at
    /// `head`".
    empty: bool,
}

impl Stats {
    pub const fn empty() -> Self {
        Self {
            days: [DayStats {
                date_yyyymmdd: 0,
                completed_pomodoros: 0,
                focus_minutes: 0,
            }; MAX_STATS_DAYS],
            head: 0,
            empty: true,
        }
    }

    pub fn load<F: ReadNorFlash>(flash: &mut F) -> (Self, StatsStore) {
        match nvs::load_latest::<F, Stats>(flash, STATS_OFFSET_A, STATS_OFFSET_B) {
            Some(LoadedRecord {
                record,
                seq,
                from_offset,
            }) => (
                record,
                StatsStore {
                    seq,
                    last_offset: Some(from_offset),
                },
            ),
            None => (
                Self::empty(),
                StatsStore {
                    seq: 0,
                    last_offset: None,
                },
            ),
        }
    }

    /// Credit a completed Pomodoro to the given calendar date. Returns `true`
    /// if anything changed (i.e. the caller should persist).
    pub fn record_completed_work(&mut self, date_yyyymmdd: u32, focus_minutes: u16) -> bool {
        if date_yyyymmdd == 0 {
            return false;
        }

        // First record ever → take slot 0 directly.
        if self.empty {
            self.head = 0;
            self.days[0] = DayStats {
                date_yyyymmdd,
                completed_pomodoros: 1,
                focus_minutes,
            };
            self.empty = false;
            return true;
        }

        let head = self.head as usize;
        if self.days[head].date_yyyymmdd == date_yyyymmdd {
            // Same day → just bump the counts.
            self.days[head].completed_pomodoros =
                self.days[head].completed_pomodoros.saturating_add(1);
            self.days[head].focus_minutes =
                self.days[head].focus_minutes.saturating_add(focus_minutes);
        } else {
            // New day → advance head, overwriting the oldest entry.
            self.head = ((self.head + 1) % MAX_STATS_DAYS as u8) as u8;
            let new_head = self.head as usize;
            self.days[new_head] = DayStats {
                date_yyyymmdd,
                completed_pomodoros: 1,
                focus_minutes,
            };
        }
        true
    }

    /// Credit completed work to the best day we can identify right now.
    ///
    /// With a wall-clock date this is the real calendar day. Without a clock
    /// anchor, keep accumulating in the current head slot so offline devices
    /// still show and persist progress instead of dropping completions.
    pub fn record_completed_work_best_effort(
        &mut self,
        date_yyyymmdd: Option<u32>,
        focus_minutes: u16,
    ) -> bool {
        let date = match date_yyyymmdd {
            Some(date) => date,
            None if self.empty => UNDATED_DATE,
            None => self.days[self.head as usize]
                .date_yyyymmdd
                .max(UNDATED_DATE),
        };
        self.record_completed_work(date, focus_minutes)
    }

    pub fn current(&self) -> DayStats {
        if self.empty {
            DayStats::default()
        } else {
            self.days[self.head as usize]
        }
    }

    /// Today's count, or 0 if we haven't recorded anything for `today`.
    pub fn today(&self, today: u32) -> DayStats {
        if self.empty || today == 0 {
            return DayStats::default();
        }
        let entry = self.days[self.head as usize];
        if entry.date_yyyymmdd == today {
            entry
        } else {
            DayStats {
                date_yyyymmdd: today,
                completed_pomodoros: 0,
                focus_minutes: 0,
            }
        }
    }

    /// The last `n` entries in calendar order (oldest first), padding with
    /// zero-day entries if the ring is not yet full.
    pub fn last_n(&self, n: usize) -> [DayStats; MAX_STATS_DAYS] {
        let mut out = [DayStats::default(); MAX_STATS_DAYS];
        if self.empty {
            return out;
        }
        let take = n.min(MAX_STATS_DAYS);
        for i in 0..take {
            let from_end = take - 1 - i;
            // head − from_end (mod 32)
            let idx = (self.head as i32 - from_end as i32).rem_euclid(MAX_STATS_DAYS as i32);
            out[i] = self.days[idx as usize];
        }
        out
    }

    pub fn head(&self) -> u8 {
        self.head
    }

    pub fn is_empty(&self) -> bool {
        self.empty
    }
}

pub struct StatsStore {
    seq: u64,
    last_offset: Option<u32>,
}

impl StatsStore {
    pub fn save<F: NorFlash>(&mut self, flash: &mut F, stats: &Stats) -> Result<(), StorageError> {
        let next_seq = self.seq.wrapping_add(1);
        let written = nvs::write_alternate::<F, Stats>(
            flash,
            STATS_OFFSET_A,
            STATS_OFFSET_B,
            self.last_offset,
            stats,
            next_seq,
        )?;
        self.seq = next_seq;
        self.last_offset = Some(written);
        Ok(())
    }
}

impl Record for Stats {
    const MAGIC: [u8; 4] = *b"POST";
    const VERSION: u8 = 1;
    const PAYLOAD_MAX: usize = ENCODED_LEN;

    fn encode(&self, out: &mut [u8]) -> usize {
        debug_assert!(out.len() >= ENCODED_LEN);
        for (i, day) in self.days.iter().enumerate() {
            let base = i * 8;
            out[base..base + 4].copy_from_slice(&day.date_yyyymmdd.to_le_bytes());
            out[base + 4..base + 6].copy_from_slice(&day.completed_pomodoros.to_le_bytes());
            out[base + 6..base + 8].copy_from_slice(&day.focus_minutes.to_le_bytes());
        }
        out[256] = self.head;
        // Pack the "empty" flag into byte 257 so we don't lose it across reboots.
        out[257] = if self.empty { 1 } else { 0 };
        out[258] = 0xFF;
        out[259] = 0xFF;
        ENCODED_LEN
    }

    fn decode(payload: &[u8]) -> Option<Self> {
        if payload.len() != ENCODED_LEN {
            return None;
        }
        let mut days = [DayStats::default(); MAX_STATS_DAYS];
        for (i, day) in days.iter_mut().enumerate() {
            let base = i * 8;
            day.date_yyyymmdd = u32::from_le_bytes([
                payload[base],
                payload[base + 1],
                payload[base + 2],
                payload[base + 3],
            ]);
            day.completed_pomodoros =
                u16::from_le_bytes([payload[base + 4], payload[base + 5]]);
            day.focus_minutes = u16::from_le_bytes([payload[base + 6], payload[base + 7]]);
        }
        let head = payload[256];
        if head as usize >= MAX_STATS_DAYS {
            return None;
        }
        let empty = payload[257] != 0;
        Some(Self { days, head, empty })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_advances_on_new_day() {
        let mut s = Stats::empty();
        assert!(s.record_completed_work(20260510, 25));
        assert!(s.record_completed_work(20260510, 25));
        assert!(s.record_completed_work(20260511, 50));

        assert_eq!(s.head, 1);
        assert_eq!(s.days[0].date_yyyymmdd, 20260510);
        assert_eq!(s.days[0].completed_pomodoros, 2);
        assert_eq!(s.days[0].focus_minutes, 50);
        assert_eq!(s.days[1].date_yyyymmdd, 20260511);
        assert_eq!(s.days[1].completed_pomodoros, 1);
    }

    #[test]
    fn last_n_pads_when_ring_is_short() {
        let mut s = Stats::empty();
        s.record_completed_work(20260510, 25);
        s.record_completed_work(20260511, 25);
        let week = s.last_n(7);
        // 5 leading zero days, then 510, then 511.
        assert_eq!(week[5].date_yyyymmdd, 20260510);
        assert_eq!(week[6].date_yyyymmdd, 20260511);
        for i in 0..5 {
            assert_eq!(week[i].date_yyyymmdd, 0);
        }
    }

    #[test]
    fn encode_decode_roundtrip() {
        let mut s = Stats::empty();
        s.record_completed_work(20260510, 25);
        s.record_completed_work(20260511, 50);
        let mut buf = [0u8; ENCODED_LEN];
        let n = s.encode(&mut buf);
        assert_eq!(n, ENCODED_LEN);
        let back = Stats::decode(&buf).unwrap();
        assert_eq!(s, back);
    }
}
