//! Low-level slot helpers shared by [`Settings`] and [`Stats`].
//!
//! Each slot occupies one flash sector and starts with a fixed 20-byte header:
//!
//! ```text
//!   0..4    magic (4 bytes, identifies the kind of record)
//!   4       schema version (u8)
//!   5..6    payload length (little-endian u16)
//!   6..8    reserved (0xFFFF)
//!   8..16   monotonic sequence number (little-endian u64)
//!   16..20  CRC32 of header[0..16] and the payload (little-endian u32)
//!   20..    payload, padded with 0xFF to the next 4-byte word
//! ```
//!
//! [`Settings`]: crate::storage::settings::Settings
//! [`Stats`]: crate::storage::stats::Stats

use embedded_storage::nor_flash::{NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash};

/// Flash sector size on the ESP32 (and the slot granularity we use).
pub const SLOT_SIZE: u32 = 0x1000;

pub const SETTINGS_OFFSET_A: u32 = 0x9000;
pub const SETTINGS_OFFSET_B: u32 = 0xA000;
pub const STATS_OFFSET_A: u32 = 0xB000;
pub const STATS_OFFSET_B: u32 = 0xC000;

pub const HEADER_LEN: usize = 20;
pub const MAX_PAYLOAD: usize = 1024;

#[derive(Debug)]
pub enum StorageError {
    /// Slot was blank (entirely 0xFF) — never written yet.
    Empty,
    /// Magic prefix did not match what we expected.
    BadMagic,
    /// Schema version is newer than we know how to read.
    UnsupportedVersion(u8),
    /// CRC mismatch — the slot is corrupted.
    CrcMismatch,
    /// Payload is longer than the slot can hold.
    PayloadTooLong,
    /// Underlying NOR-flash driver returned an error.
    Flash(NorFlashErrorKind),
}

impl<E: NorFlashError> From<E> for StorageError {
    fn from(value: E) -> Self {
        StorageError::Flash(value.kind())
    }
}

/// Description of a record schema. Implemented by `Settings` and `Stats`.
pub trait Record: Sized {
    const MAGIC: [u8; 4];
    const VERSION: u8;
    /// Maximum encoded payload length (used to size scratch buffers and to
    /// reject decode attempts whose `payload_len` is bigger than the schema
    /// could possibly produce).
    const PAYLOAD_MAX: usize;

    /// Encode self into `out` and return the number of bytes written.
    fn encode(&self, out: &mut [u8]) -> usize;
    fn decode(payload: &[u8]) -> Option<Self>;
}

pub struct LoadedRecord<R> {
    pub record: R,
    pub seq: u64,
    /// Offset of the slot the record was read from.
    pub from_offset: u32,
}

/// Try to read a record from `offset`. Returns [`StorageError::Empty`] for a
/// pristine sector so the caller can distinguish "no prior data" from
/// "corrupted".
pub fn read_slot<F: ReadNorFlash, R: Record>(
    flash: &mut F,
    offset: u32,
) -> Result<(R, u64), StorageError> {
    let mut header = [0u8; HEADER_LEN];
    flash.read(offset, &mut header)?;

    // Blank sectors are filled with 0xFF after an erase — treat that as the
    // "no prior data" case.
    if header.iter().all(|&b| b == 0xFF) {
        return Err(StorageError::Empty);
    }

    if header[0..4] != R::MAGIC {
        return Err(StorageError::BadMagic);
    }

    let version = header[4];
    if version != R::VERSION {
        return Err(StorageError::UnsupportedVersion(version));
    }

    let payload_len = u16::from_le_bytes([header[5], header[6]]) as usize;
    if payload_len > R::PAYLOAD_MAX || payload_len + HEADER_LEN > SLOT_SIZE as usize {
        return Err(StorageError::PayloadTooLong);
    }
    let seq = u64::from_le_bytes([
        header[8], header[9], header[10], header[11], header[12], header[13], header[14],
        header[15],
    ]);
    let expected_crc =
        u32::from_le_bytes([header[16], header[17], header[18], header[19]]);

    let mut payload = [0u8; MAX_PAYLOAD];
    let payload = &mut payload[..payload_len];
    flash.read(offset + HEADER_LEN as u32, payload)?;

    let actual_crc = crc32(&header[..16], payload);
    if actual_crc != expected_crc {
        return Err(StorageError::CrcMismatch);
    }

    let record = R::decode(payload).ok_or(StorageError::CrcMismatch)?;
    Ok((record, seq))
}

