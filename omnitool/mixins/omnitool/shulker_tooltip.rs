// Shulker Box Tooltip (docs/spec/omnitool/tooltips.md, section 2): the text
// part. The item grid (2.7) needs a second hook in the tooltip drawing that
// Horizon's `stack_tooltip_lines` does not give, so `style = GRID` shows the
// text summary too. Ender chest memory (2.9) and the loot
// table info (2.11) are not done: Horizon's stacks carry no loot table, and
// the ender chest snapshot needs its own file format. The pure rules for the
// grid (key mode, layouts, counts, sizes) are here and tested. Added to the
// game as `game::omnitool_shulker_tooltip`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, NONE, SettingDef};
use super::omnitool_tooltips::{DARK_GRAY, GRAY, Line, StackView, summarize};

const KEYS: &[&str] = &["SHIFT", "ALT", "CTRL"];

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "shulker_tooltip",
    name: "Shulker Box Tooltip",
    category: Category::Misc,
    description: "Item grid preview of shulker boxes, bundles, other containers and your ender chest in the tooltip. Compact = same items merged, full = exact slots.",
    default_on: true,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "ShulkerBoxTooltip",
    settings: &[
        SettingDef::new(
            "style",
            "Style",
            Kind::Enum {
                values: &["GRID", "TEXT"],
                default: 0,
            },
        ),
        SettingDef::new(
            "theme",
            "Theme",
            Kind::Enum {
                values: &["SHULKER_BOX_TOOLTIP", "VANILLA"],
                default: 0,
            },
        ),
        SettingDef::new("colored", "Colored panel", Kind::Bool(true)),
        SettingDef::new(
            "ender_color",
            "Ender chest color",
            Kind::Color(0xFF2E6B62u32 as i32),
        ),
        SettingDef::new(
            "container_color",
            "Other containers color",
            Kind::Color(0xFFFFFFFFu32 as i32),
        ),
        SettingDef::new(
            "preview_key",
            "Preview key",
            Kind::Enum {
                values: KEYS,
                default: 0,
            },
        ),
        SettingDef::new(
            "full_preview_key",
            "Full preview key",
            Kind::Enum {
                values: KEYS,
                default: 1,
            },
        ),
        SettingDef::new(
            "always_on",
            "Always show preview (no key needed)",
            Kind::Bool(false),
        ),
        SettingDef::new(
            "swap_modes",
            "Swap compact and full preview",
            Kind::Bool(false),
        ),
        SettingDef::new(
            "merge",
            "Compact: items with different data",
            Kind::Enum {
                values: &["SEPARATE", "FIRST_ITEM", "IGNORE"],
                default: 0,
            },
        ),
        SettingDef::new(
            "order",
            "Compact: order",
            Kind::Enum {
                values: &["STACK_SIZE", "PREVIEW_ORDER"],
                default: 0,
            },
        ),
        SettingDef::new(
            "row_size",
            "Max row size",
            Kind::Number {
                default: 9.0,
                min: 1.0,
                max: 18.0,
                step: 1.0,
            },
        ),
        SettingDef::new(
            "max_rows",
            "Max rows",
            Kind::Number {
                default: 6.0,
                min: 1.0,
                max: 12.0,
                step: 1.0,
            },
        ),
        SettingDef::new("short_counts", "Short item counts (1.2k)", Kind::Bool(true)),
        SettingDef::new(
            "generic",
            "Preview other containers (chests, barrels, ...)",
            Kind::Bool(true),
        ),
        SettingDef::new("ender_chest", "Ender chest preview", Kind::Bool(true)),
        SettingDef::new(
            "bundles",
            "Bundle preview",
            Kind::Enum {
                values: &["OMNITOOL", "VANILLA"],
                default: 0,
            },
        ),
        SettingDef::new(
            "bundle_items",
            "Bundle: items shown (preview key = all)",
            Kind::Number {
                default: 8.0,
                min: 1.0,
                max: 64.0,
                step: 1.0,
            },
        ),
        SettingDef::new(
            "text_lines_mode",
            "Tooltip text",
            Kind::Enum {
                values: &["VANILLA", "OMNITOOL", "NONE"],
                default: 1,
            },
        ),
        SettingDef::new("key_hints", "Show key hints", Kind::Bool(true)),
        SettingDef::new(
            "loot_info",
            "Loot table info",
            Kind::Enum {
                values: &["HIDE", "SIMPLE", "ADVANCED"],
                default: 0,
            },
        ),
        SettingDef::new(
            "text_lines",
            "Max text lines",
            Kind::Number {
                default: 8.0,
                min: 1.0,
                max: 27.0,
                step: 1.0,
            },
        ),
    ],
    command: None,
    key: NONE,
    hold: false,
    hud: None,
    tick: None,
    on_key: None,
    hud_right: None,
    on_action: None,
};

