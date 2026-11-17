# v3.10.2 | Linux Port

> [!IMPORTANT]
> This is a Linux build and as such will NOT work on Windows.

This release brings the Linux port up to upstream **v3.10.2** (from v3.8.0). See `CHANGELOG.md` for everything that changed upstream.

## New on Linux
- Everything from upstream v3.9 and v3.10: themes and the theme editor, click points, custom stop / pause / start zones, master switch key, click indicator, sounds, usage stats, settings search, first-run setup.
- Features the previous port had to drop now work: process whitelist / blacklist, custom stop zone and click point pickers, start with session.
- Hotkeys accept every key and mouse button upstream does, including left / right specific modifiers and key chords.
- Works with input remappers that grab devices (keyd, input-remapper).
- Correct window size on HiDPI displays with fractional scaling.

## Requirements
- Add your user to the `input` group for full Wayland support: `sudo usermod -aG input $USER`, then log in again.

## Known limitations
- Stop zones and position picking need the cursor position, which Wayland only exposes over XWayland windows.
- The process list only sees X11 / XWayland windows.
