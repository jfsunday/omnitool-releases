// Autoclicker (docs/spec/omnitool/automation.md, section 3): clicks with a
// random CPS rate through `Input::click`. Cheaty. `step` is the pure tick
// rule, checked against tests/fixtures/trading.json (autoclicker_cps,
// autoclicker_tick). Vanilla's `missTime = 0` / `rightClickDelay = 0` resets
// have no counterpart in Horizon's `play_mouse` path, so they are not done.
// Added to the game as `game::omnitool_autoclicker`; also a module of this
// crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{self as setting, Kind, SettingDef};
use std::cell::Cell;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "autoclicker",
    name: "Autoclicker",
    category: Category::Combat,
    description: "Clicks for you with randomized CPS.",
    default_on: false,
    cheaty: true,
    beta: false,
    fair_variant: false,
    replaces: "Autoclicker, Simple Autoclicker",
    settings: &[
        SettingDef::new(
            "button",
            "Button",
            Kind::Enum {
                values: &["LEFT", "RIGHT"],
                default: 0,
            },
        ),
        SettingDef::new(
            "min_cps",
            "Min CPS",
            Kind::Number {
                default: 8.0,
                min: 1.0,
                max: 20.0,
                step: 1.0,
            },
        ),
        SettingDef::new(
            "max_cps",
            "Max CPS",
            Kind::Number {
                default: 12.0,
                min: 1.0,
                max: 20.0,
                step: 1.0,
            },
        ),
        SettingDef::new(
            "while_holding",
            "Only while holding the button",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "cooldown",
            "Wait for full attack strength (left)",
            Kind::Bool(false),
        ),
        SettingDef::new(
            "items",
            "Only with these items (empty = any)",
            Kind::Text(""),
        ),
        SettingDef::new(
            "skip_blocks",
            "Don't left-click while mining a block",
            Kind::Bool(true),
        ),
    ],
    command: None,
    key: setting::NONE,
    hold: false,
    hud: None,
    tick: Some(tick),
    on_key: None,
    hud_right: None,
    on_action: None,
};

/// What one tick looks at (the spec's checks 1-5).
#[derive(Clone, Copy, Debug)]
pub struct In {
    pub active: bool,
    pub screen: bool,
    /// The mapped button (attack for left, use for right) is held.
    pub btn_down: bool,
    pub hold: bool,
    /// The main hand passes the `items` filter.
    pub item_ok: bool,
    pub skip_blocks: bool,
    pub hit_block: bool,
    pub cooldown: bool,
    pub strength_full: bool,
    pub left: bool,
    pub min_cps: f64,
    pub max_cps: f64,
}

/// Java's `tick` without the Minecraft calls: updates `pending` and returns
/// the number of clicks. `rand` is a uniform number in [0, 1).
pub fn step(pending: &mut f64, i: &In, rand: f64) -> u32 {
    if !i.active || i.screen {
        *pending = 0.0;
        return 0;
    }
    if i.hold && !i.btn_down {
        *pending = 0.0;
        return 0;
    }
    if !i.item_ok {
        return 0;
    }
    if i.left && i.skip_blocks && i.hit_block {
        *pending = 0.0;
        return 0;
    }
    if i.left && i.cooldown {
        return u32::from(i.strength_full);
    }
    let lo = i.min_cps.min(i.max_cps);
    let hi = i.min_cps.max(i.max_cps);
    *pending += (lo + rand * (hi - lo)) / 20.0;
    let mut clicks = 0;
    while *pending >= 1.0 {
        clicks += 1;
        *pending -= 1.0;
    }
    clicks
}

thread_local! {
    static PENDING: Cell<f64> = const { Cell::new(0.0) };
    static SEED: Cell<u64> = const { Cell::new(0) };
}

/// xorshift64*: any uniform [0, 1) will do (the spec allows it).
fn random(host: &dyn Host) -> f64 {
    SEED.with(|s| {
        let mut x = s.get();
        if x == 0 {
            x = host.now_ms() | 1;
        }
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        s.set(x);
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    })
}

