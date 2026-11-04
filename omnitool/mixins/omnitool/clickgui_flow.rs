// Pure data and geometry for the ClickGui's dropdown and workflow editor
// (spec clickgui.md 2.8, 4.6, appendix B). No game access.

use super::omnitool_workflows::{Arg, STEPS};

// ------------------------------------------------------------ dropdown

/// Row height of a dropdown.
pub const DROP_ROW: i32 = 12;

/// Visible rows: `min(labels, max(6, (screen_h - 12) / 12))`.
pub fn dropdown_rows(labels: usize, screen_h: i32) -> usize {
    labels.min(6.max((screen_h - 12) / DROP_ROW) as usize)
}

/// Top edge: `max(4, min(y, screen_h - rows * 12 - 4))`.
pub fn dropdown_top(y: i32, rows: usize, screen_h: i32) -> i32 {
    4.max(y.min(screen_h - rows as i32 * DROP_ROW - 4))
}

/// Scrollbar of a long dropdown: (bar height, offset below the top).
pub fn dropdown_bar(labels: usize, rows: usize, scroll: usize) -> (i32, i32) {
    let area = rows as i32 * DROP_ROW;
    let bar_h = 8.max(area * rows as i32 / labels.max(1) as i32);
    let span = labels.saturating_sub(rows).max(1) as i32;
    (bar_h, (area - bar_h) * scroll as i32 / span)
}

/// Hint box next to a hovered entry: (width, x). `fits` = the box fits right
/// of the list.
pub fn hint_box(text_width: i32, list_x: i32, list_w: i32, screen_w: i32) -> (i32, i32) {
    let hw = 180.min(text_width + 8);
    let x = if list_x + list_w + 4 + hw <= screen_w {
        list_x + list_w + 4
    } else {
        list_x - hw - 4
    };
    (hw, x)
}

/// An open dropdown.
#[derive(Clone, Debug, PartialEq)]
pub struct Dropdown {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub labels: Vec<String>,
    /// Indices of header rows (not pickable).
    pub headers: Vec<usize>,
    /// Hover hint per entry ("" = none).
    pub hints: Vec<String>,
    pub scroll: usize,
    pub target: Pick,
}

/// What a pick does.
#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    /// Step type of step `.1` of workflow `.0`: the entry value is the step id.
    StepType(u64, usize, Vec<String>),
    /// New step at the end: entry values are step ids.
    NewStep(u64, Vec<String>),
    /// Argument of step `.1` (values: the stored strings).
    StepArg(u64, usize, Vec<String>),
    Mode(u64),
    Trigger(u64),
}

impl Dropdown {
    pub fn rows(&self, screen_h: i32) -> usize {
        dropdown_rows(self.labels.len(), screen_h)
    }
    pub fn top(&self, screen_h: i32) -> i32 {
        dropdown_top(self.y, self.rows(screen_h), screen_h)
    }
    /// Mouse wheel: `amount` positive = up.
    pub fn wheel(&mut self, amount: f64, screen_h: i32) {
        let max = self.labels.len().saturating_sub(self.rows(screen_h));
        let step = if amount > 0.0 {
            -1
        } else if amount < 0.0 {
            1
        } else {
            0
        };
        self.scroll = (self.scroll as i32 + step).clamp(0, max as i32) as usize;
    }
    /// The entry under a click in the list: None = outside, Some(None) = a
    /// header row.
    pub fn index_at(&self, mx: i32, my: i32, screen_h: i32) -> Option<Option<usize>> {
        let top = self.top(screen_h);
        let rows = self.rows(screen_h) as i32;
        if mx < self.x || mx >= self.x + self.w || my < top || my >= top + rows * DROP_ROW {
            return None;
        }
        let i = ((my - top) / DROP_ROW) as usize + self.scroll;
        if i >= self.labels.len() || self.headers.contains(&i) {
            return Some(None);
        }
        Some(Some(i))
    }
}

// ------------------------------------------------------------ steps

pub const GROUPS: [(&str, u32); 7] = [
    ("Modules & settings", 0xFF3F_A9F5),
    ("Chat & commands", 0xFFA9_8BFF),
    ("Flow", 0xFFFF_C940),
    ("Movement & clicks", 0xFF3F_CF6A),
    ("Items & crafting", 0xFFFF_A040),
    ("Villagers", 0xFF2F_D3C0),
    ("Camera & world", 0xFFFF_6FA8),
];

