//! Enumeration of system-wide power requests via the undocumented
//! `NtPowerInformation(GetPowerRequestList)` information class.
//!
//! This is the same call `powercfg /requests` makes internally. It requires
//! administrator rights; a limited token gets STATUS_ACCESS_DENIED.
//!
//! The returned layout is undocumented and varies by Windows version, so every
//! read here goes through a bounds-checked accessor that returns `None` rather
//! than reading past the buffer. A failed decode must surface as "unknown",
//! never as a plausible-looking result.

use std::ffi::c_void;

#[cfg(not(target_pointer_width = "64"))]
compile_error!("wakewatch assumes the x64 POWER_REQUEST layout (8-byte SIZE_T)");

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtPowerInformation(
        level: i32,
        in_buf: *const c_void,
        in_len: u32,
        out_buf: *mut c_void,
        out_len: u32,
    ) -> i32;

    fn RtlGetNtVersionNumbers(major: *mut u32, minor: *mut u32, build: *mut u32);
}

const GET_POWER_REQUEST_LIST: i32 = 45;
const STATUS_BUFFER_TOO_SMALL: i32 = 0xC000_0023u32 as i32;
const STATUS_ACCESS_DENIED: i32 = 0xC000_0022u32 as i32;

const MAX_BUFFER: usize = 1024 * 1024;
/// Sanity ceiling on the request count; the live system shows ~50.
const MAX_REQUESTS: usize = 100_000;
/// Sanity ceiling on a single UTF-16 string, in code units.
const MAX_STR_UNITS: usize = 4096;

/// Index into `PowerRequestCount`, matching POWER_REQUEST_TYPE_INTERNAL.
pub const MODE_DISPLAY: usize = 0;
pub const MODE_SYSTEM: usize = 1;
pub const MODE_AWAYMODE: usize = 2;
pub const MODE_EXECUTION: usize = 3;
pub const MODE_PERFBOOST: usize = 4;
pub const MODE_ACTIVELOCKSCREEN: usize = 5;

pub const MODE_NAMES: [&str; 6] = [
    "DISPLAY",
    "SYSTEM",
    "AWAYMODE",
    "EXECUTION",
    "PERFBOOST",
    "ACTIVELOCKSCREEN",
];

/// REQUESTER_TYPE
pub const CALLER_KERNEL: u32 = 0;
pub const CALLER_PROCESS: u32 = 1;
pub const CALLER_SERVICE: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerError {
    /// Not running elevated.
    AccessDenied,
    /// The syscall failed for some other reason.
    Nt(i32),
    /// The buffer grew past the cap without the call succeeding.
    TooLarge,
    /// The buffer came back but did not match the expected layout.
    Malformed,
}

impl PowerError {
    pub fn describe(&self) -> String {
        match self {
            PowerError::AccessDenied => "Administrator rights required".into(),
            PowerError::Nt(st) => format!("Query failed (NTSTATUS 0x{:08X})", *st as u32),
            PowerError::TooLarge => "Request list too large to read".into(),
            PowerError::Malformed => "Unrecognized power request layout".into(),
        }
    }
}

/// One decoded power request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRequest {
    /// Active counter per mode, indexed by `MODE_*`.
    pub counts: [u32; 6],
    pub caller_type: u32,
    /// Process image path (NT device form) or driver device description.
    pub name: String,
    pub pid: Option<u32>,
    pub reason: Option<String>,
}

impl RawRequest {
    pub fn holds(&self, mode: usize) -> bool {
        self.counts.get(mode).copied().unwrap_or(0) > 0
    }
}

// ---------------------------------------------------------------------------
// Bounds-checked readers. Every one returns None on any out-of-range access.
// ---------------------------------------------------------------------------

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    let end = off.checked_add(4)?;
    let s = b.get(off..end)?;
    Some(u32::from_le_bytes(s.try_into().ok()?))
}

fn usize_at(b: &[u8], off: usize) -> Option<u64> {
    let end = off.checked_add(8)?;
    let s = b.get(off..end)?;
    Some(u64::from_le_bytes(s.try_into().ok()?))
}