fn tick(host: &mut dyn Host) {
    let active = core::active(host, &MODULE);
    let left = core::value(host, &MODULE, "button").index() == 0;
    let allowed = setting::ids(core::value(host, &MODULE, "items").text());
    let item_ok = allowed.is_empty() || allowed.contains(&host.main_hand_id());
    let i = In {
        active,
        screen: host.screen_open(),
        btn_down: host
            .mapping(if left { "key.attack" } else { "key.use" })
            .is_some_and(|m| m.0),
        hold: core::value(host, &MODULE, "while_holding").on(),
        item_ok,
        skip_blocks: core::value(host, &MODULE, "skip_blocks").on(),
        hit_block: host.aiming_at_block(),
        cooldown: core::value(host, &MODULE, "cooldown").on(),
        strength_full: host.attack_charged(),
        left,
        min_cps: core::value(host, &MODULE, "min_cps").num(),
        max_cps: core::value(host, &MODULE, "max_cps").num(),
    };
    let r = random(host);
    let mut pending = PENDING.with(Cell::get);
    let clicks = step(&mut pending, &i, r);
    PENDING.with(|p| p.set(pending));
    if clicks > 0
        && let Some(input) = host.input()
    {
        for _ in 0..clicks {
            input.click(left);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixtures() -> Value {
        serde_json::from_str(
            &std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../tests/fixtures/trading.json"
            ))
            .unwrap(),
        )
        .unwrap()
    }

    fn base(left: bool, min: f64, max: f64) -> In {
        In {
            active: true,
            screen: false,
            btn_down: true,
            hold: true,
            item_ok: true,
            skip_blocks: true,
            hit_block: false,
            cooldown: false,
            strength_full: false,
            left,
            min_cps: min,
            max_cps: max,
        }
    }

    #[test]
    fn cps_fixtures() {
        for c in fixtures()["autoclicker_cps"].as_array().unwrap() {
            let i = base(
                true,
                c["min_cps"].as_f64().unwrap(),
                c["max_cps"].as_f64().unwrap(),
            );
            let mut pending = 0.0;
            let got: Vec<u32> = (0..c["ticks"].as_u64().unwrap())
                .map(|_| step(&mut pending, &i, c["rand"].as_f64().unwrap()))
                .collect();
            let want: Vec<u32> = c["expect_clicks_per_tick"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect();
            assert_eq!(got, want, "{}", c["desc"]);
        }
    }

    #[test]
    fn tick_fixtures() {
        for c in fixtures()["autoclicker_tick"].as_array().unwrap() {
            let mut pending = 0.0;
            for (n, t) in c["ticks"].as_array().unwrap().iter().enumerate() {
                let f = &t["in"];
                let b = |k: &str| f[k].as_bool().unwrap();
                let cps = f["cps"].as_f64().unwrap();
                let i = In {
                    active: b("active"),
                    screen: b("screen"),
                    btn_down: b("btn_down"),
                    hold: b("hold"),
                    item_ok: b("item_ok"),
                    skip_blocks: b("skip_blocks"),
                    hit_block: b("hit_block"),
                    cooldown: b("cooldown"),
                    strength_full: b("strength_full"),
                    left: f["button"] == "left",
                    min_cps: cps,
                    max_cps: cps,
                };
                let clicks = step(&mut pending, &i, 0.3);
                let ctx = format!("{} tick {n}", c["desc"]);
                assert_eq!(clicks as u64, t["clicks"].as_u64().unwrap(), "{ctx}");
                assert!(
                    (pending - t["pending"].as_f64().unwrap()).abs() < 1e-9,
                    "{ctx}"
                );
            }
        }
    }

    #[test]
    fn registered() {
        assert!(
            crate::omnitool_registry::MODULES
                .iter()
                .any(|m| m.id == "autoclicker")
        );
    }
}