/// Group index of a step id (`ClickGui.group`).
pub fn group(id: &str) -> usize {
    match id {
        "toggle" | "enable" | "disable" | "set" | "profile" | "safemode" => 0,
        "chat" | "command" => 1,
        "wait" | "run" | "if" | "else" | "end" | "repeat" | "stop" => 2,
        "walk" | "move" | "look" | "turn" | "jump" | "hold" | "attack" | "use" => 3,
        "hotbar" | "hotbar_preset" | "sort" | "open" | "close" | "craft" => 4,
        "trade" | "villager" => 5,
        _ => 6,
    }
}

/// (English label, English hint) of a step id (appendix B).
pub fn label_hint(id: &str) -> (&'static str, &'static str) {
    match id {
        "toggle" => ("Toggle module", ""),
        "enable" => ("Turn on", ""),
        "disable" => ("Turn off", ""),
        "set" => ("Set setting", "zoom.factor=8"),
        "profile" => ("Load profile", ""),
        "safemode" => ("Safe Mode", ""),
        "chat" => ("Send chat", "Hello!"),
        "command" => ("Run command", "/home"),
        "wait" => ("Wait", "seconds or ms, e.g. 2 / 250ms"),
        "run" => ("Run workflow", ""),
        "waypoint" => ("Add waypoint", "name (optional)"),
        "sort" => ("Sort inventory", ""),
        "panorama" => ("Panorama", ""),
        "disconnect" => ("Disconnect", ""),
        "tripod_save" => ("Save tripod (1-9)", ""),
        "tripod_load" => ("Load tripod (1-9)", ""),
        "freecam_control" => ("Freecam: control player", ""),
        "walk" => ("Walk", "forward 3 (blocks) / left 2s / 500ms"),
        "look" => ("Look at", "north / 90 0 / x y z"),
        "turn" => ("Turn", "90 / -45 10 / around"),
        "jump" => ("Jump", ""),
        "attack" => ("Attack (left click)", ""),
        "use" => ("Use (right click)", ""),
        "hold" => ("Hold key", "use 2 / attack 1.5 / sneak 3"),
        "hotbar" => ("Hotbar slot (1-9)", "1-9"),
        "move" => ("Move (several keys)", "forward+left+sprint 1.2s"),
        "open" => (
            "Open / talk to (block, villager, inventory)",
            "crafting_table / chest / villager / inventory",
        ),
        "craft" => ("Craft", "stick 16 / torch all / planks all drop"),
        "close" => ("Close screen", ""),
        "if" => ("If", "health < 10 / holding stick / not sneaking"),
        "else" => ("Else", ""),
        "end" => ("End (if / repeat)", ""),
        "repeat" => ("Repeat", "3 / forever"),
        "stop" => ("Stop workflow", ""),
        "hotbar_preset" => ("Load hotbar preset", "preset name"),
        "trade" => (
            "Trade with villager",
            "emerald all / sell wheat 5 / mending / from farmer",
        ),
        "villager" => (
            "Villager: preferred trade",
            "Trades the preferred trade of the villager's profession (Villager Trades module) completely",
        ),
        "camera_path" => ("Play camera path", "path name (empty = selected path)"),
        "camera_stop" => ("Stop camera path", ""),
        _ => ("", ""),
    }
}

/// Argument of a freshly picked step (`defaultArg`); `module` and `profile`
/// are the first module id and the first profile ("" = none).
pub fn default_arg(id: &str, module: &str, profile: &str) -> String {
    let Some(step) = STEPS.iter().find(|s| s.id == id) else {
        return String::new();
    };
    match step.arg {
        Arg::None => String::new(),
        Arg::Module => module.to_owned(),
        Arg::Profile => profile.to_owned(),
        Arg::Workflow => String::new(),
        Arg::OnOff => "toggle".to_owned(),
        Arg::Text => match id {
            "wait" | "tripod_save" | "tripod_load" | "hotbar" => "1",
            "walk" => "forward 3",
            "look" => "north",
            "turn" => "90",
            "hold" => "use 2",
            "open" => "crafting_table",
            "craft" => "stick 16",
            "move" => "forward+sprint 1s",
            "if" => "health < 10",
            "repeat" => "3",
            "trade" => "emerald all",
            _ => "",
        }
        .to_owned(),
    }
}

/// The stored step for a picked id: `id` or `id:defaultArg`.
pub fn new_step(id: &str, module: &str, profile: &str) -> String {
    let a = default_arg(id, module, profile);
    if a.is_empty() {
        id.to_owned()
    } else {
        format!("{id}:{a}")
    }
}