/// Reads a NUL-terminated UTF-16 string. Returns None if it runs off the end
/// of the buffer without a terminator, or exceeds `MAX_STR_UNITS`.
fn wstr_at(b: &[u8], off: usize) -> Option<String> {
    if off >= b.len() {
        return None;
    }
    let mut units: Vec<u16> = Vec::new();
    let mut i = off;
    while i + 1 < b.len() {
        let u = u16::from_le_bytes([b[i], b[i + 1]]);
        if u == 0 {
            return Some(String::from_utf16_lossy(&units));
        }
        if units.len() >= MAX_STR_UNITS {
            return None;
        }
        units.push(u);
        i += 2;
    }
    None
}

// ---------------------------------------------------------------------------
// Layout selection
// ---------------------------------------------------------------------------

fn nt_build() -> u32 {
    let (mut major, mut minor, mut build) = (0u32, 0u32, 0u32);
    unsafe { RtlGetNtVersionNumbers(&mut major, &mut minor, &mut build) };
    // The high nibble is a flag area, not part of the build number.
    build & 0x0FFF_FFFF
}

/// Number of entries in `PowerRequestCount`, per POWER_REQUEST_SUPPORTED_TYPES_Vn.
/// Deliberately keyed off the OS build: `SupportedRequestMask` is NOT reliable
/// for this (kernel requesters were observed with mask 0x12, not 0x3F).
fn mode_count(build: u32) -> usize {
    match build {
        b if b >= 14393 => 6, // V4, Win10 RS1+
        b if b >= 9600 => 5,  // V3, Win8.1 / Win10 TH1-TH2
        b if b >= 9200 => 9,  // V2, Win8
        _ => 3,               // V1, Win7
    }
}

/// Offset of DIAGNOSTIC_BUFFER within POWER_REQUEST: the mask plus the counter
/// array, rounded up to SIZE_T alignment.
fn diag_offset(modes: usize) -> usize {
    let raw = 4 + modes * 4;
    (raw + 7) & !7
}

// ---------------------------------------------------------------------------
// Query + decode
// ---------------------------------------------------------------------------

/// Issues the syscall, growing the buffer until it fits.
pub fn query_raw() -> Result<Vec<u8>, PowerError> {
    let mut len = 4096usize;
    loop {
        let mut buf = vec![0u8; len];
        let status = unsafe {
            NtPowerInformation(
                GET_POWER_REQUEST_LIST,
                std::ptr::null(),
                0,
                buf.as_mut_ptr() as *mut c_void,
                len as u32,
            )
        };
        match status {
            0 => return Ok(buf),
            STATUS_BUFFER_TOO_SMALL => {
                len += 4096;
                if len > MAX_BUFFER {
                    return Err(PowerError::TooLarge);
                }
            }
            STATUS_ACCESS_DENIED => return Err(PowerError::AccessDenied),
            other => return Err(PowerError::Nt(other)),
        }
    }
}

/// Decodes a raw POWER_REQUEST_LIST buffer. Any structural surprise yields
/// `Err(Malformed)` so the caller can show "unknown" instead of a wrong colour.
pub fn decode(buf: &[u8]) -> Result<Vec<RawRequest>, PowerError> {
    decode_with_build(buf, nt_build())
}

pub fn decode_with_build(buf: &[u8], build: u32) -> Result<Vec<RawRequest>, PowerError> {
    let modes = mode_count(build);
    let db_off = diag_offset(modes);

    let count = usize_at(buf, 0).ok_or(PowerError::Malformed)? as usize;
    if count > MAX_REQUESTS {
        return Err(PowerError::Malformed);
    }

    let mut out = Vec::with_capacity(count.min(1024));
    for i in 0..count {
        let slot = 8usize
            .checked_add(i.checked_mul(8).ok_or(PowerError::Malformed)?)
            .ok_or(PowerError::Malformed)?;
        let req = usize_at(buf, slot).ok_or(PowerError::Malformed)? as usize;
        out.push(decode_one(buf, req, modes, db_off).ok_or(PowerError::Malformed)?);
    }
    Ok(out)
}

