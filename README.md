# WakeWatch

[![CI](https://github.com/NightHammer1000/wakewatch/actions/workflows/ci.yml/badge.svg)](https://github.com/NightHammer1000/wakewatch/actions/workflows/ci.yml)

A tiny Windows tray indicator that tells you, at a glance, whether anything is
holding your display or system awake.

Built for OLED screens, where an app silently pinning the display on means
static content burning into the panel while you are away from the desk.
Windows gives you no indication this is happening — the only way to find out is
running `powercfg /requests` in an elevated console, which nobody does
proactively.

| Icon | Meaning |
|---|---|
| 🟢 Green | No locks — display and sleep are free |
| 🟡 Yellow | Standby lock: the system cannot sleep |
| 🔴 Red | Display lock: the screen cannot turn off |
| ⚪ Grey | Could not read the lock state (see the tooltip) |

Hover for a summary. Right-click to see exactly what holds each lock, with
process paths, PIDs and reason strings — duplicates collapsed, so an app
holding thirty identical locks shows as `steam.exe ×30` rather than thirty rows.

Roughly 320 KB, ~11 MB resident, one syscall per second (~65 µs).

## Requires administrator

Enumerating system-wide power requests needs an elevated token. This is a
Windows restriction, not a design choice — `powercfg /requests` has exactly the
same requirement, and an unelevated call returns `STATUS_ACCESS_DENIED`. The
binary carries a `requireAdministrator` manifest.

Use the tray menu's **Start with Windows** to register a Scheduled Task that
runs at logon with highest privileges, so you are not prompted by UAC on every
boot. The same menu item removes it again.

## Install

Download `wakewatch.exe` from the [latest release][releases], put it somewhere
permanent, run it, and enable **Start with Windows** from the menu. Each
release ships a `wakewatch.exe.sha256` next to the binary.

[releases]: https://github.com/NightHammer1000/wakewatch/releases/latest

The binary is unsigned, so SmartScreen will warn on first run.

Or build it yourself:

```
cargo build --release
```

## How it works

WakeWatch calls `ntdll!NtPowerInformation` with the undocumented
`GetPowerRequestList` (45) information class — the same thing
`powercfg /requests` uses internally.

Notably, the *documented* `powrprof!CallNtPowerInformation` wrapper **rejects**
this information class with `STATUS_INVALID_PARAMETER`, so the syscall has to be
made against `ntdll` directly.

The returned `POWER_REQUEST` layout is undocumented and changes across Windows
versions, so `src/power.rs` reads it exclusively through bounds-checked
accessors that return `Option`, and validates every entry (plausible
`DIAGNOSTIC_BUFFER.Size`, `CallerType <= 2`, all string offsets inside the
buffer). Anything unexpected produces the grey "unknown" state rather than a
plausible but wrong colour. **Never** a false "all clear" — that is the one
failure mode that would defeat the tool.

The structure version is selected by OS build number. Deriving it from
`SupportedRequestMask` does not work: kernel-mode requesters were observed
reporting `0x12` rather than a full `0x3F`.

Only DISPLAY drives red, and SYSTEM/AWAYMODE drive yellow. EXECUTION,
PERFBOOST and ACTIVELOCKSCREEN are listed in the menu but deliberately do not
affect the colour — EXECUTION is process-lifetime (a browser playing audio
holds one) and has no bearing on the screen or on sleep.

### Why it polls

There is no Windows event for power-request changes.
`RegisterPowerSettingNotification` only reports power *settings* — display
on/off, AC/DC, away mode — which tells you the screen turned off, not that
something is stopping it from turning off. ETW's
`Microsoft-Windows-Kernel-Power` provider does trace power requests, but
consuming it means a real-time trace session against undocumented event IDs:
a lot of fragility for latency nobody would perceive. `powercfg` polls too.

Credit to [diversenok/Powercfg][dp] for reverse-engineering the information
class, and to [phnt][phnt] for the structure definitions.

[dp]: https://github.com/diversenok/Powercfg
[phnt]: https://github.com/winsiderss/systeminformer/blob/master/phnt/include/ntpoapi.h

## Development

```
cargo test                            # decoder, model, icons, timer, single-instance
cargo run --example dump              # decoded lock list; compare to powercfg /requests
cargo run --example timer_check       # proves the poll timer actually fires
cargo run --example startup_check     # walk the startup sequence on a console
cargo run --example autostart_check   # exercise the scheduled-task code path
```

`dump` is the ground-truth check: its output should agree with
`powercfg /requests` line for line. Both need an elevated shell.

### Releasing

Bump `version` in `Cargo.toml`, then tag and push:

```
git tag v0.1.0 && git push origin v0.1.0
```

CI refuses the tag if it disagrees with `Cargo.toml`, and otherwise builds,
tests, and publishes a release with the binary and its checksum attached.
Pushes to `main` only build — they do not create releases.

## Limitations

- Driver reasons stored as resource-string references are not resolved — only
  simple-string reasons are shown. The driver is still listed by name.
  Resolving them would need `LoadLibraryEx` + `FormatMessage`.
- x64 only. The struct layout assumes an 8-byte `SIZE_T`; the build fails
  loudly on other pointer widths rather than decoding garbage.

## License

MIT — see [LICENSE](LICENSE).
