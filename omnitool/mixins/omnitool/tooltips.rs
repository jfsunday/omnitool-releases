// Better Tooltips (docs/spec/omnitool/tooltips.md, section 1) and the shared
// part of the three tooltip modules: the stack as the modules see it, the
// lines they add, and `append`, which runs them in Java's order (Better
// Tooltips, Shulker Box Tooltip, AppleSkin). Mixin needed to apply them:
// RETURN of `Game::stack_tooltip_lines` (game/stations.rs:1818), see
// glue.rs `tooltip_lines`. Added to the game as `game::omnitool_tooltips`;
// also a module of this crate.

use super::omnitool_appleskin as appleskin;
use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, NONE, SettingDef};
use super::omnitool_shulker_tooltip as shulker;

/// One non-empty slot of a container or bundle: the displayed name
/// (`getHoverName`) and the count.
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub name: String,
    pub count: i32,
}

/// What the tooltip modules need of an `ItemStack`.
#[derive(Clone, Debug, Default)]
pub struct StackView {
    /// `namespace:path`.
    pub id: String,
    pub damage: i32,
    /// 0 = not damageable (or unbreakable).
    pub max_damage: i32,
    /// `FOOD`: nutrition and saturation.
    pub food: Option<(i32, f32)>,
    /// The stack has `CONSUMABLE` (AppleSkin only reads food with it).
    pub consumable: bool,
    /// `CONTAINER`: the non-empty slots in slot order, None = no component.
    pub container: Option<Vec<Slot>>,
    /// `BUNDLE_CONTENTS` in the game's order, None = no component.
    pub bundle: Option<Vec<Slot>>,
    /// The lines vanilla itself writes for the container (`item_count`,
    /// `more_items`): the Shulker module may replace them.
    pub vanilla_container: Vec<String>,
}

/// A tooltip line to add: text and `0xRRGGBB`.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub color: u32,
}

impl Line {
    pub fn new(text: impl Into<String>, color: u32) -> Line {
        Line {
            text: text.into(),
            color,
        }
    }
}

