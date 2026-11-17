//! Linux backend.
//!
//! Upstream BlurAutoClicker is written against Win32. Rather than rewriting
//! each shared module, the Linux port provides:
//!
//! * `win32`   - the handful of Win32 names the shared code uses (virtual key
//!   codes, low level hook plumbing, a few queries), backed by the modules
//!   below. Shared files import from here on Linux and from `windows_sys` on
//!   Windows, so they stay close to upstream and merge cleanly.
//! * `keymap`  - Windows virtual key <-> evdev key code tables.
//! * `x11`     - X11 / XWayland queries and XTEST injection.
//! * `uinput`  - kernel level injection, the only thing that reaches native
//!   Wayland windows.
//! * `monitor` - global key / button state from evdev (or X11 polling).
//! * `inject`  - picks an injection backend and exposes button / key / move.
//! * `startup` - warms the backends up and reports missing permissions.
//!
//! Whole-file replacements for the Windows-only modules (elevation, process
//! list, autostart, session end) live next to these and are swapped in with
//! `#[path]` from `lib.rs` / `engine/mod.rs`.

pub mod inject;
pub mod keymap;
pub mod monitor;
pub mod startup;
pub mod uinput;
pub mod win32;
pub mod x11;