/// Whether `s` is `x y z` (numbers with an optional fraction), for the
/// trade/villager coordinate field.
pub fn coords_valid(s: &str) -> bool {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let num = |p: &str| {
        let p = p.strip_prefix('-').unwrap_or(p);
        let (int, frac) = match p.split_once('.') {
            Some((i, f)) => (i, Some(f)),
            None => (p, None),
        };
        !int.is_empty()
            && int.bytes().all(|b| b.is_ascii_digit())
            && frac.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
    };
    parts.len() == 3 && parts.iter().all(|p| num(p))
}

/// Closing step to append after a new `if` / `repeat`.
pub fn needs_end(id: &str) -> bool {
    matches!(id, "if" | "repeat")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value as Json;

    fn fixtures() -> Json {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/clickgui.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn n(j: &Json, k: &str) -> i32 {
        j.get(k).unwrap().as_f64().unwrap() as i32
    }

    #[test]
    fn dropdown_geometry_fixtures() {
        for c in fixtures()["dropdown_geometry"].as_array().unwrap() {
            let rows = dropdown_rows(n(c, "labels") as usize, n(c, "screen_h"));
            assert_eq!(rows as i32, n(c, "rows"), "{c}");
            assert_eq!(
                dropdown_top(n(c, "y"), rows, n(c, "screen_h")),
                n(c, "top"),
                "{c}"
            );
        }
        for c in fixtures()["dropdown_scrollbar"].as_array().unwrap() {
            let (h, off) = dropdown_bar(
                n(c, "labels") as usize,
                n(c, "rows") as usize,
                n(c, "scroll") as usize,
            );
            assert_eq!((h, off), (n(c, "bar_h"), n(c, "bar_offset")));
        }
    }

    #[test]
    fn hint_box_fixtures() {
        for c in fixtures()["dropdown_hint_box"].as_array().unwrap() {
            let hint = c["hint"].as_str().unwrap();
            let (w, _) = hint_box(hint.chars().count() as i32 * 6, 0, 100, 1000);
            assert_eq!(w, n(c, "hint_w"), "{hint}");
        }
        // does not fit on the right: left of the list
        assert_eq!(hint_box(40, 900, 100, 1000), (48, 900 - 48 - 4));
        assert_eq!(hint_box(40, 100, 100, 1000), (48, 204));
    }

    #[test]
    fn default_args_match_fixture() {
        let f = &fixtures()["step_default_arg"];
        for (k, v) in f.as_object().unwrap() {
            if k.ends_with("_ARG") || k == "note" {
                continue;
            }
            let id = k.to_lowercase();
            assert_eq!(
                default_arg(&id, "fullbright", ""),
                v.as_str().unwrap(),
                "{k}"
            );
        }
        assert_eq!(default_arg("toggle", "zoom", ""), "zoom");
        assert_eq!(default_arg("safemode", "", ""), "toggle");
        assert_eq!(new_step("jump", "", ""), "jump");
        assert_eq!(new_step("repeat", "", ""), "repeat:3");
    }

    #[test]
    fn every_step_has_a_label_and_group() {
        for s in STEPS.iter() {
            assert!(!label_hint(s.id).0.is_empty(), "{}", s.id);
            assert!(group(s.id) < GROUPS.len());
        }
        assert_eq!(group("camera_path"), 6);
        assert_eq!(group("sort"), 4);
    }

    #[test]
    fn coords() {
        assert!(coords_valid("1 -2 3.5"));
        assert!(!coords_valid("1 2"));
        assert!(!coords_valid("1 2 x"));
        assert!(!coords_valid("1. 2 3"));
    }

    #[test]
    fn dropdown_wheel_and_click() {
        let d = Dropdown {
            x: 10,
            y: 20,
            w: 100,
            labels: (0..50).map(|i| i.to_string()).collect(),
            headers: vec![0],
            hints: Vec::new(),
            scroll: 0,
            target: Pick::Mode(1),
        };
        let mut d = d;
        d.wheel(-1.0, 240);
        assert_eq!(d.scroll, 1);
        d.wheel(1.0, 240);
        d.wheel(1.0, 240);
        assert_eq!(d.scroll, 0);
        let top = d.top(240);
        assert_eq!(d.index_at(15, top + 1, 240), Some(None)); // header
        assert_eq!(d.index_at(15, top + 13, 240), Some(Some(1)));
        assert_eq!(d.index_at(500, top, 240), None);
    }
}
