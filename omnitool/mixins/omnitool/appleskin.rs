// AppleSkin (docs/spec/omnitool/tooltips.md, section 3): the tooltip part.
// The hunger/saturation icon row (3.3) is an image line, which Horizon's
// `stack_tooltip_lines` cannot carry, so a text line with the same numbers
// stands in. The HUD overlays (3.4) are another task. Added to the game as
// `game::omnitool_appleskin`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, NONE, SettingDef};
use super::omnitool_tooltips::{GRAY, Line, StackView, food_text};

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "appleskin",
    name: "AppleSkin",
    category: Category::Hud,
    description: "Food values in tooltips, saturation and exhaustion on the hunger bar, preview of what held food restores.",
    default_on: true,
    cheaty: false,
    beta: true,
    fair_variant: false,
    replaces: "AppleSkin",
    settings: &[
        SettingDef::new(
            "tooltip_shift",
            "Food values in tooltip (with Shift)",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "tooltip_always",
            "Food values in tooltip (always)",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "saturation_overlay",
            "Saturation on hunger bar",
            Kind::Bool(true),
        ),
        SettingDef::new("food_overlay", "Held food on hunger bar", Kind::Bool(true)),
        SettingDef::new(
            "food_overlay_offhand",
            "Also food in off hand",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "exhaustion_underlay",
            "Exhaustion behind hunger bar",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "health_overlay",
            "Estimated health on hearts",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "vanilla_animations",
            "Shake icons like vanilla",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "flash_alpha",
            "Max flash opacity",
            Kind::Number {
                default: 0.65,
                min: 0.0,
                max: 1.0,
                step: 0.05,
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

/// Java's `handlesTooltips`: the module is active and shows tooltips, so
/// Better Tooltips leaves its own food line out.
pub fn handles_tooltips(host: &dyn Host) -> bool {
    core::active(host, &MODULE)
        && (core::value(host, &MODULE, "tooltip_shift").on()
            || core::value(host, &MODULE, "tooltip_always").on())
}

/// The shape of the icon row (`FoodTooltip`): `(hunger bars, hunger text,
/// saturation bars, saturation text)`.
pub fn food_tooltip(hunger: i32, saturation: f32) -> (i32, Option<String>, i32, Option<String>) {
    let sign = |v: f32| if v < 0.0 { -1 } else { 1 };
    let mut hunger_bars = (hunger.abs() as f32 / 2.0).ceil() as i32;
    let mut hunger_text = None;
    if hunger_bars > 10 {
        hunger_text = Some(format!(
            "x{}",
            (if hunger < 0 { -1 } else { 1 }) * hunger_bars
        ));
        hunger_bars = 1;
    }
    let mut sat_bars = (saturation.abs() / 2.0).ceil() as i32;
    let mut sat_text = None;
    if sat_bars > 10 || sat_bars == 0 {
        sat_text = Some(format!("x{}", sign(saturation) * sat_bars));
        sat_bars = 1;
    }
    (hunger_bars, hunger_text, sat_bars, sat_text)
}

/// The sprite stage (0-4) of the saturation icon `i` for `|saturation|`.
#[allow(dead_code)] // needs the icon row
pub fn sat_stage(abs: f32, i: i32) -> i32 {
    let bar = (abs - i as f32) / 2.0;
    if bar >= 1.0 {
        3
    } else if bar > 0.5 {
        2
    } else if bar > 0.25 {
        1
    } else if bar > 0.0 {
        0
    } else {
        4
    }
}

/// Java's `appendTooltip` (spec 3.3) as one text line.
pub fn lines(host: &dyn Host, s: &StackView) -> Vec<Line> {
    if !core::active(host, &MODULE) {
        return Vec::new();
    }
    let show = core::value(host, &MODULE, "tooltip_always").on()
        || (core::value(host, &MODULE, "tooltip_shift").on() && host.shift_down());
    let Some((nutrition, saturation)) = s.food.filter(|_| show && s.consumable) else {
        return Vec::new();
    };
    // `hungerBars <= 0` (nutrition 0): no row.
    if food_tooltip(nutrition, saturation).0 <= 0 {
        return Vec::new();
    }
    vec![Line::new(food_text(nutrition, saturation), GRAY)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};

    #[test]
    fn fixture_cases() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/tooltips.json"
        );
        let f: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        for c in f["food_tooltip"].as_array().unwrap() {
            let got = food_tooltip(c[0].as_i64().unwrap() as i32, c[1].as_f64().unwrap() as f32);
            let want = (
                c[2].as_i64().unwrap() as i32,
                c[3].as_str().map(str::to_owned),
                c[4].as_i64().unwrap() as i32,
                c[5].as_str().map(str::to_owned),
            );
            assert_eq!(got, want, "{c}");
        }
        for c in f["sat_stage"].as_array().unwrap() {
            let got = sat_stage(c[0].as_f64().unwrap() as f32, c[1].as_i64().unwrap() as i32);
            assert_eq!(got as i64, c[2].as_i64().unwrap(), "{c}");
        }
    }

    fn steak() -> StackView {
        StackView {
            id: "minecraft:cooked_beef".into(),
            food: Some((8, 12.8)),
            consumable: true,
            ..Default::default()
        }
    }

    #[test]
    fn replaces_the_food_line() {
        restart();
        let host = Fake::default();
        assert_eq!(
            lines(&host, &steak()),
            vec![Line::new("Food: 8 hunger, 12.8 saturation", GRAY)]
        );
        // Better Tooltips leaves its own line out, so the text shows once.
        let all = crate::omnitool_tooltips::better_lines(&host, &steak(), false);
        assert!(all.is_empty());
        // Without CONSUMABLE (or with 0 nutrition) AppleSkin adds nothing.
        let mut plain = steak();
        plain.consumable = false;
        assert!(lines(&host, &plain).is_empty());
        plain.consumable = true;
        plain.food = Some((0, 1.0));
        assert!(lines(&host, &plain).is_empty());
    }
}
