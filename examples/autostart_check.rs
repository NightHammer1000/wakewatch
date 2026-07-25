//! Exercises the real autostart code path (the same one the tray menu uses)
//! and leaves the task enabled.
//!
//!     cargo run --example autostart_check
//!
//! Must be run elevated.

use std::path::PathBuf;
use wakewatch::autostart;

fn main() {
    let exe = PathBuf::from(r"C:\Tools\WakeWatch\wakewatch.exe");
    println!("target: {}", exe.display());
    println!("initially enabled: {}", autostart::is_enabled());

    print!("enable… ");
    match autostart::enable(&exe) {
        Ok(()) => println!("OK"),
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(1);
        }
    }
    println!("  is_enabled now: {}", autostart::is_enabled());

    print!("disable… ");
    match autostart::disable() {
        Ok(()) => println!("OK"),
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(2);
        }
    }
    println!("  is_enabled now: {}", autostart::is_enabled());

    print!("re-enable (leaving it on)… ");
    match autostart::enable(&exe) {
        Ok(()) => println!("OK"),
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(3);
        }
    }
    println!("  final is_enabled: {}", autostart::is_enabled());
}