fn decode_one(buf: &[u8], req: usize, modes: usize, db_off: usize) -> Option<RawRequest> {
    // Validate the whole counter array is present, even for modes we ignore.
    let last = req.checked_add(4 + modes.checked_sub(1)?.checked_mul(4)?)?;
    u32_at(buf, last)?;

    let mut counts = [0u32; 6];
    for (m, slot) in counts.iter_mut().enumerate().take(modes.min(6)) {
        *slot = u32_at(buf, req.checked_add(4 + m * 4)?)?;
    }

    let db = req.checked_add(db_off)?;

    // DIAGNOSTIC_BUFFER.Size — a cheap structural plausibility check.
    let size = usize_at(buf, db)? as usize;
    if size == 0 || size > buf.len() {
        return None;
    }

    let caller_type = u32_at(buf, db.checked_add(8)?)?;
    if caller_type > CALLER_SERVICE {
        return None;
    }

    // Union at +16. Both arms start with a string offset relative to `db`.
    let name_off = usize_at(buf, db.checked_add(16)?)? as usize;
    let name = if name_off != 0 {
        wstr_at(buf, db.checked_add(name_off)?)?
    } else {
        String::new()
    };

    let pid = if caller_type == CALLER_KERNEL {
        None
    } else {
        Some(u32_at(buf, db.checked_add(24)?)?)
    };

    let reason = read_reason(buf, db);

    Some(RawRequest {
        counts,
        caller_type,
        name,
        pid,
        reason,
    })
}

/// COUNTED_REASON_CONTEXT_RELATIVE at DIAGNOSTIC_BUFFER + ReasonOffset.
/// Only the simple-string form is read; the resource-file form would need
/// LoadLibraryEx + FormatMessage and adds nothing for our purposes.
/// A missing or unreadable reason is not an error.
fn read_reason(buf: &[u8], db: usize) -> Option<String> {
    let reason_off = usize_at(buf, db.checked_add(32)?)? as usize;
    if reason_off == 0 {
        return None;
    }
    let ctx = db.checked_add(reason_off)?;
    let flags = u32_at(buf, ctx)?;
    const POWER_REQUEST_CONTEXT_SIMPLE_STRING: u32 = 0x1;
    if flags & POWER_REQUEST_CONTEXT_SIMPLE_STRING == 0 {
        return None;
    }
    let str_off = usize_at(buf, ctx.checked_add(8)?)? as usize;
    if str_off == 0 {
        return None;
    }
    let s = wstr_at(buf, ctx.checked_add(str_off)?)?;
    if s.is_empty() { None } else { Some(s) }
}

