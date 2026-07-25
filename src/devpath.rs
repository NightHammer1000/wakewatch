//! Translation of NT device paths to familiar DOS paths.
//!
//! The power request list reports process images as
//! `\Device\HarddiskVolume3\Users\...`. Nobody wants to read that in a tooltip,
//! so map the device prefix back to a drive letter via QueryDosDeviceW.

use windows::Win32::Storage::FileSystem::QueryDosDeviceW;
use windows::core::PCWSTR;

pub struct DevicePathMap {
    /// (`\Device\HarddiskVolume3`, `C:`), longest device prefix first.
    entries: Vec<(String, String)>,
}

impl DevicePathMap {
    pub fn new() -> Self {
        let mut map = DevicePathMap {
            entries: Vec::new(),
        };
        map.refresh();
        map
    }

    pub fn refresh(&mut self) {
        let mut entries = Vec::new();
        for letter in b'A'..=b'Z' {
            let dos = format!("{}:", letter as char);
            if let Some(device) = query_dos_device(&dos) {
                entries.push((device, dos));
            }
        }
        // Longest first so `\Device\HarddiskVolume30` cannot be shadowed by
        // `\Device\HarddiskVolume3`.
        entries.sort_by_key(|e| std::cmp::Reverse(e.0.len()));
        self.entries = entries;
    }

    /// Returns the DOS form if the path sits under a known device, else None.
    pub fn try_translate(&self, nt_path: &str) -> Option<String> {
        for (device, dos) in &self.entries {
            if let Some(rest) = nt_path.strip_prefix(device.as_str()) {
                // Only match on a path boundary, never mid-component.
                if rest.is_empty() || rest.starts_with('\\') {
                    return Some(format!("{dos}{rest}"));
                }
            }
        }
        None
    }

    /// Translates, rebuilding the map once if the first attempt misses —
    /// drives can appear while we are running.
    pub fn translate(&mut self, nt_path: &str) -> String {
        if let Some(p) = self.try_translate(nt_path) {
            return p;
        }
        if nt_path.starts_with("\\Device\\") {
            self.refresh();
            if let Some(p) = self.try_translate(nt_path) {
                return p;
            }
        }
        nt_path.to_string()
    }
}

impl Default for DevicePathMap {
    fn default() -> Self {
        Self::new()
    }
}

fn query_dos_device(dos_name: &str) -> Option<String> {
    let wide: Vec<u16> = dos_name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = vec![0u16; 1024];
    let len = unsafe { QueryDosDeviceW(PCWSTR(wide.as_ptr()), Some(&mut buf)) };
    if len == 0 {
        return None;
    }
    // The result is a NUL-separated list; the first entry is what we want.
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    if end == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..end]))
}

/// Last path component, for compact display.
pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> DevicePathMap {
        DevicePathMap {
            entries: vec![
                ("\\Device\\HarddiskVolume30".to_string(), "E:".to_string()),
                ("\\Device\\HarddiskVolume3".to_string(), "C:".to_string()),
            ],
        }
    }

    #[test]
    fn translates_a_known_device() {
        let m = fixture();
        assert_eq!(
            m.try_translate("\\Device\\HarddiskVolume3\\Users\\n\\opera.exe")
                .as_deref(),
            Some("C:\\Users\\n\\opera.exe")
        );
    }

    #[test]
    fn prefers_the_longer_device_prefix() {
        let m = fixture();
        assert_eq!(
            m.try_translate("\\Device\\HarddiskVolume30\\x.exe")
                .as_deref(),
            Some("E:\\x.exe")
        );
    }

    #[test]
    fn does_not_match_mid_component() {
        let m = fixture();
        assert_eq!(m.try_translate("\\Device\\HarddiskVolume31\\x.exe"), None);
    }

    #[test]
    fn unknown_device_is_left_alone() {
        let mut m = fixture();
        let raw = "\\Device\\Nope\\x.exe";
        // refresh() will run against the real system and still not match.
        assert!(m.translate(raw).ends_with("x.exe"));
    }

    #[test]
    fn file_name_extracts_last_component() {
        assert_eq!(file_name("C:\\a\\b\\c.exe"), "c.exe");
        assert_eq!(file_name("c.exe"), "c.exe");
        assert_eq!(file_name(""), "");
    }
}
