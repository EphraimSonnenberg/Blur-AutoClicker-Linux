//! Process list support (which application is in front, what is running),
//! built on `/proc` and the X11 window properties.
//!
//! This file replaces `engine/process.rs` on Linux and keeps its public
//! surface. Windows are discovered through X11, so native Wayland windows are
//! invisible here: they cannot be listed, and while one has focus the
//! foreground application is "unidentified" (a whitelist then blocks after a
//! short grace period, a blacklist allows, exactly as on Windows).

use super::ClickerConfig;
use std::collections::{HashMap, VecDeque};
use std::io::Cursor;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::error::poisoned_inner;
use crate::linux::win32::{VK_MENU, VK_TAB};
use crate::linux::{monitor, x11};

use image::ImageEncoder;

const PROCESS_DISPLAY_TITLE_MAX_CHARS: usize = 35;
const ICON_CACHE_MAX_ENTRIES: usize = 128;
const FOREGROUND_CACHE_TTL: Duration = Duration::from_millis(1000);
const FOREGROUND_UNKNOWN_GRACE: Duration = Duration::from_millis(250);

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub name: String,
    pub display_name: String,
    pub pid: u32,
    pub icon_base64: Option<String>,
}

#[derive(Default)]
struct IconCache {
    entries: HashMap<String, Option<String>>,
    order: VecDeque<String>,
}

impl IconCache {
    fn get(&self, name: &str) -> Option<Option<String>> {
        self.entries.get(name).cloned()
    }

    fn insert(&mut self, name: &str, icon: Option<String>) {
        if self.entries.contains_key(name) {
            self.entries.insert(name.to_string(), icon);
            return;
        }
        while self.entries.len() >= ICON_CACHE_MAX_ENTRIES {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.entries.insert(name.to_string(), icon);
        self.order.push_back(name.to_string());
    }
}

fn icon_cache() -> &'static Mutex<IconCache> {
    static CACHE: OnceLock<Mutex<IconCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(IconCache::default()))
}

fn base_name(path: &str) -> Option<String> {
    path.rsplit(['\\', '/'])
        .next()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn is_wine_loader(exe_name: &str) -> bool {
    let name = exe_name.to_ascii_lowercase();
    name.starts_with("wine") && name.contains("preloader") || name == "wine" || name == "wine64"
}

/// Picks the name users know a process by. Windows programs under Wine or
/// Proton all run from the same loader binary, so for those the first
/// argument (the `.exe`) is the useful name.
fn pick_process_name(exe: Option<&str>, argv0: Option<&str>, comm: Option<&str>) -> Option<String> {
    let exe_name = exe.and_then(base_name);
    let arg_name = argv0.and_then(base_name);
    match (&exe_name, &arg_name) {
        (Some(exe_name), Some(arg_name)) if is_wine_loader(exe_name) => Some(arg_name.clone()),
        (Some(exe_name), _) => Some(exe_name.trim_end_matches(" (deleted)").to_string()),
        (None, Some(arg_name)) => Some(arg_name.clone()),
        (None, None) => comm
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_string),
    }
}

/// Name of a running process, or `None` when it is gone or not ours to read.
pub fn process_name(pid: u32) -> Option<String> {
    let dir = format!("/proc/{pid}");
    let exe = std::fs::read_link(format!("{dir}/exe"))
        .ok()
        .map(|path| path.to_string_lossy().into_owned());
    let cmdline = std::fs::read(format!("{dir}/cmdline")).ok();
    let argv0 = cmdline.as_ref().and_then(|raw| {
        let first = raw.split(|byte| *byte == 0).next()?;
        (!first.is_empty()).then(|| String::from_utf8_lossy(first).into_owned())
    });
    let comm = std::fs::read_to_string(format!("{dir}/comm")).ok();
    pick_process_name(exe.as_deref(), argv0.as_deref(), comm.as_deref())
}

/// Entries are matched case-insensitively. Unlike Windows there is no `.exe`
/// suffix to add: native programs do not have one, and Wine programs already
/// report theirs.
pub fn normalize_process_name(name: &str) -> String {
    name.trim().to_lowercase()
}

fn encode_icon(window: u32) -> Option<String> {
    use base64::Engine;
    let (rgba, width, height) = x11::window_icon(window)?;
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(Cursor::new(&mut png))
        .write_image(&rgba, width, height, image::ColorType::Rgba8.into())
        .ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&png)
    ))
}

fn icon_for(name: &str, window: u32) -> Option<String> {
    if let Some(cached) = icon_cache().lock().unwrap_or_else(poisoned_inner).get(name) {
        return cached;
    }
    let icon = encode_icon(window);
    icon_cache()
        .lock()
        .unwrap_or_else(poisoned_inner)
        .insert(name, icon.clone());
    icon
}

