//! Session-end guard. The Windows build needs one to survive `WM_ENDSESSION`;
//! on Linux the session manager just sends SIGTERM and the default handling
//! is fine, so there is nothing to install.

pub fn shutdown_requested() -> bool {
    false
}

pub fn install() {}