/// Convenience: query and decode in one step.
pub fn snapshot() -> Result<Vec<RawRequest>, PowerError> {
    let buf = query_raw()?;
    decode(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN11: u32 = 26200;

    /// Builds a synthetic V4 buffer with one process request holding DISPLAY.
    fn synth() -> Vec<u8> {
        let mut b = vec![0u8; 512];
        // POWER_REQUEST_LIST
        b[0..8].copy_from_slice(&1u64.to_le_bytes()); // Count
        b[8..16].copy_from_slice(&64u64.to_le_bytes()); // Offsets[0]

        let req = 64usize;
        b[req..req + 4].copy_from_slice(&0x3Fu32.to_le_bytes()); // mask
        b[req + 4..req + 8].copy_from_slice(&1u32.to_le_bytes()); // DISPLAY = 1

        let db = req + 32;
        b[db..db + 8].copy_from_slice(&120u64.to_le_bytes()); // Size
        b[db + 8..db + 12].copy_from_slice(&CALLER_PROCESS.to_le_bytes());
        b[db + 16..db + 24].copy_from_slice(&64u64.to_le_bytes()); // name offset
        b[db + 24..db + 28].copy_from_slice(&1234u32.to_le_bytes()); // pid

        // UTF-16 "a.exe" at db + 64
        let name = db + 64;
        for (i, u) in "a.exe".encode_utf16().enumerate() {
            b[name + i * 2..name + i * 2 + 2].copy_from_slice(&u.to_le_bytes());
        }
        b
    }

    #[test]
    fn decodes_a_well_formed_request() {
        let reqs = decode_with_build(&synth(), WIN11).expect("should decode");
        assert_eq!(reqs.len(), 1);
        assert!(reqs[0].holds(MODE_DISPLAY));
        assert!(!reqs[0].holds(MODE_SYSTEM));
        assert_eq!(reqs[0].name, "a.exe");
        assert_eq!(reqs[0].pid, Some(1234));
    }

    #[test]
    fn diag_offsets_match_known_layouts() {
        assert_eq!(diag_offset(6), 32); // V4, confirmed against live data
        assert_eq!(diag_offset(5), 24);
        assert_eq!(diag_offset(9), 40);
        assert_eq!(diag_offset(3), 16);
    }

    #[test]
    fn mode_count_tracks_build() {
        assert_eq!(mode_count(26200), 6);
        assert_eq!(mode_count(14393), 6);
        assert_eq!(mode_count(9600), 5);
        assert_eq!(mode_count(9200), 9);
        assert_eq!(mode_count(7601), 3);
    }

    #[test]
    fn truncated_buffer_is_malformed_not_panic() {
        let full = synth();
        for cut in [0, 4, 8, 15, 40, 64, 70, 100, 120] {
            let r = decode_with_build(&full[..cut.min(full.len())], WIN11);
            assert!(r.is_err(), "cut={cut} should not decode");
        }
    }

    #[test]
    fn absurd_count_is_rejected() {
        let mut b = synth();
        b[0..8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(decode_with_build(&b, WIN11), Err(PowerError::Malformed));
    }

    #[test]
    fn count_beyond_buffer_is_rejected() {
        let mut b = synth();
        b[0..8].copy_from_slice(&5000u64.to_le_bytes());
        assert_eq!(decode_with_build(&b, WIN11), Err(PowerError::Malformed));
    }

    #[test]
    fn offset_past_end_is_rejected() {
        let mut b = synth();
        b[8..16].copy_from_slice(&100_000u64.to_le_bytes());
        assert_eq!(decode_with_build(&b, WIN11), Err(PowerError::Malformed));
    }

    #[test]
    fn bad_caller_type_is_rejected() {
        let mut b = synth();
        let db = 64 + 32;
        b[db + 8..db + 12].copy_from_slice(&7u32.to_le_bytes());
        assert_eq!(decode_with_build(&b, WIN11), Err(PowerError::Malformed));
    }

    #[test]
    fn zero_and_oversized_diag_size_are_rejected() {
        let db = 64 + 32;
        for size in [0u64, 10_000_000u64] {
            let mut b = synth();
            b[db..db + 8].copy_from_slice(&size.to_le_bytes());
            assert_eq!(decode_with_build(&b, WIN11), Err(PowerError::Malformed));
        }
    }

    #[test]
    fn unterminated_string_is_rejected() {
        let mut b = synth();
        // Fill from the name offset to the end with non-zero bytes.
        let name = 64 + 32 + 64;
        for byte in b[name..].iter_mut() {
            *byte = 0x41;
        }
        assert_eq!(decode_with_build(&b, WIN11), Err(PowerError::Malformed));
    }

    #[test]
    fn zero_name_offset_yields_empty_name() {
        let mut b = synth();
        let db = 64 + 32;
        b[db + 16..db + 24].copy_from_slice(&0u64.to_le_bytes());
        let reqs = decode_with_build(&b, WIN11).expect("should decode");
        assert_eq!(reqs[0].name, "");
    }

    #[test]
    fn readers_never_panic_on_arbitrary_input() {
        let b = synth();
        for off in 0..b.len() + 32 {
            let _ = u32_at(&b, off);
            let _ = usize_at(&b, off);
            let _ = wstr_at(&b, off);
        }
        let _ = u32_at(&b, usize::MAX);
        let _ = usize_at(&b, usize::MAX);
        let _ = wstr_at(&b, usize::MAX);
    }

    #[test]
    fn empty_buffer_is_malformed() {
        assert_eq!(decode_with_build(&[], WIN11), Err(PowerError::Malformed));
    }
}