fn truncate_title_for_display(title: &str) -> String {
    match title.char_indices().nth(PROCESS_DISPLAY_TITLE_MAX_CHARS) {
        Some((idx, _)) => title[..idx].to_string(),
        None => title.to_string(),
    }
}

struct ForegroundEntry {
    window: u32,
    pid: u32,
    name: String,
    captured: Instant,
}

#[derive(Default)]
struct ForegroundCache {
    entry: Option<ForegroundEntry>,
    unidentified_since: Option<Instant>,
}

impl ForegroundCache {
    fn note_identified(&mut self) {
        self.unidentified_since = None;
    }

    fn note_unidentified(&mut self, now: Instant) -> bool {
        let since = *self.unidentified_since.get_or_insert(now);
        now.saturating_duration_since(since) >= FOREGROUND_UNKNOWN_GRACE
    }

    fn get(&self, window: u32, pid: u32, now: Instant) -> Option<String> {
        let entry = self.entry.as_ref()?;
        if entry.window != window || entry.pid != pid {
            return None;
        }
        if now.saturating_duration_since(entry.captured) >= FOREGROUND_CACHE_TTL {
            return None;
        }
        Some(entry.name.clone())
    }

    fn store(&mut self, window: u32, pid: u32, name: &str, now: Instant) {
        self.entry = Some(ForegroundEntry {
            window,
            pid,
            name: name.to_string(),
            captured: now,
        });
    }
}

fn foreground_cache() -> &'static Mutex<ForegroundCache> {
    static CACHE: OnceLock<Mutex<ForegroundCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(ForegroundCache::default()))
}

pub fn reset_foreground_unknown_grace() {
    foreground_cache()
        .lock()
        .unwrap_or_else(poisoned_inner)
        .note_identified();
}

pub fn get_foreground_process_name() -> Option<String> {
    let window = x11::active_window()?;
    let pid = x11::window_pid(window)?;
    let now = Instant::now();
    {
        let cache = foreground_cache().lock().unwrap_or_else(poisoned_inner);
        if let Some(name) = cache.get(window, pid, now) {
            return Some(name);
        }
    }
    let name = process_name(pid)?;
    foreground_cache()
        .lock()
        .unwrap_or_else(poisoned_inner)
        .store(window, pid, &name, now);
    Some(name)
}

pub fn list_running_processes() -> Vec<ProcessInfo> {
    let own_pid = std::process::id();
    let mut by_name: HashMap<String, ProcessInfo> = HashMap::new();
    for window in x11::client_windows() {
        let Some(pid) = x11::window_pid(window) else {
            continue;
        };
        if pid == own_pid {
            continue;
        }
        let Some(title) = x11::window_title(window) else {
            continue;
        };
        let Some(name) = process_name(pid).map(|name| normalize_process_name(&name)) else {
            continue;
        };
        if by_name.contains_key(&name) {
            continue;
        }
        let icon_base64 = icon_for(&name, window);
        by_name.insert(
            name.clone(),
            ProcessInfo {
                name,
                display_name: truncate_title_for_display(&title),
                pid,
                icon_base64,
            },
        );
    }
    let mut result: Vec<ProcessInfo> = by_name.into_values().collect();
    result.sort_by_cached_key(|process| process.display_name.to_lowercase());
    result
}

