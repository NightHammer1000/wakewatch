//! Console dump of the decoded power request list, for cross-checking against
//! `powercfg /requests`.
//!
//!     cargo run --example dump
//!
//! Must be run elevated, exactly like powercfg.

use wakewatch::devpath::DevicePathMap;
use wakewatch::model::LockLevel;
use wakewatch::power;

fn main() {
    let raw = match power::query_raw() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("query failed: {}", e.describe());
            std::process::exit(1);
        }
    };
    println!("buffer: {} bytes", raw.len());

    let requests = match power::decode(&raw) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("decode failed: {}", e.describe());
            std::process::exit(2);
        }
    };
    println!("requests: {}\n", requests.len());

    let mut paths = DevicePathMap::new();
    let snap = wakewatch::model::build(&requests, &mut paths);

    println!("LEVEL: {:?}", snap.level);
    println!("TOOLTIP: {}\n", snap.tooltip());

    for group in &snap.groups {
        println!("{}:", group.name());
        for h in &group.holders {
            let count = if h.count > 1 {
                format!(" x{}", h.count)
            } else {
                String::new()
            };
            let reason = h
                .reason
                .as_deref()
                .map(|r| format!("  [{r}]"))
                .unwrap_or_default();
            println!("  {}{}{}", h.label, count, reason);
            println!("      {}", h.detail);
        }
        println!();
    }

    if snap.level == LockLevel::Unknown {
        eprintln!("NOTE: level is Unknown — the tray would show grey.");
    }
}
