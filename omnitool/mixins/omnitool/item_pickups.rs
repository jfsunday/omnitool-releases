// Item Pickups (docs/spec/omnitool/hud.md, section 6): "+12 Oak Log" lines for
// gains in the inventory. `Host::inventory` has no hook yet (stub, empty), so
// in the game nothing is counted until Horizon exposes the inventory. No item
// icons (no hook to draw items); the text fades by dimming its colour towards
// black since `native_text` takes no alpha. Added to the game as
// `game::omnitool_item_pickups`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_hud::{Canvas, HudBox, HudCorner, PickupFeed, alpha};
use super::omnitool_setting::{Kind, SettingDef};
use std::cell::RefCell;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "item_pickups",
    name: "Item Pickups",
    category: Category::Hud,
    description: "Shows picked up items for a few seconds.",
    default_on: false,
    cheaty: false,
    beta: true,
    fair_variant: false,
    replaces: "",
    settings: &[
        SettingDef::new(
            "seconds",
            "Show for seconds",
            Kind::Number {
                default: 3.0,
                min: 1.0,
                max: 15.0,
                step: 0.5,
            },
        ),
        SettingDef::new(
            "max",
            "Max lines",
            Kind::Number {
                default: 6.0,
                min: 1.0,
                max: 20.0,
                step: 1.0,
            },
        ),
        SettingDef::new("total", "Show total in inventory", Kind::Bool(true)),
        SettingDef::new("background", "Background", Kind::Bool(false)),
    ],
    command: None,
    key: super::omnitool_setting::NONE,
    hold: false,
    hud: Some(hud),
    tick: Some(tick),
    on_key: None,
    hud_right: None,
    on_action: None,
};

/// Java's fade length.
const FADE_MS: i64 = 500;

thread_local! {
    static FEED: RefCell<PickupFeed> = RefCell::new(PickupFeed::default());
}

/// Counts per item over all slots, in first-seen order.
pub fn totals(slots: &[(String, u32)]) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = Vec::new();
    for (k, n) in slots {
        match out.iter_mut().find(|o| &o.0 == k) {
            Some(o) => o.1 += n,
            None => out.push((k.clone(), *n)),
        }
    }
    out
}

fn tick(host: &mut dyn Host) {
    if !core::active(host, &MODULE) {
        FEED.with(|f| f.borrow_mut().reset());
        return;
    }
    let inv = host.inventory();
    let max = core::value(host, &MODULE, "max").num() as usize;
    let now = host.now_ms();
    FEED.with(|f| {
        let mut f = f.borrow_mut();
        if inv.screen_open || inv.creative {
            f.reset();
        } else {
            f.update(&totals(&inv.slots), now, max);
        }
    });
}

/// `colour` dimmed to `alpha` (0..1) against a dark background.
fn dim(rgb: u32, alpha: f32) -> u32 {
    let a = alpha.clamp(0.0, 1.0);
    let ch = |shift: u32| (((rgb >> shift) & 0xFF) as f32 * a) as u32;
    ch(16) << 16 | ch(8) << 8 | ch(0)
}

fn hud(host: &mut dyn Host) -> Option<HudBox> {
    let show = (core::value(host, &MODULE, "seconds").num() * 1000.0) as u64;
    let total = core::value(host, &MODULE, "total").on();
    let background = core::value(host, &MODULE, "background").on();
    let now = host.now_ms();
    let list = FEED.with(|f| f.borrow_mut().visible(now, show).to_vec());
    if list.is_empty() {
        return None;
    }
    let inv = host.inventory();
    let have = totals(&inv.slots);
    let mut c = Canvas::new(host);
    let (mut cy, mut width) = (0.0f32, 60.0f32);
    for e in &list {
        let mut txt = format!("+{} {}", e.amount, e.key);
        if total {
            let n = have.iter().find(|h| h.0 == e.key).map_or(0, |h| h.1);
            txt.push_str(&format!(" §7({n})"));
        }
        let a = alpha((now - e.time) as i64, show as i64, FADE_MS);
        let rw = 4.0 + c.width(&txt) + 2.0;
        if background {
            c.fill(0.0, cy, rw, cy + 17.0, ((a * 0x90 as f32) as u32) << 24);
        }
        c.text(
            &txt,
            20.0,
            cy + 5.0,
            dim(0xFFFFFF, a.max(8.0 / 255.0)),
            true,
        );
        width = width.max(rw + 16.0);
        cy += 18.0;
    }
    Some(HudBox {
        corner: HudCorner::BottomRight,
        w: width,
        h: cy,
        ops: c.ops,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};
    use crate::omnitool_hud::{HudOp, Inventory};

    fn inv(slots: &[(&str, u32)], screen: bool) -> Inventory {
        Inventory {
            slots: slots.iter().map(|(k, n)| (k.to_string(), *n)).collect(),
            screen_open: screen,
            creative: false,
        }
    }

    fn host() -> Fake {
        restart();
        FEED.with(|f| *f.borrow_mut() = PickupFeed::default());
        let mut h = Fake::default();
        core::set_enabled(&mut h, &MODULE, true);
        h
    }

    #[test]
    fn totals_merge_stacks() {
        let slots = [
            ("a".to_string(), 3),
            ("b".to_string(), 1),
            ("a".to_string(), 4),
        ];
        assert_eq!(totals(&slots), [("a".to_string(), 7), ("b".to_string(), 1)]);
    }

    #[test]
    fn gain_shows_with_total_and_fades_out() {
        let mut h = host();
        h.inventory = inv(&[("Oak Log", 5)], false);
        tick(&mut h);
        h.inventory = inv(&[("Oak Log", 5), ("Oak Log", 7)], false);
        h.now = 1000;
        tick(&mut h);
        let b = hud(&mut h).unwrap();
        let text = b.ops.iter().find_map(|o| match o {
            HudOp::Text { text, .. } => Some(text.clone()),
            _ => None,
        });
        assert_eq!(text.unwrap(), "+7 Oak Log §7(12)");
        h.now = 1000 + 3000;
        assert!(hud(&mut h).is_none());
    }

    #[test]
    fn open_screen_resets_baseline() {
        let mut h = host();
        h.inventory = inv(&[("Dirt", 1)], false);
        tick(&mut h);
        h.inventory = inv(&[("Dirt", 9)], true);
        tick(&mut h);
        h.inventory = inv(&[("Dirt", 9)], false);
        tick(&mut h);
        assert!(hud(&mut h).is_none());
    }

    #[test]
    fn dimming() {
        assert_eq!(dim(0xFFFFFF, 0.5), 0x7F7F7F);
        assert_eq!(dim(0xFFFFFF, 1.0), 0xFFFFFF);
    }
}
