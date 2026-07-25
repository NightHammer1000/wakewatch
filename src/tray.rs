//! Tray icon, tooltip and context menu.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use tray_icon::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::autostart;
use crate::icons::IconSet;
use crate::model::{LockLevel, ModeGroup, Snapshot};

pub const ID_REFRESH: &str = "wakewatch.refresh";
pub const ID_AUTOSTART: &str = "wakewatch.autostart";
pub const ID_EXIT: &str = "wakewatch.exit";

/// Holders listed per mode before collapsing the remainder.
const MAX_MENU_HOLDERS: usize = 15;
/// Menu lines are truncated here; the tooltip carries the summary anyway.
const MAX_MENU_LINE: usize = 90;
/// How stale the cached scheduled-task state may get. Re-reading means
/// spawning schtasks, so this is throttled rather than checked every poll.
const AUTOSTART_TTL: Duration = Duration::from_secs(30);

pub struct Tray {
    icons: IconSet,
    tray: TrayIcon,
    /// What is currently on screen, so we can skip redundant updates.
    shown: Option<Snapshot>,
    autostart_on: bool,
    autostart_checked_at: Instant,
    exe: PathBuf,
}

impl Tray {
    pub fn new(icons: IconSet, exe: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let autostart_on = autostart::is_enabled();
        let initial = Snapshot {
            level: LockLevel::Unknown,
            groups: Vec::new(),
            error: Some("Starting…".into()),
        };
        let tray = TrayIconBuilder::new()
            .with_icon(icons.for_level(LockLevel::Unknown))
            .with_tooltip("WakeWatch")
            .with_menu(Box::new(build_menu(&initial, autostart_on)?))
            .build()?;

        Ok(Tray {
            icons,
            tray,
            shown: None,
            autostart_on,
            autostart_checked_at: Instant::now(),
            exe,
        })
    }

    pub fn exe(&self) -> &PathBuf {
        &self.exe
    }

    pub fn autostart_on(&self) -> bool {
        self.autostart_on
    }

    /// Pushes a snapshot to the tray, doing nothing if it is unchanged.
    pub fn apply(&mut self, snapshot: Snapshot) {
        if self.shown.as_ref() == Some(&snapshot) {
            return;
        }
        // The menu is being rebuilt anyway, so take the opportunity to notice
        // the task having been changed from outside the app.
        self.reread_autostart_if_stale();

        let _ = self
            .tray
            .set_icon(Some(self.icons.for_level(snapshot.level)));
        let _ = self.tray.set_tooltip(Some(snapshot.tooltip()));
        if let Ok(menu) = build_menu(&snapshot, self.autostart_on) {
            self.tray.set_menu(Some(Box::new(menu)));
        }
        self.shown = Some(snapshot);
    }

    /// Re-reads the scheduled task state and rebuilds the menu to match.
    pub fn refresh_autostart(&mut self) {
        self.autostart_on = autostart::is_enabled();
        self.autostart_checked_at = Instant::now();
        if let Some(shown) = self.shown.clone()
            && let Ok(menu) = build_menu(&shown, self.autostart_on)
        {
            self.tray.set_menu(Some(Box::new(menu)));
        }
    }

    fn reread_autostart_if_stale(&mut self) {
        if self.autostart_checked_at.elapsed() >= AUTOSTART_TTL {
            self.autostart_on = autostart::is_enabled();
            self.autostart_checked_at = Instant::now();
        }
    }
}

fn build_menu(snapshot: &Snapshot, autostart_on: bool) -> Result<Menu, tray_icon::menu::Error> {
    let menu = Menu::new();

    match snapshot.level {
        LockLevel::Unknown => {
            let msg = snapshot.error.as_deref().unwrap_or("Query failed");
            menu.append(&disabled(&format!("⚠ {msg}")))?;
        }
        LockLevel::None => {
            menu.append(&disabled("No wakelocks — display and sleep are free"))?;
        }
        _ => {
            for (i, group) in snapshot.groups.iter().enumerate() {
                if i > 0 {
                    menu.append(&PredefinedMenuItem::separator())?;
                }
                menu.append(&disabled(group.name()))?;
                append_holders(&menu, group)?;
            }
        }
    }

    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id(ID_REFRESH, "Refresh now", true, None))?;
    menu.append(&CheckMenuItem::with_id(
        ID_AUTOSTART,
        "Start with Windows",
        true,
        autostart_on,
        None,
    ))?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id(ID_EXIT, "Exit", true, None))?;

    Ok(menu)
}

fn append_holders(menu: &Menu, group: &ModeGroup) -> Result<(), tray_icon::menu::Error> {
    for holder in group.holders.iter().take(MAX_MENU_HOLDERS) {
        let mut line = format!("    {}", holder.label);
        if holder.count > 1 {
            line.push_str(&format!(" ×{}", holder.count));
        }
        if let Some(reason) = &holder.reason {
            line.push_str(&format!(" — {reason}"));
        }
        menu.append(&disabled(&clip(&line)))?;
        menu.append(&disabled(&clip(&format!("        {}", holder.detail))))?;
    }
    let extra = group.holders.len().saturating_sub(MAX_MENU_HOLDERS);
    if extra > 0 {
        menu.append(&disabled(&format!("    (+{extra} more)")))?;
    }
    Ok(())
}

fn disabled(text: &str) -> MenuItem {
    MenuItem::new(text, false, None)
}

fn clip(s: &str) -> String {
    if s.chars().count() <= MAX_MENU_LINE {
        return s.to_string();
    }
    let mut out: String = s.chars().take(MAX_MENU_LINE.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_leaves_short_lines_alone() {
        assert_eq!(clip("short"), "short");
    }

    #[test]
    fn clip_bounds_long_lines() {
        let long = "x".repeat(500);
        let out = clip(&long);
        assert_eq!(out.chars().count(), MAX_MENU_LINE);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn clip_is_safe_on_multibyte_text() {
        let s = "ä".repeat(500);
        let out = clip(&s);
        assert_eq!(out.chars().count(), MAX_MENU_LINE);
    }
}
