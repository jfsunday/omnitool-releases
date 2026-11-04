// Low Fire (docs/spec/omnitool/render-tweaks.md, section 4): when to hide the
// fire overlay, how far to lower and shrink it, and the opacity maths.
// Mixin needed to apply it: Horizon draws the first-person fire in
// horizon-client `render_tick.rs` (the `on_fire` expression, ~line 261) and
// horizon-client-render `screen_effects.rs` (~line 190, the fire quad): skip
// the draw when `hide` holds, otherwise translate by `offset_y`, scale by
// `scale_factor` and multiply the alpha as `alpha`/`argb` say. Added to the
// game as `game::omnitool_low_fire`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, NONE, SettingDef};

const fn percent(
    id: &'static str,
    name: &'static str,
    default: f64,
    min: f64,
    max: f64,
) -> SettingDef {
    SettingDef::new(
        id,
        name,
        Kind::Number {
            default,
            min,
            max,
            step: 1.0,
        },
    )
}

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "low_fire",
    name: "Low Fire",
    category: Category::Render,
    description: "Lowers and shrinks the fire overlay when you burn, or hides it.",
    default_on: false,
    cheaty: false,
    beta: true,
    fair_variant: false,
    replaces: "Low Fire, Cleanview",
    settings: &[
        percent("offset", "Lower by (% of the screen)", 30.0, 0.0, 50.0),
        percent("scale", "Size (%)", 100.0, 20.0, 100.0),
        percent("opacity", "Opacity (%)", 90.0, 0.0, 100.0),
        SettingDef::new(
            "fire_resistance",
            "Hide with Fire Resistance",
            Kind::Bool(true),
        ),
        SettingDef::new("creative", "Hide in Creative", Kind::Bool(true)),
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

/// What `hide` needs to know about the player.
#[derive(Clone, Copy, Debug, Default)]
pub struct Burning {
    pub has_player: bool,
    pub fire_resistance: bool,
    pub creative: bool,
}

fn num(host: &dyn Host, id: &str) -> f64 {
    core::value(host, &MODULE, id).num()
}

/// Java's `hide()`.
#[allow(dead_code)] // needs the fire-overlay injection
pub fn hide(host: &dyn Host, p: Burning) -> bool {
    if !core::active(host, &MODULE) || !p.has_player {
        return false;
    }
    if num(host, "opacity") <= 0.0 {
        return true;
    }
    if core::value(host, &MODULE, "fire_resistance").on() && p.fire_resistance {
        return true;
    }
    core::value(host, &MODULE, "creative").on() && p.creative
}

/// Java's `push`: the translation in clip space (the screen is 2 high, so
/// `offset` % of it is `offset / 50`), applied before the scale.
#[allow(dead_code)] // needs the fire-overlay injection
pub fn offset_y(host: &dyn Host) -> f32 {
    -(num(host, "offset") / 50.0) as f32
}

/// The x/y scale of `push`.
#[allow(dead_code)] // needs the fire-overlay injection
pub fn scale_factor(host: &dyn Host) -> f32 {
    (num(host, "scale") / 100.0) as f32
}

/// Vanilla's fire alpha.
#[allow(dead_code)]
pub const VANILLA_ALPHA: f32 = 0.9;

/// Java's float alpha (1.21 - 26.1): not clamped.
#[allow(dead_code)] // needs the fire-overlay injection
pub fn alpha(host: &dyn Host, v: f32) -> f32 {
    if core::active(host, &MODULE) {
        v * (num(host, "opacity") / 90.0) as f32
    } else {
        v
    }
}

/// Java's ARGB variant (26.2+): the alpha byte scaled and rounded
/// (`floor(x + 0.5)`), at most 255.
pub fn argb_for(opacity: f64, c: u32) -> u32 {
    let a = f64::from(c >> 24) * (opacity / 90.0);
    let a = (a.min(255.0) + 0.5).floor() as u32;
    (a << 24) | (c & 0x00FF_FFFF)
}

#[allow(dead_code)] // needs the fire-overlay injection
pub fn argb(host: &dyn Host, c: u32) -> u32 {
    if core::active(host, &MODULE) {
        argb_for(num(host, "opacity"), c)
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};

    fn set(h: &mut Fake, id: &str, v: &str) {
        core::with(h, |s, _| s.set_setting(&MODULE, id, v)).unwrap();
    }

    #[test]
    fn fixture_cases() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/render-tweaks.json"
        );
        let f: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for c in f["low_fire_alpha"].as_array().unwrap() {
            let op = c["opacity"].as_f64().unwrap();
            let a = c["alpha_in"].as_f64().unwrap() as f32 * (op / 90.0) as f32;
            assert_eq!(a, c["alpha_out"].as_f64().unwrap() as f32, "{c}");
            let out = argb_for(op, c["argb_in"].as_u64().unwrap() as u32);
            assert_eq!(u64::from(out), c["argb_out"].as_u64().unwrap(), "{c}");
        }
    }

    #[test]
    fn hiding_rules() {
        restart();
        let mut h = Fake::default();
        assert_eq!(MODULE.settings.len(), 5);
        let p = Burning {
            has_player: true,
            ..Burning::default()
        };
        assert!(!hide(&h, p), "inactive");
        core::set_enabled(&mut h, &MODULE, true);
        assert!(!hide(&h, p));
        assert!(hide(
            &h,
            Burning {
                fire_resistance: true,
                ..p
            }
        ));
        assert!(hide(
            &h,
            Burning {
                creative: true,
                ..p
            }
        ));
        assert!(!hide(&h, Burning::default()), "no player");
        set(&mut h, "creative", "false");
        assert!(!hide(
            &h,
            Burning {
                creative: true,
                ..p
            }
        ));
        set(&mut h, "opacity", "0");
        assert!(hide(&h, p));
    }

    #[test]
    fn transform_and_alpha() {
        restart();
        let mut h = Fake::default();
        assert_eq!(alpha(&h, 0.9), 0.9);
        assert_eq!(argb(&h, 0xE5FF_FFFF), 0xE5FF_FFFF);
        core::set_enabled(&mut h, &MODULE, true);
        assert!((offset_y(&h) + 0.6).abs() < 1e-6);
        assert_eq!(scale_factor(&h), 1.0);
        assert_eq!(alpha(&h, VANILLA_ALPHA), 0.9);
        set(&mut h, "opacity", "45");
        assert_eq!(argb(&h, 0xE5FF_FFFF) >> 24, 0x73);
    }
}
