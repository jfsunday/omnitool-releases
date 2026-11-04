// Custom Crosshair (docs/spec/omnitool/render-tweaks.md, section 7): the
// shapes as rectangle lists, the colour choice and the attack bar, as pure
// functions checked against tests/fixtures/render-tweaks.json. Drawing needs a
// Mixin: Horizon draws the crosshair in horizon-client-render `hud.rs` (the
// "HUD crosshair" pass, ~line 1525; game.rs ~6432 decides when) and
// `native_hud` can only add boxes after it, so Vanilla's crosshair cannot be
// replaced without an injection that skips that pass when `wanted` holds.
// Added to the game as `game::omnitool_custom_crosshair`; also a module of
// this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, NONE, SettingDef};

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "custom_crosshair",
    name: "Custom Crosshair",
    category: Category::Render,
    description: "Your own crosshair: shape, size, gap, color and outline.",
    default_on: false,
    cheaty: false,
    beta: true,
    fair_variant: false,
    replaces: "Custom Crosshair Mod, Dynamic Crosshair",
    settings: &[
        SettingDef::new(
            "style",
            "Style",
            Kind::Enum {
                values: &["VANILLA", "CROSS", "DOT", "CIRCLE", "CROSS_CIRCLE"],
                default: 1,
            },
        ),
        SettingDef::new(
            "size",
            "Size",
            Kind::Number {
                default: 5.0,
                min: 1.0,
                max: 20.0,
                step: 1.0,
            },
        ),
        SettingDef::new(
            "gap",
            "Gap",
            Kind::Number {
                default: 2.0,
                min: 0.0,
                max: 10.0,
                step: 1.0,
            },
        ),
        SettingDef::new(
            "thickness",
            "Thickness",
            Kind::Number {
                default: 1.0,
                min: 1.0,
                max: 5.0,
                step: 1.0,
            },
        ),
        SettingDef::new("center_dot", "Center dot", Kind::Bool(false)),
        SettingDef::new("color", "Color", Kind::Color(0xFFFF_FFFFu32 as i32)),
        SettingDef::new("outline", "Outline", Kind::Bool(true)),
        SettingDef::new(
            "outline_color",
            "Outline color",
            Kind::Color(0xC000_0000u32 as i32),
        ),
        SettingDef::new(
            "dynamic",
            "Change color when aiming at an entity",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "entity_color",
            "Color on entities",
            Kind::Color(0xFFFF_4040u32 as i32),
        ),
        SettingDef::new(
            "attack_indicator",
            "Attack cooldown bar (if set to Crosshair in vanilla)",
            Kind::Bool(true),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Vanilla,
    Cross,
    Dot,
    Circle,
    CrossCircle,
}

impl Style {
    pub fn from_index(i: usize) -> Style {
        match i {
            0 => Style::Vanilla,
            2 => Style::Dot,
            3 => Style::Circle,
            4 => Style::CrossCircle,
            _ => Style::Cross,
        }
    }
}

/// A rectangle `[x1, y1, x2, y2]`, x2/y2 exclusive.
#[allow(dead_code)]
pub type Rect = [i32; 4];

/// Java's `shape`: the rectangles around the centre (0, 0), in the order the
/// fixtures list them (cross bars, ring or disc, centre dot).
#[allow(dead_code)]
pub fn shape(style: Style, size: i32, gap: i32, thickness: i32, center_dot: bool) -> Vec<Rect> {
    let (s, g, t) = (size, gap, thickness);
    // Java's integer division rounds toward zero.
    let lo = -((t - 1) / 2);
    let hi = lo + t;
    let mut out = Vec::new();
    if matches!(style, Style::Cross | Style::CrossCircle) {
        out.push([-g - s, lo, -g, hi]);
        out.push([1 + g, lo, 1 + g + s, hi]);
        out.push([lo, -g - s, hi, -g]);
        out.push([lo, 1 + g, hi, 1 + g + s]);
    }
    if matches!(style, Style::Circle | Style::CrossCircle) {
        let r = g + s + if style == Style::CrossCircle { 2 } else { 0 };
        let ro = f64::from(r) + 0.5;
        let ri = (f64::from(r) + 0.5 - f64::from(t)).max(0.0);
        for dy in -r..=r {
            let d = f64::from(dy);
            let ho = (ro * ro - d * d).max(0.0).sqrt().floor() as i32;
            let inner = if d.abs() < ri {
                (ri * ri - d * d).sqrt().ceil() as i32
            } else {
                -1
            };
            if inner < 0 || inner > ho {
                out.push([-ho, dy, ho + 1, dy + 1]);
            } else if ho - inner + 1 > 0 {
                out.push([-ho, dy, -inner + 1, dy + 1]);
                out.push([inner, dy, ho + 1, dy + 1]);
            }
        }
    }
    if style == Style::Dot {
        let r = (s / 2).max(1) + t - 1;
        if r <= 1 {
            out.push([-r, -r, r + 1, r + 1]);
        } else {
            for dy in -r..=r {
                let d = f64::from(dy);
                let rr = f64::from(r) + 0.5;
                let h = (rr * rr - d * d).max(0.0).sqrt().floor() as i32;
                out.push([-h, dy, h + 1, dy + 1]);
            }
        }
    }
    if style != Style::Dot && style != Style::Vanilla && center_dot {
        out.push([lo, lo, hi, hi]);
    }
    out
}

/// The crosshair's centre pixel for a screen size (Vanilla's 15x15 sprite).
#[allow(dead_code)]
pub fn center(width: i32, height: i32) -> (i32, i32) {
    ((width - 15) / 2 + 7, (height - 15) / 2 + 7)
}

/// Step 7: the colour, the entity colour while aiming at an entity.
#[allow(dead_code)]
pub fn color_for(dynamic: bool, on_entity: bool, color: i32, entity_color: i32) -> i32 {
    if dynamic && on_entity {
        entity_color
    } else {
        color
    }
}

/// Step 10: the attack bar for a cooldown scale `f` in 0..1 around the
/// centre: the background rectangle and the filled part, None at `f >= 1`.
#[allow(dead_code)]
pub fn attack_bar(cx: i32, cy: i32, gap: i32, size: i32, f: f32) -> Option<(Rect, Rect)> {
    if f >= 1.0 {
        return None;
    }
    let y = cy + gap + size + 4;
    let fill = (16.0 * f64::from(f) + 0.5).floor() as i32;
    Some((
        [cx - 8, y, cx + 8, y + 2],
        [cx - 8, y, cx - 8 + fill, y + 2],
    ))
}

/// Whether the module replaces Vanilla's crosshair (steps 1 and 2).
#[allow(dead_code)] // needs the crosshair injection
pub fn wanted(host: &dyn Host) -> bool {
    core::active(host, &MODULE)
        && host.player().is_some()
        && Style::from_index(core::value(host, &MODULE, "style").index()) != Style::Vanilla
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};

    #[test]
    fn shape_fixtures() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/render-tweaks.json"
        );
        let f: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for c in f["crosshair_shape"].as_array().unwrap() {
            let style = match c["style"].as_str().unwrap() {
                "DOT" => Style::Dot,
                "CIRCLE" => Style::Circle,
                "CROSS_CIRCLE" => Style::CrossCircle,
                _ => Style::Cross,
            };
            let int = |k: &str| c[k].as_i64().unwrap() as i32;
            let got = shape(
                style,
                int("size"),
                int("gap"),
                int("thickness"),
                c["center_dot"].as_bool().unwrap(),
            );
            let want: Vec<Rect> = c["rects"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    let r = r.as_array().unwrap();
                    [0, 1, 2, 3].map(|i| r[i].as_i64().unwrap() as i32)
                })
                .collect();
            assert_eq!(got, want, "{c}");
        }
    }

    #[test]
    fn centre_colour_and_bar() {
        assert_eq!(center(854, 480), (426, 239));
        assert_eq!(color_for(true, true, 1, 2), 2);
        assert_eq!(color_for(false, true, 1, 2), 1);
        assert_eq!(color_for(true, false, 1, 2), 1);
        assert_eq!(attack_bar(0, 0, 2, 5, 1.0), None);
        let (bg, fg) = attack_bar(0, 0, 2, 5, 0.5).unwrap();
        assert_eq!(bg, [-8, 11, 8, 13]);
        assert_eq!(fg, [-8, 11, 0, 13]);
    }

    #[test]
    fn defaults_and_wanted() {
        restart();
        let mut h = Fake::default();
        assert_eq!(MODULE.settings.len(), 11);
        assert!(!wanted(&h));
        core::set_enabled(&mut h, &MODULE, true);
        assert!(!wanted(&h), "no player");
    }
}