pub const GRAY: u32 = 0xAAAAAA;
pub const DARK_GRAY: u32 = 0x555555;
pub const DARK_AQUA: u32 = 0x00AAAA;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "tooltips",
    name: "Better Tooltips",
    category: Category::Misc,
    description: "Durability numbers, food values, shulker box contents.",
    default_on: true,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "",
    settings: &[
        SettingDef::new("durability", "Durability numbers", Kind::Bool(true)),
        SettingDef::new("food", "Hunger and saturation", Kind::Bool(true)),
        SettingDef::new("container", "Shulker box contents", Kind::Bool(true)),
        SettingDef::new(
            "container_lines",
            "Max content lines",
            Kind::Number {
                default: 8.0,
                min: 1.0,
                max: 27.0,
                step: 1.0,
            },
        ),
        SettingDef::new("item_id", "Item id", Kind::Bool(false)),
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

/// Java's `ContainerSummary.summarize`: the entries grouped by name in order
/// of first appearance (counts added), `"<sum>x <name>"` each, and after
/// `max` groups one `... and <rest> more`.
pub fn summarize(items: &[(String, i32)], max: usize) -> Vec<String> {
    let mut groups: Vec<(&str, i64)> = Vec::new();
    for (name, count) in items {
        match groups.iter_mut().find(|g| g.0 == name) {
            Some(g) => g.1 += i64::from(*count),
            None => groups.push((name, i64::from(*count))),
        }
    }
    let mut out = Vec::new();
    for (i, (name, count)) in groups.iter().enumerate() {
        if i >= max {
            out.push(format!("... and {} more", groups.len() - max));
            break;
        }
        out.push(format!("{count}x {name}"));
    }
    out
}

/// `String.format("%.1f")`: Java rounds half up, Rust's `{:.1}` half to even.
pub fn one_decimal(v: f64) -> String {
    let r = (v.abs() * 10.0).round() / 10.0;
    format!("{}{r:.1}", if v < 0.0 && r != 0.0 { "-" } else { "" })
}

/// Java's `Tooltips.appendTooltip` (spec 1.3). `skip_food` is
/// `APPLESKIN.handlesTooltips()`, `shulker_active` is the Shulker module's
/// `isActive()`.
pub fn better_lines(host: &dyn Host, s: &StackView, shulker_active: bool) -> Vec<Line> {
    let mut out = Vec::new();
    if !core::active(host, &MODULE) {
        return out;
    }
    let on = |id: &str| core::value(host, &MODULE, id).on();
    if on("durability") && s.max_damage > 0 {
        out.push(Line::new(
            format!("Durability: {} / {}", s.max_damage - s.damage, s.max_damage),
            GRAY,
        ));
    }
    if on("food")
        && !appleskin::handles_tooltips(host)
        && let Some((nutrition, saturation)) = s.food
    {
        out.push(Line::new(food_text(nutrition, saturation), GRAY));
    }
    if on("container")
        && !shulker_active
        && let Some(slots) = &s.container
    {
        let max = core::value(host, &MODULE, "container_lines").num() as usize;
        let items: Vec<(String, i32)> = slots.iter().map(|x| (x.name.clone(), x.count)).collect();
        for t in summarize(&items, max) {
            out.push(Line::new(t, DARK_AQUA));
        }
    }
    if on("item_id") {
        out.push(Line::new(s.id.clone(), DARK_GRAY));
    }
    out
}

/// `Food: <nutrition> hunger, <saturation> saturation` (Java's locale comma
/// is not copied: always a decimal point).
pub fn food_text(nutrition: i32, saturation: f32) -> String {
    format!(
        "Food: {nutrition} hunger, {} saturation",
        one_decimal(f64::from(saturation))
    )
}

/// The result for one tooltip: vanilla lines to drop (exact texts) and the
/// lines to append after the rest.
#[derive(Debug, Default, PartialEq)]
pub struct Extra {
    pub remove: Vec<String>,
    pub lines: Vec<Line>,
}

/// All lines the three modules add to one item tooltip, in Java's order
/// (spec 0.1). Each module runs in `core::guard`, like Java's callback.
pub fn append(host: &mut dyn Host, s: &StackView) -> Extra {
    let mut out = Vec::new();
    let mut remove = Vec::new();
    let shulker_active = core::active(host, &shulker::MODULE);
    core::guard(host, &MODULE, |h| {
        out.extend(better_lines(h, s, shulker_active))
    });
    core::guard(host, &shulker::MODULE, |h| {
        let (rm, lines) = shulker::lines(h, s);
        remove = rm;
        out.extend(lines);
    });
    core::guard(host, &appleskin::MODULE, |h| {
        out.extend(appleskin::lines(h, s))
    });
    Extra { remove, lines: out }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};

    fn view() -> StackView {
        StackView {
            id: "minecraft:diamond_pickaxe".into(),
            damage: 1000,
            max_damage: 1561,
            ..Default::default()
        }
    }

    #[test]
    fn summary_fixture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/tooltips.json"
        );
        let f: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        for case in f["summary"].as_array().unwrap() {
            let items: Vec<(String, i32)> = case["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    (
                        e[0].as_str().unwrap().to_owned(),
                        e[1].as_i64().unwrap() as i32,
                    )
                })
                .collect();
            let want: Vec<&str> = case["out"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            assert_eq!(
                summarize(&items, case["max"].as_u64().unwrap() as usize),
                want
            );
        }
    }

    #[test]
    fn rounding_is_half_up() {
        assert_eq!(one_decimal(12.8), "12.8");
        assert_eq!(one_decimal(0.25), "0.3");
        assert_eq!(one_decimal(0.0), "0.0");
        assert_eq!(food_text(8, 12.8), "Food: 8 hunger, 12.8 saturation");
    }

    #[test]
    fn durability_and_id() {
        restart();
        let host = Fake::default();
        let lines = better_lines(&host, &view(), false);
        assert_eq!(lines, vec![Line::new("Durability: 561 / 1561", GRAY)]);
    }

    #[test]
    fn food_line_and_container_text() {
        restart();
        let host = Fake::default();
        let s = StackView {
            id: "minecraft:cooked_beef".into(),
            food: Some((8, 12.8)),
            container: Some(vec![
                Slot {
                    name: "Diamond".into(),
                    count: 12,
                },
                Slot {
                    name: "Diamond".into(),
                    count: 4,
                },
            ]),
            ..Default::default()
        };
        // AppleSkin shows food values in tooltips, so its line replaces ours.
        let lines = better_lines(&host, &s, false);
        assert_eq!(lines, vec![Line::new("16x Diamond", DARK_AQUA)]);
        // The Shulker module being active hides the container text.
        assert!(better_lines(&host, &s, true).is_empty());
        let mut host = host;
        core::set_enabled(&mut host, &appleskin::MODULE, false);
        let lines = better_lines(&host, &s, true);
        assert_eq!(
            lines,
            vec![Line::new("Food: 8 hunger, 12.8 saturation", GRAY)]
        );
    }
}
