//! Proves the poll timer actually fires, using the same Ticker + message loop
//! the tray app uses. This is the regression guard for the bug where the tray
//! never auto-refreshed because WM_TIMER's wParam carries a system-generated
//! ID, not the one passed to SetTimer.
//!
//!     cargo run --example timer_check

use std::time::Instant;

use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, TranslateMessage,
};

use wakewatch::devpath::DevicePathMap;
use wakewatch::poll;
use wakewatch::ticker::Ticker;

const INTERVAL_MS: u32 = 1000;
const WANT_TICKS: u32 = 5;

fn main() {
    let ticker = Ticker::start(INTERVAL_MS).expect("timer should start");
    println!(
        "timer started: system-generated id = {} (we asked for 0)",
        ticker.id()
    );
    println!("waiting for {WANT_TICKS} ticks at {INTERVAL_MS} ms…\n");

    let mut paths = DevicePathMap::new();
    let mut ticks = 0u32;
    let mut last = Instant::now();
    let started = Instant::now();
    let mut msg = MSG::default();

    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
        if ticker.is_tick(&msg) {
            ticks += 1;
            let gap = last.elapsed();
            last = Instant::now();

            let t0 = Instant::now();
            let snap = poll(&mut paths);
            let poll_us = t0.elapsed().as_micros();

            println!(
                "tick {ticks}: +{:>4} ms | poll took {poll_us:>5} us | level {:?}",
                gap.as_millis(),
                snap.level
            );

            if ticks >= WANT_TICKS {
                break;
            }
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    if ticks >= WANT_TICKS {
        println!(
            "\nPASS — {ticks} ticks in {:.1} s",
            started.elapsed().as_secs_f32()
        );
    } else {
        println!("\nFAIL — only {ticks} ticks; the timer is not firing");
        std::process::exit(1);
    }
}
