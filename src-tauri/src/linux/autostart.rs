//! Start-with-session via an XDG autostart entry
//! (`~/.config/autostart/BlurAutoClicker.desktop`).

use std::io;
use std::path::PathBuf;

const ENTRY_NAME: &str = "BlurAutoClicker.desktop";

fn entry_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("autostart").join(ENTRY_NAME))
}

/// Quotes a path for the `Exec` key of a desktop entry.
fn quote_exec(path: &str) -> String {
    let mut quoted = String::with_capacity(path.len() + 2);
    quoted.push('"');
    for character in path.chars() {
        if matches!(character, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted.replace('%', "%%")
}

fn entry_contents(exe: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=BlurAutoClicker\nComment=Start BlurAutoClicker in the tray\nExec={} --autostart\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        quote_exec(exe)
    )
}

pub fn get_autostart_enabled() -> bool {
    if crate::portable::is_portable() {
        return false;
    }
    entry_path().is_some_and(|path| path.is_file())
}

pub fn set_autostart_enabled(enabled: bool) -> io::Result<()> {
    if crate::portable::is_portable() {
        log::info!("[Autostart] Skipping autostart entry in portable mode");
        return Ok(());
    }
    let path = entry_path().ok_or_else(|| io::Error::other("no config directory"))?;
    if !enabled {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    // An AppImage is mounted at a different path on every launch; the image
    // itself is what has to be started.
    let exe = match std::env::var_os("APPIMAGE") {
        Some(appimage) => PathBuf::from(appimage),
        None => std::env::current_exe()?,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, entry_contents(&exe.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_line_quotes_spaces_and_escapes_field_codes() {
        let entry = entry_contents("/opt/My Apps/100%/blur \"x\"");
        assert!(entry.contains("Exec=\"/opt/My Apps/100%%/blur \\\"x\\\"\" --autostart\n"));
        assert!(entry.starts_with("[Desktop Entry]\n"));
    }
}
