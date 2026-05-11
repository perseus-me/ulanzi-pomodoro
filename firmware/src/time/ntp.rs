//! SNTPv4 client packet encoding & decoding.
//!
//! Only the bits we actually need (client → server query, server → client
//! response with the "transmit timestamp"). Everything else is left as zero.

/// Bytes between 1900-01-01 (NTP epoch) and 1970-01-01 (Unix epoch).
pub const NTP_UNIX_EPOCH_OFFSET: u64 = 2_208_988_800;

pub const NTP_PACKET_LEN: usize = 48;

#[derive(Clone, Copy, Debug)]
pub struct NtpRequest;

impl NtpRequest {
    /// Build the raw 48-byte client request payload.
    ///
    /// First byte:  `LI (2 bits) | VN (3 bits) | Mode (3 bits)`
    /// We send `LI=0`, `VN=4`, `Mode=3` (client) which is `0b00_100_011 = 0x23`.
    /// Everything else stays zero — servers don't need any client timestamps
    /// for a basic SNTP request.
    pub const fn to_bytes() -> [u8; NTP_PACKET_LEN] {
        let mut buf = [0u8; NTP_PACKET_LEN];
        buf[0] = 0x23;
        buf
    }
}

/// Extract the unix timestamp (seconds) from a 48-byte SNTP response.
///
/// Looks at the "Transmit Timestamp" field at byte offset 40..44 (seconds
/// since 1900-01-01) and subtracts the 1900→1970 offset. Returns `None` if
/// the response is too short or the seconds field is zero (which servers use
/// when they cannot provide a valid timestamp).
pub fn parse_ntp_response(buf: &[u8]) -> Option<i64> {
    if buf.len() < NTP_PACKET_LEN {
        return None;
    }
    // Reject responses where the stratum byte (offset 1) is 0 or kissing-of-
    // death codes, which servers send to signal "do not query me right now".
    let stratum = buf[1];
    if stratum == 0 {
        return None;
    }
    let seconds_1900 = u32::from_be_bytes([buf[40], buf[41], buf[42], buf[43]]);
    if seconds_1900 == 0 {
        return None;
    }
    Some(seconds_1900 as i64 - NTP_UNIX_EPOCH_OFFSET as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_is_well_formed() {
        let req = NtpRequest::to_bytes();
        assert_eq!(req.len(), 48);
        assert_eq!(req[0], 0x23);
        // Everything else is zero.
        assert!(req[1..].iter().all(|&b| b == 0));
    }

    #[test]
    fn parses_simple_response() {
        let mut resp = [0u8; NTP_PACKET_LEN];
        resp[1] = 1; // stratum 1 (primary)
        // 2026-05-12 00:14:00 UTC → unix 1_778_285_640 → NTP seconds
        // 1_778_285_640 + 2_208_988_800 = 3_987_274_440
        let ntp_seconds: u32 = 3_987_274_440;
        resp[40..44].copy_from_slice(&ntp_seconds.to_be_bytes());
        let unix = parse_ntp_response(&resp).unwrap();
        assert_eq!(unix, 1_778_285_640);
    }

    #[test]
    fn rejects_kod() {
        let mut resp = [0u8; NTP_PACKET_LEN];
        resp[1] = 0; // KoD / unspecified
        resp[40..44].copy_from_slice(&1u32.to_be_bytes());
        assert!(parse_ntp_response(&resp).is_none());
    }
}
