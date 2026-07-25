//! Aggregation of raw power requests into what the tray actually displays.

use crate::devpath::{DevicePathMap, file_name};
use crate::power::{
    CALLER_KERNEL, CALLER_SERVICE, MODE_AWAYMODE, MODE_DISPLAY, MODE_NAMES, MODE_SYSTEM,
    PowerError, RawRequest,
};

/// Severity, ordered so `max()` picks the worst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LockLevel {
    /// Query or decode failed — never claim "no locks" when we do not know.
    Unknown,
    None,
    /// System cannot sleep.
    Standby,
    /// Display cannot turn off. The burn-in risk.
    Display,
}

/// One deduplicated requester within a mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    /// Short label, e.g. `opera.exe` or `USB Audio Device`.
    pub label: String,
    /// Full path / device name, shown as secondary detail.
    pub detail: String,
    pub reason: Option<String>,
    /// How many identical requests were collapsed into this row.
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeGroup {
    pub mode: usize,
    pub holders: Vec<Holder>,
}

impl ModeGroup {
    pub fn name(&self) -> &'static str {
        MODE_NAMES.get(self.mode).copied().unwrap_or("UNKNOWN")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub level: LockLevel,
    /// Non-empty groups, in mode order.
    pub groups: Vec<ModeGroup>,
    /// Set when `level == Unknown`.
    pub error: Option<String>,
}

impl Snapshot {
    pub fn failed(err: &PowerError) -> Self {
        Snapshot {
            level: LockLevel::Unknown,
            groups: Vec::new(),
            error: Some(err.describe()),
        }
    }

    pub fn group(&self, mode: usize) -> Option<&ModeGroup> {
        self.groups.iter().find(|g| g.mode == mode)
    }

    /// Short line for the tray tooltip.
    pub fn tooltip(&self) -> String {
        match self.level {
            LockLevel::Unknown => format!(
                "WakeWatch — unknown\n{}",
                self.error.as_deref().unwrap_or("Query failed")
            ),
            LockLevel::None => "WakeWatch — no wakelocks".to_string(),
            LockLevel::Display => {
                let who = self
                    .group(MODE_DISPLAY)
                    .map(|g| summarize(&g.holders))
                    .unwrap_or_default();
                truncate(&format!("Display lock: {who}"), TOOLTIP_MAX)
            }
            LockLevel::Standby => {
                let mut holders: Vec<&Holder> = Vec::new();
                for mode in [MODE_SYSTEM, MODE_AWAYMODE] {
                    if let Some(g) = self.group(mode) {
                        holders.extend(g.holders.iter());
                    }
                }
                let who = summarize_refs(&holders);
                truncate(&format!("Standby lock: {who}"), TOOLTIP_MAX)
            }
        }
    }
}

/// Windows truncates tray tooltips beyond 127 chars; stay comfortably inside.
const TOOLTIP_MAX: usize = 120;
/// Holders named inline before collapsing the rest into "(+N more)".
const TOOLTIP_HOLDERS: usize = 3;

fn summarize(holders: &[Holder]) -> String {
    summarize_refs(&holders.iter().collect::<Vec<_>>())
}

