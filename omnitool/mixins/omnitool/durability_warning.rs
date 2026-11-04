// Durability Warning (docs/spec/omnitool/hud.md, section 3): the warning
// lines, the action bar message and the sound. The mining lock
// (`prevent_break`, `prevent_at`) needs an injection into the game's
// block-breaking code: the settings exist and `should_block_mining` holds the
// rule, but nothing calls it yet. Added to the game as
// `game::omnitool_durability_warning`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_hud::{Canvas, HudBox, HudCorner, Stack};
use super::omnitool_setting::{Kind, SettingDef};
use std::cell::RefCell;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "durability_warning",
    name: "Durability Warning",
    category: Category::Hud,
    description: "Warns when tools/armor are almost broken and can stop you from breaking your tool.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "",
    settings: &[
        SettingDef::new(
            "percent",
            "Warn below %",
            Kind::Number {
                default: 10.0,
                min: 1.0,
                max: 50.0,
                step: 1.0,
            },
        ),
        SettingDef::new("sound", "Warning sound", Kind::Bool(true)),
        SettingDef::new(
            "prevent_break",
            "Stop mining when tool is about to break",
            Kind::Bool(false),
        ),
        SettingDef::new(
            "prevent_at",
            "Stop at durability left",
            Kind::Number {
                default: 3.0,
                min: 1.0,
                max: 50.0,
                step: 1.0,
            },
        ),
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

#[derive(Default)]
struct Warn {
    /// Slots (in `Worn::warn_order`) already warned about.
    warned: [bool; 6],
    current: Vec<String>,
}

thread_local! {
    static WARN: RefCell<Warn> = RefCell::new(Warn::default());
}

/// Whether `left` of `max` is below `percent` (strictly, as a double).
pub fn below(left: i32, max: i32, percent: f64) -> bool {
    left as f64 * 100.0 / (max as f64) < percent
}

/// Java's `shouldBlockMining` for the main hand stack (the rule only).
#[allow(dead_code)] // waits for the block-breaking injection
pub fn should_block_mining(main: Option<&Stack>, prevent_break: bool, prevent_at: i32) -> bool {
    prevent_break && main.is_some_and(|s| s.damageable() && s.left() <= prevent_at)
}

fn tick(host: &mut dyn Host) {
    WARN.with(|w| w.borrow_mut().current.clear());
    if !core::active(host, &MODULE) {
        return;
    }
    let percent = core::value(host, &MODULE, "percent").num();
    let sound = core::value(host, &MODULE, "sound").on();
    let worn = host.worn();
    for (i, slot) in worn.warn_order().into_iter().enumerate() {
        let Some(s) = slot.as_ref().filter(|s| s.damageable()) else {
            WARN.with(|w| w.borrow_mut().warned[i] = false);
            continue;
        };
        if below(s.left(), s.max_damage, percent) {
            let fresh = WARN.with(|w| {
                let mut w = w.borrow_mut();
                w.current
                    .push(format!("Low durability: {} ({})", s.name, s.left()));
                !std::mem::replace(&mut w.warned[i], true)
            });
            if fresh {
                host.action_bar(&format!("Omnitool: {} is almost broken!", s.name));
                if sound {
                    host.play_warning_sound();
                }
            }
        } else {
            WARN.with(|w| w.borrow_mut().warned[i] = false);
        }
    }
}

fn hud(host: &mut dyn Host) -> Option<HudBox> {
    let lines: Vec<(String, u32)> = WARN.with(|w| {
        w.borrow()
            .current
            .iter()
            .map(|l| (l.clone(), 0xFF5050))
            .collect()
    });
    if lines.is_empty() {
        return None;
    }
    Some(Canvas::new(host).lines(HudCorner::TopLeft, &lines, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};

    fn pick(damage: i32) -> Option<Stack> {
        Some(Stack {
            name: "Diamond Pickaxe".into(),
            max_damage: 1561,
            damage,
        })
    }

    fn host() -> Fake {
        restart();
        WARN.with(|w| *w.borrow_mut() = Warn::default());
        let mut h = Fake::default();
        core::set_enabled(&mut h, &MODULE, true);
        h
    }

    #[test]
    fn threshold_is_strict() {
        assert!(below(9, 100, 10.0));
        assert!(!below(10, 100, 10.0));
    }

    #[test]
    fn warns_once_per_slot_and_shows_line() {
        let mut h = host();
        h.worn.main = pick(1500);
        tick(&mut h);
        tick(&mut h);
        assert_eq!(h.bar, ["Omnitool: Diamond Pickaxe is almost broken!"]);
        assert_eq!(h.sounds, 1);
        let b = hud(&mut h).unwrap();
        assert_eq!(b.ops.len(), 1);
        // healthy again: no line, and it can warn anew
        h.worn.main = pick(0);
        tick(&mut h);
        assert!(hud(&mut h).is_none());
        h.worn.main = pick(1500);
        tick(&mut h);
        assert_eq!(h.bar.len(), 2);
    }

    #[test]
    fn line_text_and_sound_off() {
        let mut h = host();
        core::with(&mut h, |s, _| {
            s.state_mut(&MODULE).values[1] = crate::omnitool_setting::Value::Bool(false)
        });
        h.worn.head = pick(1561 - 12);
        tick(&mut h);
        assert_eq!(h.sounds, 0);
        WARN.with(|w| assert_eq!(w.borrow().current, ["Low durability: Diamond Pickaxe (12)"]));
    }

    #[test]
    fn mining_lock_rule() {
        let p = pick(1558);
        assert!(should_block_mining(p.as_ref(), true, 3));
        assert!(!should_block_mining(p.as_ref(), false, 3));
        assert!(!should_block_mining(pick(1557).as_ref(), true, 3));
        assert!(!should_block_mining(None, true, 3));
    }
}
