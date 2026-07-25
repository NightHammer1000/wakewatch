#![windows_subsystem = "windows"]

//! Entry point: single-instance guard, tray setup, and the Win32 message loop
//! that drives polling.

use std::path::PathBuf;

use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MB_ICONERROR, MB_OK, MSG, MessageBoxW, PostQuitMessage,
    TranslateMessage,
};
use windows::core::{HSTRING, PCWSTR};

use tray_icon::menu::MenuEvent;
use wakewatch::devpath::DevicePathMap;
use wakewatch::icons::IconSet;
use wakewatch::ticker::Ticker;
use wakewatch::tray::{ID_AUTOSTART, ID_EXIT, ID_REFRESH, Tray};
use wakewatch::{autostart, poll, single};

const POLL_MS: u32 = 1000;

fn main() {
    // Held for the lifetime of the process; a second copy exits silently.
    let _instance = match single::acquire(single::MUTEX_NAME) {
        Some(guard) => guard,
        None => return,
    };

    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("wakewatch.exe"));

    let icons = match IconSet::build() {
        Ok(i) => i,
        Err(e) => return fatal(&format!("Could not create tray icons: {e}")),
    };

    let mut tray = match Tray::new(icons, exe) {
        Ok(t) => t,
        Err(e) => return fatal(&format!("Could not create the tray icon: {e}")),
    };

    let mut paths = DevicePathMap::new();
    tray.apply(poll(&mut paths));

    // A NULL-hwnd timer posts WM_TIMER straight to this thread's queue, so we
    // need no window class of our own. Ticker owns the system-generated ID.
    let ticker = match Ticker::start(POLL_MS) {
        Some(t) => t,
        None => return fatal("Could not start the poll timer."),
    };

    let menu_events = MenuEvent::receiver();
    let mut msg = MSG::default();

    // GetMessageW returns 0 on WM_QUIT and -1 on error; both end the loop.
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
        if ticker.is_tick(&msg) {
            tray.apply(poll(&mut paths));
        }

        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Menu clicks are queued by muda while the message above is dispatched.
        while let Ok(event) = menu_events.try_recv() {
            match event.id.as_ref() {
                ID_REFRESH => tray.apply(poll(&mut paths)),
                ID_AUTOSTART => toggle_autostart(&mut tray),
                ID_EXIT => unsafe { PostQuitMessage(0) },
                _ => {}
            }
        }
    }
}

fn toggle_autostart(tray: &mut Tray) {
    let result = if tray.autostart_on() {
        autostart::disable()
    } else {
        autostart::enable(tray.exe())
    };
    if let Err(e) = result {
        warn(&format!("Could not change the autostart task:\n\n{e}"));
    }
    // Re-read rather than assume, so the checkmark reflects reality.
    tray.refresh_autostart();
}

fn message_box(text: &str, flags: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE) {
    let text = HSTRING::from(text);
    let title = HSTRING::from("WakeWatch");
    unsafe {
        MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(title.as_ptr()), flags);
    }
}

fn fatal(text: &str) {
    message_box(text, MB_OK | MB_ICONERROR);
}

fn warn(text: &str) {
    message_box(text, MB_OK | MB_ICONERROR);
}
