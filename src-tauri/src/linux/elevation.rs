//! Elevation state. On Windows a non-elevated process cannot send input to an
//! elevated window, so the app detects that and offers to restart as
//! administrator. Linux input injection has no such boundary (uinput and
//! XTEST reach every window), so this reports "nothing to do" and keeps the
//! shared types the rest of the app expects.

pub const MAX_SUPPRESSED_WINDOWS: usize = 16;
pub const MAX_LABEL_CHARS: usize = 120;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElevationState {
    pub elevated: bool,
    pub can_elevate: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockedWindow {
    #[serde(skip_serializing)]
    pub handle: isize,
    #[serde(skip_serializing)]
    pub pid: u32,
    pub name: String,
    pub title: String,
}

pub fn current_process_elevated() -> bool {
    unsafe { libc::geteuid() == 0 }
}

pub fn can_elevate() -> bool {
    false
}

pub fn state() -> ElevationState {
    ElevationState {
        elevated: current_process_elevated(),
        can_elevate: can_elevate(),
    }
}

pub fn clean_label(raw: &str) -> String {
    let filtered: String = raw
        .chars()
        .map(|character| {
            if character.is_control() || is_bidi_control(character) {
                ' '
            } else {
                character
            }
        })
        .collect();
    let collapsed = filtered.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_LABEL_CHARS {
        return collapsed;
    }
    let mut truncated: String = collapsed.chars().take(MAX_LABEL_CHARS).collect();
    let keep = truncated.trim_end().len();
    truncated.truncate(keep);
    truncated.push('…');
    truncated
}

fn is_bidi_control(character: char) -> bool {
    matches!(
        character,
        '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
    )
}

pub fn describe_window(handle: isize, pid: u32) -> BlockedWindow {
    let title = u32::try_from(handle)
        .ok()
        .and_then(crate::linux::x11::window_title)
        .map(|title| clean_label(&title))
        .unwrap_or_default();
    let name = crate::engine::process::process_name(pid)
        .map(|name| clean_label(&name))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| title.clone());
    BlockedWindow {
        handle,
        pid,
        name,
        title,
    }
}

pub fn remember_suppressed(list: &mut Vec<(isize, u32)>, entry: (isize, u32)) {
    if list.contains(&entry) {
        return;
    }
    if list.len() >= MAX_SUPPRESSED_WINDOWS {
        list.remove(0);
    }
    list.push(entry);
}

/// Never: there is no window our input cannot reach for lack of privileges.
pub fn foreground_needs_prompt(_suppressed: &[(isize, u32)]) -> Option<(isize, u32)> {
    None
}

pub fn relaunch_elevated(_keep_autostart: bool) -> bool {
    false
}

pub fn relaunch_unelevated(_keep_autostart: bool) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_window_serializes_only_its_labels() {
        let window = BlockedWindow {
            handle: 42,
            pid: 7,
            name: String::from("Task Manager"),
            title: String::from("Task Manager"),
        };
        let json = serde_json::to_value(&window).unwrap();
        assert_eq!(json["name"], "Task Manager");
        assert!(json.get("handle").is_none());
        assert!(json.get("pid").is_none());
    }

    #[test]
    fn labels_lose_control_characters_and_are_capped() {
        assert_eq!(clean_label("a\u{202e}b\n\tc"), "a b c");
        let long = "x".repeat(MAX_LABEL_CHARS + 10);
        let cleaned = clean_label(&long);
        assert_eq!(cleaned.chars().count(), MAX_LABEL_CHARS + 1);
        assert!(cleaned.ends_with('…'));
    }

    #[test]
    fn suppressed_list_is_bounded_and_deduplicated() {
        let mut list = Vec::new();
        for index in 0..(MAX_SUPPRESSED_WINDOWS as isize + 4) {
            remember_suppressed(&mut list, (index, 1));
            remember_suppressed(&mut list, (index, 1));
        }
        assert_eq!(list.len(), MAX_SUPPRESSED_WINDOWS);
        assert_eq!(list[0], (4, 1));
    }

    #[test]
    fn nothing_ever_needs_an_elevation_prompt() {
        assert!(foreground_needs_prompt(&[]).is_none());
        assert!(!can_elevate());
        assert!(!relaunch_elevated(true));
    }
}