fn summarize_refs(holders: &[&Holder]) -> String {
    if holders.is_empty() {
        return "unknown".to_string();
    }
    let shown: Vec<String> = holders
        .iter()
        .take(TOOLTIP_HOLDERS)
        .map(|h| {
            if h.count > 1 {
                format!("{} x{}", h.label, h.count)
            } else {
                h.label.clone()
            }
        })
        .collect();
    let mut s = shown.join(", ");
    if holders.len() > TOOLTIP_HOLDERS {
        s.push_str(&format!(" (+{} more)", holders.len() - TOOLTIP_HOLDERS));
    }
    s
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Collapses the raw request list into per-mode, deduplicated holders.
pub fn build(requests: &[RawRequest], paths: &mut DevicePathMap) -> Snapshot {
    let mut groups = Vec::new();

    for mode in 0..MODE_NAMES.len() {
        let mut holders: Vec<Holder> = Vec::new();
        for req in requests.iter().filter(|r| r.holds(mode)) {
            let (label, detail) = describe(req, paths);
            // Dedup on the identity a user perceives, so Steam's 32 identical
            // SYSTEM requests become one row with a count.
            match holders
                .iter_mut()
                .find(|h| h.label == label && h.detail == detail && h.reason == req.reason)
            {
                Some(existing) => existing.count += 1,
                None => holders.push(Holder {
                    label,
                    detail,
                    reason: req.reason.clone(),
                    count: 1,
                }),
            }
        }
        if !holders.is_empty() {
            holders.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
            groups.push(ModeGroup { mode, holders });
        }
    }

    // Only DISPLAY and SYSTEM/AWAYMODE drive the colour. EXECUTION is
    // process-lifetime (a browser playing audio holds one) and PERFBOOST /
    // ACTIVELOCKSCREEN have no bearing on the screen or on sleep.
    let level = if groups.iter().any(|g| g.mode == MODE_DISPLAY) {
        LockLevel::Display
    } else if groups
        .iter()
        .any(|g| g.mode == MODE_SYSTEM || g.mode == MODE_AWAYMODE)
    {
        LockLevel::Standby
    } else {
        LockLevel::None
    };

    Snapshot {
        level,
        groups,
        error: None,
    }
}

fn describe(req: &RawRequest, paths: &mut DevicePathMap) -> (String, String) {
    if req.caller_type == CALLER_KERNEL {
        if req.name.is_empty() {
            return ("Kernel (legacy caller)".into(), "Kernel driver".into());
        }
        return (req.name.clone(), format!("Driver: {}", req.name));
    }

    let full = paths.translate(&req.name);
    let short = if full.is_empty() {
        "Unknown process".to_string()
    } else {
        file_name(&full).to_string()
    };
    let kind = if req.caller_type == CALLER_SERVICE {
        "Service"
    } else {
        "Process"
    };
    let detail = match req.pid {
        Some(pid) => format!("{kind} (pid {pid}): {full}"),
        None => format!("{kind}: {full}"),
    };
    (short, detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::power::{CALLER_PROCESS, MODE_EXECUTION};

    fn req(mode: usize, caller: u32, name: &str, pid: Option<u32>) -> RawRequest {
        let mut counts = [0u32; 6];
        counts[mode] = 1;
        RawRequest {
            counts,
            caller_type: caller,
            name: name.into(),
            pid,
            reason: None,
        }
    }

    #[test]
    fn no_requests_is_green() {
        let mut p = DevicePathMap::new();
        assert_eq!(build(&[], &mut p).level, LockLevel::None);
    }

    #[test]
    fn system_request_is_yellow() {
        let mut p = DevicePathMap::new();
        let r = [req(MODE_SYSTEM, CALLER_PROCESS, "x\\steam.exe", Some(1))];
        assert_eq!(build(&r, &mut p).level, LockLevel::Standby);
    }

    #[test]
    fn display_request_is_red_and_outranks_system() {
        let mut p = DevicePathMap::new();
        let r = [
            req(MODE_SYSTEM, CALLER_PROCESS, "x\\steam.exe", Some(1)),
            req(MODE_DISPLAY, CALLER_PROCESS, "x\\opera.exe", Some(2)),
        ];
        assert_eq!(build(&r, &mut p).level, LockLevel::Display);
    }

    #[test]
    fn execution_alone_stays_green() {
        let mut p = DevicePathMap::new();
        let r = [req(MODE_EXECUTION, CALLER_PROCESS, "x\\opera.exe", Some(1))];
        let s = build(&r, &mut p);
        assert_eq!(s.level, LockLevel::None);
        // ...but it is still listed for the user to see.
        assert!(s.group(MODE_EXECUTION).is_some());
    }

    #[test]
    fn identical_requests_collapse_with_a_count() {
        let mut p = DevicePathMap::new();
        let r: Vec<_> = (0..32)
            .map(|_| req(MODE_SYSTEM, CALLER_PROCESS, "x\\steam.exe", Some(7)))
            .collect();
        let s = build(&r, &mut p);
        let g = s.group(MODE_SYSTEM).unwrap();
        assert_eq!(g.holders.len(), 1);
        assert_eq!(g.holders[0].count, 32);
        assert_eq!(g.holders[0].label, "steam.exe");
    }

    #[test]
    fn nameless_kernel_requester_gets_a_readable_label() {
        let mut p = DevicePathMap::new();
        let r = [req(MODE_SYSTEM, CALLER_KERNEL, "", None)];
        let s = build(&r, &mut p);
        assert_eq!(
            s.group(MODE_SYSTEM).unwrap().holders[0].label,
            "Kernel (legacy caller)"
        );
    }

    #[test]
    fn failed_snapshot_is_unknown_never_none() {
        let s = Snapshot::failed(&PowerError::AccessDenied);
        assert_eq!(s.level, LockLevel::Unknown);
        assert!(s.tooltip().contains("Administrator"));
    }

    #[test]
    fn tooltip_stays_within_the_shell_limit() {
        let mut p = DevicePathMap::new();
        let r: Vec<_> = (0..40)
            .map(|i| {
                req(
                    MODE_DISPLAY,
                    CALLER_PROCESS,
                    &format!("C:\\very\\long\\path\\process-number-{i}.exe"),
                    Some(i),
                )
            })
            .collect();
        let tip = build(&r, &mut p).tooltip();
        assert!(tip.chars().count() <= TOOLTIP_MAX, "tooltip was {tip:?}");
    }

    #[test]
    fn tooltip_names_the_display_holder() {
        let mut p = DevicePathMap::new();
        let r = [req(
            MODE_DISPLAY,
            CALLER_PROCESS,
            "C:\\x\\opera.exe",
            Some(1),
        )];
        assert!(build(&r, &mut p).tooltip().contains("opera.exe"));
    }
}
