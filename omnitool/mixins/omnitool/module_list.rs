// Module List (docs/spec/omnitool/hud.md, section 2): the active modules,
// longest name first. Horizon places the box in the top right corner, so the
// lines are always right aligned. Added to the game as
// `game::omnitool_module_list`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_hud::{Canvas, HudBox, HudCorner, LINE};
use super::omnitool_registry::MODULES;
use super::omnitool_setting::{Kind, SettingDef};

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "module_list",
    name: "Module List",
    category: Category::Hud,
    description: "Shows which modules are active.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "",
    settings: &[
        SettingDef::new("color", "Text color", Kind::Color(0xFF60D0FFu32 as i32)),
        SettingDef::new("rainbow", "Rainbow", Kind::Bool(false)),
        SettingDef::new("background", "Background", Kind::Bool(true)),
    ],
    command: None,
    key: super::omnitool_setting::NONE,
    hold: false,
    hud: Some(hud),
    tick: None,
    on_key: None,
    hud_right: None,
    on_action: None,
};

/// `java.awt.Color.HSBtoRGB` (RGB without alpha).
pub fn hsb_to_rgb(hue: f32, saturation: f32, brightness: f32) -> u32 {
    let h = (hue - hue.floor()) * 6.0;
    let f = h - h.floor();
    let p = brightness * (1.0 - saturation);
    let q = brightness * (1.0 - saturation * f);
    let t = brightness * (1.0 - saturation * (1.0 - f));
    let (r, g, b) = match h as i32 {
        0 => (brightness, t, p),
        1 => (q, brightness, p),
        2 => (p, brightness, t),
        3 => (p, q, brightness),
        4 => (t, p, brightness),
        _ => (brightness, p, q),
    };
    let c = |v: f32| (v * 255.0 + 0.5) as u32;
    c(r) << 16 | c(g) << 8 | c(b)
}

/// The rainbow colour of line `i` at `now_ms`.
pub fn rainbow(now_ms: u64, i: usize) -> u32 {
    let deg = (now_ms / 20 + i as u64 * 15) % 360;
    hsb_to_rgb(deg as f32 / 360.0, 0.7, 1.0)
}

fn hud(host: &mut dyn Host) -> Option<HudBox> {
    let color = core::value(host, &MODULE, "color");
    let color = match color {
        super::omnitool_setting::Value::Color(c) => c as u32 & 0x00FF_FFFF,
        _ => 0x60D0FF,
    };
    let rainbow_on = core::value(host, &MODULE, "rainbow").on();
    let background = core::value(host, &MODULE, "background").on();
    let now = host.now_ms();
    let mut names: Vec<&str> = MODULES
        .iter()
        .filter(|m| m.id != MODULE.id && core::active(host, m))
        .map(|m| m.name)
        .collect();
    if names.is_empty() {
        return None;
    }
    let mut c = Canvas::new(host);
    // Stable: ties stay in registration order.
    names.sort_by(|a, b| c.width(b).total_cmp(&c.width(a)));
    let lh = LINE + 1.0;
    let max = names.iter().map(|n| c.width(n)).fold(0.0, f32::max);
    let width = max + 4.0;
    let height = names.len() as f32 * lh + 2.0;
    for (i, name) in names.iter().enumerate() {
        let ty = 1.0 + i as f32 * lh;
        let tw = c.width(name);
        let tx = width - tw - 2.0;
        if background {
            c.fill(
                tx - 2.0,
                ty - 1.0,
                tx + tw + 2.0,
                ty + lh - 1.0,
                0x9000_0000,
            );
        }
        let rgb = if rainbow_on { rainbow(now, i) } else { color };
        c.text(name, tx, ty, rgb, true);
    }
    Some(HudBox {
        corner: HudCorner::TopRight,
        w: width,
        h: height,
        ops: c.ops,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};
    use crate::omnitool_hud::HudOp;

    #[test]
    fn hsb_primaries() {
        assert_eq!(hsb_to_rgb(0.0, 1.0, 1.0), 0xFF0000);
        assert_eq!(hsb_to_rgb(1.0 / 3.0, 1.0, 1.0), 0x00FF00);
        assert_eq!(hsb_to_rgb(0.0, 0.7, 1.0), 0xFF4D4D);
    }

    #[test]
    fn lists_active_modules_without_itself() {
        restart();
        let mut h = Fake::default();
        let calc = &super::super::omnitool_calc::MODULE;
        core::set_enabled(&mut h, calc, false);
        // on by default, and listed like any module
        core::set_enabled(&mut h, &super::super::omnitool_omni_overlay::MODULE, false);
        core::set_enabled(&mut h, &super::super::omnitool_death_coords::MODULE, false);
        core::set_enabled(
            &mut h,
            &super::super::omnitool_quick_disconnect::MODULE,
            false,
        );
        core::set_enabled(&mut h, &super::super::omnitool_chat_tweaks::MODULE, false);
        core::set_enabled(&mut h, &super::super::omnitool_chat_log::MODULE, false);
        core::set_enabled(&mut h, &super::super::omnitool_session_stats::MODULE, false);
        core::set_enabled(&mut h, &super::super::omnitool_tps::MODULE, false);
        core::set_enabled(&mut h, &super::super::omnitool_mouse_tweaks::MODULE, false);
        core::set_enabled(&mut h, &super::super::omnitool_tooltips::MODULE, false);
        core::set_enabled(
            &mut h,
            &super::super::omnitool_shulker_tooltip::MODULE,
            false,
        );
        core::set_enabled(&mut h, &super::super::omnitool_appleskin::MODULE, false);
        assert!(hud(&mut h).is_none());
        core::set_enabled(&mut h, &MODULE, true);
        core::set_enabled(&mut h, calc, true);
        let b = hud(&mut h).unwrap();
        assert_eq!(b.corner, HudCorner::TopRight);
        let texts: Vec<_> = b
            .ops
            .iter()
            .filter_map(|o| match o {
                HudOp::Text { text, rgb, .. } => Some((text.as_str(), *rgb)),
                _ => None,
            })
            .collect();
        assert_eq!(texts, [("Calculator", 0x60D0FF)]);
        assert_eq!(b.w, 10.0 * 6.0 + 4.0);
        assert_eq!(b.h, 12.0);
    }
}
