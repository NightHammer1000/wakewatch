//! WakeWatch — a tray indicator for Windows display and standby wakelocks.
//!
//! Exposed as a library so the decoder can be unit-tested and exercised by
//! `examples/dump.rs` against live system state.

pub mod autostart;
pub mod devpath;
pub mod icons;
pub mod model;
pub mod power;
pub mod single;
pub mod ticker;
pub mod tray;

use devpath::DevicePathMap;
use model::Snapshot;

/// Queries the system and folds the result into a displayable snapshot.
/// Any failure becomes `LockLevel::Unknown` — never a false "all clear".
pub fn poll(paths: &mut DevicePathMap) -> Snapshot {
    match power::snapshot() {
        Ok(requests) => model::build(&requests, paths),
        Err(e) => Snapshot::failed(&e),
    }
}
