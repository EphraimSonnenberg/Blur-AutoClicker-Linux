[![Downloads](https://img.shields.io/github/downloads/Zynix-Scripts/Blur-AutoClicker-Linux/total?style=for-the-badge&label=downloads)](https://github.com/Zynix-Scripts/Blur-AutoClicker-Linux/releases)
[![Version](https://img.shields.io/github/package-json/v/Zynix-Scripts/Blur-AutoClicker-Linux?style=for-the-badge&label=version)](https://github.com/Zynix-Scripts/Blur-AutoClicker-Linux/releases)
[![License](https://img.shields.io/github/license/Zynix-Scripts/Blur-AutoClicker-Linux?style=for-the-badge)](LICENSE)

# Blur Auto Clicker | Linux Port

## Wayland-First Auto Clicker for Linux

This port is built with **Wayland first as the other supports x11 fully**. Most auto clickers on Linux only support X11 or break under native Wayland. This project aims to be the autoclicker that actually works on modern compositors.

## Why

A lot of popular auto clickers like OP Auto Clicker and Speed Auto Clicker are inaccurate at higher speeds | setting 50 CPS might give you 40 or 60. This project aims for precision: click exactly at the CPS you set, even at high speeds.

Performance is a core focus. RAM usage stays around 50 MB and is designed to never exceed 100 MB.

---

## Platform Support

| Feature | X11 | Wayland (with XWayland) | Notes |
|---------|-----|-------------------------|-------|
| **Auto clicking / key pressing** | Full | Full | Wayland uses `uinput`, X11 uses XTEST |
| **Hotkeys (keyboard and mouse buttons)** | Full | Full | Read from evdev; falls back to X11 polling without the `input` group |
| **Click points / smooth cursor moves** | Full | Works | Wayland moves the cursor through an absolute `uinput` device |
| **Corner / edge / custom stop zones** | Full | Limited | Needs the cursor position, see below |
| **Process whitelist / blacklist** | Full | XWayland apps only | Native Wayland windows cannot be identified |
| **Overlay, click indicator, always on top** | Full | Full | The app's own windows run through XWayland |
| **Start with session** | Full | Full | XDG autostart entry |
| **Run as administrator prompts** | n/a | n/a | Not needed on Linux |

### Permissions

For full functionality your user needs access to `/dev/uinput` and `/dev/input/event*`:

```bash
sudo usermod -aG input $USER
```

Then log out and back in. Without it the app still works on a plain X11 session; on Wayland it tells you what is missing in its status line.

### Wayland notes

- On a Wayland session the app's own windows use XWayland (`GDK_BACKEND=x11` is set automatically; set it yourself to override). Wayland does not let a program place a window at exact screen coordinates, which the overlay and click indicator need.
- Simulated input goes through `uinput`, so it reaches native Wayland and XWayland windows alike.
- Wayland has no way to ask where the cursor is. The position comes from XWayland, which only updates while the cursor is over an XWayland window (most games through Proton / Wine). Stop zones and the position picker are unreliable over native Wayland windows.
- Input remappers that grab devices (keyd, input-remapper, ...) are supported.

Set `BLUR_INPUT_BACKEND=x11` or `BLUR_INPUT_BACKEND=uinput` to force how input is simulated.

---

## Features

<div align="center">
    <img src="https://github.com/Blur009/Blur-AutoClicker/blob/main/public/30s_500cps_Speed_Test.png" width="600"/>
</div>
<p align="center"><em>Blur Auto Clicker reaching 500 CPS steadily</em></p>

**Simple Mode:**
- On / Off indicator (Blur logo turns green when active)
- Individual mouse button settings (left, right, middle)
- Keyboard key pressing with lower/uppercase letter control
- Hold / Toggle activation modes
- Customizable hotkeys

**Advanced Mode** (includes all Simple Mode features plus):
- Adjustable click timing (duty cycle)
- Speed Range Mode (randomizes CPS within a range)
- Corner and edge stopping (failsafe stop zones)
- Click and Time limits (stop after a set number of clicks or elapsed time)
- Double clicks
- Position Clicking (pick a position | the mouse moves there and clicks)
- Clicks adjustable to per Second, Minute, Hour, or Day

**Other Features:**
- Click stats (total clicks, sessions, avg CPU)
- Multi-monitor aware edge/corner detection

---

## Quick Start

<div align="center">
  <a href="https://github.com/Zynix-Scripts/Blur-AutoClicker-Linux/releases/latest">
    <img src="https://github.com/machiav3lli/oandbackupx/blob/034b226cea5c1b30eb4f6a6f313e4dadcbb0ece4/badge_github.png" alt="Download from GitHub" height="75">
  </a>
</div>

Settings are stored in `~/.local/share/com.blur009.blurautoclicker/`, stats and diagnostics in `~/.local/share/BlurAutoClicker/`.

---

## Building From Source

### Requirements:
- Node.js 20 or newer
- Rust via `rustup`
- Linux system dependencies for Tauri (see [Tauri prerequisites](https://tauri.app/start/prerequisites/))

### Setup:
```bash
git clone https://github.com/Zynix-Scripts/Blur-AutoClicker-Linux.git
cd Blur-AutoClicker-Linux
```

**To setup automatically (Suggested option, Arch & Ubuntu)**:
```bash
./setup.sh
```
and follow the prompts

**OR**

Manually run (Best for development):
```bash
npm install
rustup default stable
```

Run the app in development:
```bash
npm exec tauri dev
```

Build a release bundle:
```bash
npm exec tauri build
```

Useful validation commands:
```bash
npm run lint
npm run frontend:build
cargo test --manifest-path src-tauri/Cargo.toml
```

The built binary and packages are written to `src-tauri/target/release/bundle/`.

---

## FAQ

<details>
<summary><b>Why is CPS capped at 500?</b></summary>

The practical limit for reliable mouse events on most systems is around 500 CPS. A 1000 CPS setting is available but not recommended.
</details>

<details>
<summary><b>Can I build from source?</b></summary>

Yes — see <a href="BUILDING.md">BUILDING.md</a> for setup, build, and validation commands. For contributing guidelines, see <a href="CONTRIBUTING.md">CONTRIBUTING.md</a>.
</details>

---

## Support the main Project

[![ko-fi](https://www.ko-fi.com/img/donate_sm.png)](https://ko-fi.com/blur009)

## Support the linux port
You can support the linux port by starring the repository and sharing it with friends. Thank you!

---

## License

This project is licensed under the [GNU General Public License v3.0](https://www.gnu.org/licenses/gpl-3.0.en.html#license-text).
