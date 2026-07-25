//! Walks the same startup sequence as the tray app, but on a console so each
//! step's outcome is visible. Diagnostic aid for "it exited immediately".
//!
//!     cargo run --example startup_check

use std::path::PathBuf;

use wakewatch::devpath::DevicePathMap;
use wakewatch::icons::IconSet;
use wakewatch::tray::Tray;
use wakewatch::{poll, single};

fn main() {
    println!("1. single-instance guard…");
    match single::acquire(single::MUTEX_NAME) {
        Some(_g) => println!("   OK — this is the first instance"),
        None => {
            println!("   BLOCKED — another instance holds {}", single::MUTEX_NAME);
            println!("   (the tray app would exit silently here)");
            return;
        }
    }

    println!("2. icon set…");
    let icons = match IconSet::build() {
        Ok(i) => {
            println!("   OK");
            i
        }
        Err(e) => {
            println!("   FAILED: {e}");
            return;
        }
    };

    println!("3. poll…");
    let mut paths = DevicePathMap::new();
    let snap = poll(&mut paths);
    println!(
        "   OK — level {:?}, tooltip {:?}",
        snap.level,
        snap.tooltip()
    );

    println!("4. tray icon…");
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("wakewatch.exe"));
    match Tray::new(icons, exe) {
        Ok(mut tray) => {
            println!("   OK — tray created");
            tray.apply(snap);
            println!("   OK — snapshot applied");
        }
        Err(e) => println!("   FAILED: {e}"),
    }

    println!("\nall startup steps completed");
}