/// Java's `down(Key)`: the setting's modifier is held.
fn down(host: &dyn Host, key_index: usize) -> bool {
    match key_index {
        0 => host.shift_down(),
        1 => host.alt_down(),
        _ => host.ctrl_down(),
    }
}

/// The display name of a `Key` constant (`keyName`, before `Lang.tr`).
fn key_name(key_index: usize) -> &'static str {
    ["Shift", "Alt", "Ctrl"][key_index.min(2)]
}

/// Java's `mode`: None = no preview, Some(true) = full, Some(false) = compact.
pub fn mode(preview_down: bool, full_down: bool, always_on: bool, swap: bool) -> Option<bool> {
    if !preview_down && !full_down && !always_on {
        return None;
    }
    Some(full_down != swap)
}

/// Java's `layout(path)`: `(slot count, row size)` of a container item.
#[allow(dead_code)] // needs the grid drawing
pub fn layout(path: &str) -> (usize, usize) {
    if path.ends_with("shulker_box") || path.ends_with("chest") || path == "barrel" {
        (27, 9)
    } else if path == "hopper" || path == "brewing_stand" {
        (5, 5)
    } else if matches!(path, "dispenser" | "dropper" | "crafter") {
        (9, 3)
    } else if matches!(path, "furnace" | "smoker" | "blast_furnace") {
        (3, 3)
    } else if path == "chiseled_bookshelf" {
        (6, 3)
    } else if path.ends_with("_shelf") {
        (3, 3)
    } else if path == "decorated_pot" {
        (1, 1)
    } else if path.ends_with("campfire") {
        (4, 4)
    } else {
        (9, 9)
    }
}

/// The slots a container preview shows (spec 2.4 step 7): rows up to the
/// last filled slot, at least the container's own size, at most 54.
#[allow(dead_code)] // needs the grid drawing
pub fn contents_size(path: &str, last: usize) -> usize {
    let (count, cols) = layout(path);
    count.max((last / cols + 1) * cols).min(54)
}

/// Java's `count(n, short)`: `1.2k`, `17k`, `1.5M`.
#[allow(dead_code)] // needs the grid drawing
pub fn short_count(n: i64, short: bool) -> String {
    if !short || n < 1000 {
        return n.to_string();
    }
    let trim = |s: String, unit: &str| s.replace(&format!(".0{unit}"), unit);
    if n < 10_000 {
        return trim(
            format!(
                "{}k",
                super::omnitool_tooltips::one_decimal(n as f64 / 1000.0)
            ),
            "k",
        );
    }
    if n < 1_000_000 {
        return format!("{}k", n / 1000);
    }
    trim(
        format!(
            "{}M",
            super::omnitool_tooltips::one_decimal(n as f64 / 1_000_000.0)
        ),
        "M",
    )
}

/// `(columns, shown rows)` of a grid with `n` items (spec 2.7 `build`).
#[allow(dead_code)] // needs the grid drawing
pub fn preview_layout(
    n: usize,
    full: bool,
    container_cols: usize,
    row_size: usize,
    max_rows: usize,
) -> (usize, usize) {
    let cols = if full {
        container_cols.min(row_size)
    } else {
        row_size.min(n).max(1)
    };
    let rows = n.div_ceil(cols).max(1);
    (cols, rows.min(max_rows))
}

/// `(width, height)` of the grid panel (`Preview.width/height`);
/// `name_width` is the selected bundle item's text width, if any.
#[allow(dead_code)] // needs the grid drawing
pub fn dims(cols: usize, rows: usize, more: usize, name_width: Option<usize>) -> (usize, usize) {
    let mut width = cols * 18 + 6;
    if let Some(w) = name_width {
        width = width.max(w + 8);
    }
    let panel = rows * 18 + 6 + if more > 0 { 10 } else { 0 };
    (width, panel + 2 + if name_width.is_some() { 11 } else { 0 })
}