/// Load the freshest of two slots (A and B). Returns the record, its
/// sequence and the offset of the *other* slot (i.e. where the next write
/// should go).
pub fn load_latest<F: ReadNorFlash, R: Record>(
    flash: &mut F,
    offset_a: u32,
    offset_b: u32,
) -> Option<LoadedRecord<R>> {
    let a = read_slot::<F, R>(flash, offset_a).ok();
    let b = read_slot::<F, R>(flash, offset_b).ok();
    match (a, b) {
        (Some((rec_a, seq_a)), Some((rec_b, seq_b))) => {
            if seq_a >= seq_b {
                Some(LoadedRecord {
                    record: rec_a,
                    seq: seq_a,
                    from_offset: offset_a,
                })
            } else {
                Some(LoadedRecord {
                    record: rec_b,
                    seq: seq_b,
                    from_offset: offset_b,
                })
            }
        }
        (Some((rec, seq)), None) => Some(LoadedRecord {
            record: rec,
            seq,
            from_offset: offset_a,
        }),
        (None, Some((rec, seq))) => Some(LoadedRecord {
            record: rec,
            seq,
            from_offset: offset_b,
        }),
        (None, None) => None,
    }
}

/// Write `record` to whichever of the two slots is *not* `last_used_offset`.
/// Returns the offset the new copy was written to.
pub fn write_alternate<F: NorFlash, R: Record>(
    flash: &mut F,
    offset_a: u32,
    offset_b: u32,
    last_used_offset: Option<u32>,
    record: &R,
    seq: u64,
) -> Result<u32, StorageError> {
    let target = match last_used_offset {
        Some(o) if o == offset_a => offset_b,
        Some(o) if o == offset_b => offset_a,
        // First write ever: just start with A.
        _ => offset_a,
    };
    write_slot(flash, target, record, seq)?;
    Ok(target)
}

/// Erase and overwrite a single slot.
fn write_slot<F: NorFlash, R: Record>(
    flash: &mut F,
    offset: u32,
    record: &R,
    seq: u64,
) -> Result<(), StorageError> {
    let mut buf = [0xFFu8; MAX_PAYLOAD + HEADER_LEN];
    let payload_len = record.encode(&mut buf[HEADER_LEN..]);
    if payload_len > R::PAYLOAD_MAX {
        return Err(StorageError::PayloadTooLong);
    }

    // Header.
    buf[0..4].copy_from_slice(&R::MAGIC);
    buf[4] = R::VERSION;
    let plen = payload_len as u16;
    buf[5..7].copy_from_slice(&plen.to_le_bytes());
    buf[7] = 0xFF;
    buf[8..16].copy_from_slice(&seq.to_le_bytes());

    let crc = crc32(&buf[..16], &buf[HEADER_LEN..HEADER_LEN + payload_len]);
    buf[16..20].copy_from_slice(&crc.to_le_bytes());

    let total_unaligned = HEADER_LEN + payload_len;
    // Round up to a multiple of WRITE_SIZE (typically 4 bytes on ESP32).
    let write_size = F::WRITE_SIZE.max(1);
    let total = total_unaligned.div_ceil(write_size) * write_size;

    flash.erase(offset, offset + SLOT_SIZE)?;
    flash.write(offset, &buf[..total])?;
    Ok(())
}

/// CRC-32 IEEE 802.3 (the "Ethernet" / `zlib.crc32` flavour). Tableless
/// implementation — fast enough for the few hundred bytes we hash per save.
pub fn crc32(header_prefix: &[u8], payload: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for chunk in [header_prefix, payload] {
        for &byte in chunk {
            crc ^= byte as u32;
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vector() {
        // "123456789" → 0xCBF43926 (well-known CRC-32 test vector).
        let crc = crc32(b"", b"123456789");
        assert_eq!(crc, 0xCBF4_3926);
    }
}