pub const PROCESS_LIST_BLOCKED_REASON: &str = "Blocked by process list";
pub const PROCESS_LIST_UNKNOWN_REASON: &str =
    "Blocked by process list: foreground application could not be identified";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ForegroundMatch {
    Identified { is_in_list: bool },
    Unidentified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcessListVerdict {
    Allow,
    Blocked,
    Unidentified,
}

fn process_list_verdict(
    mode: super::ProcessListMode,
    foreground: ForegroundMatch,
) -> ProcessListVerdict {
    use super::ProcessListMode::{Blacklist, Whitelist};
    match (mode, foreground) {
        (Whitelist, ForegroundMatch::Identified { is_in_list: true }) => ProcessListVerdict::Allow,
        (Whitelist, ForegroundMatch::Identified { is_in_list: false }) => {
            ProcessListVerdict::Blocked
        }
        (Whitelist, ForegroundMatch::Unidentified) => ProcessListVerdict::Unidentified,
        (Blacklist, ForegroundMatch::Identified { is_in_list: true }) => {
            ProcessListVerdict::Blocked
        }
        (Blacklist, _) => ProcessListVerdict::Allow,
    }
}

pub fn check_process_list(config: &ClickerConfig) -> Option<String> {
    if !config.process_list_enabled {
        return None;
    }
    let foreground = match get_foreground_process_name() {
        Some(name) => {
            let current = normalize_process_name(&name);
            ForegroundMatch::Identified {
                is_in_list: config
                    .process_list_entries
                    .iter()
                    .any(|entry| entry.enabled && normalize_process_name(&entry.name) == current),
            }
        }
        None => ForegroundMatch::Unidentified,
    };
    let mut cache = foreground_cache().lock().unwrap_or_else(poisoned_inner);
    match process_list_verdict(config.process_list_mode, foreground) {
        ProcessListVerdict::Allow => {
            cache.note_identified();
            None
        }
        ProcessListVerdict::Blocked => {
            cache.note_identified();
            Some(String::from(PROCESS_LIST_BLOCKED_REASON))
        }
        ProcessListVerdict::Unidentified => cache
            .note_unidentified(Instant::now())
            .then(|| String::from(PROCESS_LIST_UNKNOWN_REASON)),
    }
}

/// Desktops do not expose their window switcher as a window we can detect,
/// so this is the keyboard half of the Windows check: Alt+Tab being held.
pub fn is_task_switcher_active() -> bool {
    monitor::is_down(VK_MENU as i32) && monitor::is_down(VK_TAB as i32)
}

pub fn is_process_running(name: &str) -> bool {
    let target = normalize_process_name(name);
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(process_name)
        .any(|candidate| normalize_process_name(&candidate) == target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ProcessListMode;

    #[test]
    fn native_programs_are_named_after_their_executable() {
        assert_eq!(
            pick_process_name(Some("/usr/bin/firefox"), Some("firefox"), Some("firefox")),
            Some(String::from("firefox"))
        );
        assert_eq!(
            pick_process_name(Some("/opt/app/bin (deleted)"), None, None),
            Some(String::from("bin"))
        );
    }

    #[test]
    fn wine_programs_are_named_after_the_windows_executable() {
        assert_eq!(
            pick_process_name(
                Some("/usr/bin/wine64-preloader"),
                Some("Z:\\games\\Some Game\\Game.exe"),
                Some("Game.exe"),
            ),
            Some(String::from("Game.exe"))
        );
    }

    #[test]
    fn kernel_threads_fall_back_to_comm() {
        assert_eq!(
            pick_process_name(None, None, Some("kworker/0:1\n")),
            Some(String::from("kworker/0:1"))
        );
        assert_eq!(pick_process_name(None, None, None), None);
    }

    #[test]
    fn names_are_normalized_without_inventing_an_extension() {
        assert_eq!(normalize_process_name("  FireFox "), "firefox");
        assert_eq!(normalize_process_name("Game.EXE"), "game.exe");
    }

    #[test]
    fn this_process_can_name_itself_and_is_running() {
        let name = process_name(std::process::id()).expect("own process is readable");
        assert!(!name.is_empty());
        assert!(is_process_running(&name));
        assert!(!is_process_running("definitely-not-a-real-process-name"));
    }

    #[test]
    fn whitelist_blocks_unlisted_and_defers_on_unidentified() {
        let listed = ForegroundMatch::Identified { is_in_list: true };
        let unlisted = ForegroundMatch::Identified { is_in_list: false };
        assert_eq!(
            process_list_verdict(ProcessListMode::Whitelist, listed),
            ProcessListVerdict::Allow
        );
        assert_eq!(
            process_list_verdict(ProcessListMode::Whitelist, unlisted),
            ProcessListVerdict::Blocked
        );
        assert_eq!(
            process_list_verdict(ProcessListMode::Whitelist, ForegroundMatch::Unidentified),
            ProcessListVerdict::Unidentified
        );
    }

    #[test]
    fn blacklist_blocks_only_listed_applications() {
        let listed = ForegroundMatch::Identified { is_in_list: true };
        let unlisted = ForegroundMatch::Identified { is_in_list: false };
        assert_eq!(
            process_list_verdict(ProcessListMode::Blacklist, listed),
            ProcessListVerdict::Blocked
        );
        assert_eq!(
            process_list_verdict(ProcessListMode::Blacklist, unlisted),
            ProcessListVerdict::Allow
        );
        assert_eq!(
            process_list_verdict(ProcessListMode::Blacklist, ForegroundMatch::Unidentified),
            ProcessListVerdict::Allow
        );
    }

    #[test]
    fn an_unidentified_foreground_only_blocks_after_the_grace_period() {
        let mut cache = ForegroundCache::default();
        let start = Instant::now();
        assert!(!cache.note_unidentified(start));
        assert!(!cache.note_unidentified(start + Duration::from_millis(100)));
        assert!(cache.note_unidentified(start + FOREGROUND_UNKNOWN_GRACE));
        cache.note_identified();
        assert!(!cache.note_unidentified(start + Duration::from_secs(5)));
    }

    #[test]
    fn icon_cache_evicts_the_oldest_entry_when_full() {
        let mut cache = IconCache::default();
        for index in 0..=ICON_CACHE_MAX_ENTRIES {
            cache.insert(&format!("app{index}"), None);
        }
        assert_eq!(cache.entries.len(), ICON_CACHE_MAX_ENTRIES);
        assert!(cache.get("app0").is_none());
        assert!(cache.get("app1").is_some());
    }
}
