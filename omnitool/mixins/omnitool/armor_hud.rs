// Armor HUD (docs/spec/omnitool/hud.md, section 1): armor and held items
// with their durability numbers. Horizon has no hook to draw an item icon, so
// the icon's cell stays empty (TODO: needs a mixin); worn items and the
// offhand slot's position are not readable either (`Host::worn` is a stub).
// Added to the game as `game::omnitool_armor_hud`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_hud::{Canvas, HudBox, HudCorner, durability_color};
use super::omnitool_setting::{Kind, SettingDef};

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "armor_hud",
    name: "Armor HUD",
    category: Category::Hud,
    description: "Armor and held items with durability.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "Ukus Armor HUD",
    settings: &[
        SettingDef::new(
            "layout",
            "Layout",
            Kind::Enum {
                values: &["VERTICAL", "HORIZONTAL"],
                default: 0,
            },
        ),
        SettingDef::new("numbers", "Durability numbers", Kind::Bool(true)),
        SettingDef::new("hands", "Show held items", Kind::Bool(true)),
        SettingDef::new(
            "next_to_hotbar",
            "Next to the offhand slot (like Ukus Armor HUD)",
            Kind::Bool(true),
        ),
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

fn hud(host: &mut dyn Host) -> Option<HudBox> {
    let numbers = core::value(host, &MODULE, "numbers").on();
    let hands = core::value(host, &MODULE, "hands").on();
    let anchored = core::value(host, &MODULE, "next_to_hotbar").on();
    let vertical = !anchored && core::value(host, &MODULE, "layout").index() == 0;
    let worn = host.worn();
    let mut c = Canvas::new(host);
    // Anchored: a row left of the hotbar, numbers above the icons.
    let y0 = if anchored && numbers { 9.0 } else { 0.0 };
    let (mut cx, mut cy) = (0.0f32, y0);
    let (mut w, mut h) = (0.0f32, 0.0f32);
    for (i, stack) in worn.hud_order().into_iter().enumerate() {
        if i >= 4 && (anchored || !hands) {
            continue;
        }
        let Some(s) = stack else { continue };
        let mut cell_w = 18.0;
        if numbers && s.damageable() {
            let text = s.left().to_string();
            let color = durability_color(s.damage, s.max_damage) & 0x00FF_FFFF;
            let tw = c.width(&text);
            if vertical {
                c.text(&text, cx + 18.0, cy + 4.0, color, true);
                cell_w = 18.0 + tw + 2.0;
            } else {
                let ty = if anchored { cy - 9.0 } else { cy + 17.0 };
                c.text(&text, cx + 9.0 - tw / 2.0, ty, color, true);
            }
        }
        if vertical {
            cy += 17.0;
            w = w.max(cell_w);
            h = cy - y0;
        } else {
            cx += 20.0;
            w = cx;
            h = if numbers { 27.0 } else { 17.0 };
        }
    }
    if w == 0.0 {
        return None;
    }
    if anchored {
        h = 17.0 + y0;
    }
    Some(HudBox {
        corner: HudCorner::BottomLeft,
        w: w.max(16.0),
        h: h.max(16.0),
        ops: c.ops,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::Fake;
    use crate::omnitool_hud::{HudOp, Stack};

    fn stack(damage: i32) -> Option<Stack> {
        Some(Stack {
            name: "Pick".into(),
            max_damage: 100,
            damage,
        })
    }

    #[test]
    fn nothing_worn_draws_nothing() {
        assert!(hud(&mut Fake::default()).is_none());
    }

    #[test]
    fn vertical_numbers_colored() {
        crate::omnitool_core::tests::restart();
        let mut h = Fake::default();
        core::set_enabled(&mut h, &MODULE, true);
        // Default is anchored (next to hotbar): a horizontal row with numbers above.
        h.worn.head = stack(10);
        h.worn.chest = stack(90);
        let b = hud(&mut h).unwrap();
        assert_eq!(b.w, 40.0);
        assert_eq!(b.h, 26.0);
        let texts: Vec<_> = b
            .ops
            .iter()
            .map(|o| match o {
                HudOp::Text { text, rgb, .. } => (text.clone(), *rgb),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(texts, [("90".into(), 0x60FF60), ("10".into(), 0xFF4040)]);
    }
}