/// Java's `appendTooltip` for the text part (spec 2.10 without ender chest
/// and loot): the vanilla lines to drop and the lines to add.
pub fn lines(host: &dyn Host, s: &StackView) -> (Vec<String>, Vec<Line>) {
    let none = (Vec::new(), Vec::new());
    if !core::active(host, &MODULE) {
        return none;
    }
    let v = |id: &str| core::value(host, &MODULE, id);
    let tr = |t: &str| core::tr(host, t);
    let path = s.id.rsplit(':').next().unwrap_or(&s.id);
    let text_max = v("text_lines").num() as usize;
    let summary = |slots: &[super::omnitool_tooltips::Slot]| -> Vec<Line> {
        let items: Vec<(String, i32)> = slots.iter().map(|x| (x.name.clone(), x.count)).collect();
        summarize(&items, text_max)
            .into_iter()
            .map(|t| Line::new(t, GRAY))
            .collect()
    };
    let hints = v("key_hints").on();
    // 2.4: what there is to show.
    if path == "ender_chest" {
        return none;
    }
    if let Some(bundle) = &s.bundle {
        if bundle.is_empty() || v("bundles").index() != 0 {
            return none;
        }
        return (Vec::new(), summary(bundle));
    }
    let Some(slots) = &s.container else {
        return none;
    };
    if !path.ends_with("shulker_box") && !v("generic").on() {
        return none;
    }
    if slots.is_empty() {
        return none;
    }
    let mut remove = Vec::new();
    let mut out = Vec::new();
    // 2.10 step 5/6.
    let text_mode = v("text_lines_mode").index();
    if text_mode != 0 {
        remove = s.vanilla_container.clone();
    }
    if text_mode == 1 {
        out.push(Line::new(
            tr("Contains %d stacks").replace("%d", &slots.len().to_string()),
            GRAY,
        ));
    }
    // Steps 7-9. (Step 10's hint about compact/full only exists with the grid.)
    let preview = mode(
        down(host, v("preview_key").index()),
        down(host, v("full_preview_key").index()),
        v("always_on").on(),
        v("swap_modes").on(),
    );
    if preview.is_none() {
        if hints {
            out.push(Line::new(
                tr("Hold %s to preview").replace("%s", &tr(key_name(v("preview_key").index()))),
                DARK_GRAY,
            ));
        }
        return (remove, out);
    }
    out.extend(summary(slots));
    (remove, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};
    use crate::omnitool_tooltips::Slot;
    use serde_json::Value;

    fn fixtures() -> Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/tooltips.json"
        );
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn fixture_cases() {
        let f = fixtures();
        for c in f["mode"].as_array().unwrap() {
            let b = |i: usize| c[i].as_bool().unwrap();
            assert_eq!(mode(b(0), b(1), b(2), b(3)), c[4].as_bool(), "{c}");
        }
        for c in f["layout"].as_array().unwrap() {
            let (n, r) = layout(c[0].as_str().unwrap());
            assert_eq!(
                [n as u64, r as u64],
                [c[1][0].as_u64().unwrap(), c[1][1].as_u64().unwrap()]
            );
        }
        for c in f["contents_size"].as_array().unwrap() {
            let got = contents_size(c[0].as_str().unwrap(), c[1].as_u64().unwrap() as usize);
            assert_eq!(got as u64, c[2].as_u64().unwrap(), "{c}");
        }
        for c in f["short_count"].as_array().unwrap() {
            assert_eq!(
                short_count(c[0].as_i64().unwrap(), c[1].as_bool().unwrap()),
                c[2].as_str().unwrap(),
                "{c}"
            );
        }
        for c in f["preview_layout"].as_array().unwrap() {
            let u = |i: usize| c[i].as_u64().unwrap() as usize;
            let got = preview_layout(u(0), c[1].as_bool().unwrap(), u(2), u(3), u(4));
            assert_eq!(got, (u(5), u(6)), "{c}");
        }
        for c in f["dims"].as_array().unwrap() {
            let u = |i: usize| c[i].as_u64().unwrap() as usize;
            // The name width is only used through `max(width, w + 8)`; the
            // fixture's width 150 for a name case comes from 8 columns.
            let name = c[3].as_bool().unwrap().then_some(0);
            assert_eq!(dims(u(0), u(1), u(2), name), (u(4), u(5)), "{c}");
        }
    }

    fn chest() -> StackView {
        StackView {
            id: "minecraft:shulker_box".into(),
            container: Some(vec![
                Slot {
                    name: "Diamond".into(),
                    count: 12,
                },
                Slot {
                    name: "Stick".into(),
                    count: 3,
                },
                Slot {
                    name: "Diamond".into(),
                    count: 4,
                },
            ]),
            vanilla_container: vec!["12 x Diamond".into()],
            ..Default::default()
        }
    }

    #[test]
    fn text_lines_without_and_with_key() {
        restart();
        let mut host = Fake::default();
        let (remove, out) = lines(&host, &chest());
        assert_eq!(remove, vec!["12 x Diamond".to_owned()]);
        assert_eq!(out[0], Line::new("Contains 3 stacks", GRAY));
        assert_eq!(out[1], Line::new("Hold Shift to preview", DARK_GRAY));
        assert_eq!(out.len(), 2);
        host.shift = true;
        let (_, out) = lines(&host, &chest());
        assert_eq!(out[1], Line::new("16x Diamond", GRAY));
        assert_eq!(out[2], Line::new("3x Stick", GRAY));
    }

    #[test]
    fn bundles_and_plain_items() {
        restart();
        let host = Fake::default();
        let bundle = StackView {
            id: "minecraft:bundle".into(),
            bundle: Some(vec![Slot {
                name: "Apple".into(),
                count: 2,
            }]),
            ..Default::default()
        };
        assert_eq!(lines(&host, &bundle).1, vec![Line::new("2x Apple", GRAY)]);
        assert_eq!(lines(&host, &StackView::default()), (vec![], vec![]));
    }
}
