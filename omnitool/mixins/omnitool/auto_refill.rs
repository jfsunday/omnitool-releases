// Auto Refill (docs/spec/omnitool/automation.md, section 2): when a hotbar
// (or offhand) stack is used up, or a tool broke, moves a matching stack from
// the main inventory into the slot with ordinary menu clicks. Fair variant:
// Safe Mode leaves the offhand alone. The decision (`refill`) is pure and
// checked against tests/fixtures/trading.json (refill_enabled_for,
// refill_source, refill_tick). Slots are inventory-menu slots: 9-35 main,
// 36-44 hotbar, 45 offhand. Added to the game as `game::omnitool_auto_refill`;
// also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_inventory_plan::tool_family;
use super::omnitool_setting::{self as setting, Kind, SettingDef};
use std::cell::RefCell;
use std::collections::HashMap;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "auto_refill",
    name: "Auto Refill",
    category: Category::Player,
    description: "Refills a hotbar slot from your inventory when the stack runs out or a tool breaks.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: true,
    replaces: "",
    settings: &[
        SettingDef::new("blocks", "Refill blocks", Kind::Bool(true)),
        SettingDef::new("food", "Refill food", Kind::Bool(true)),
        SettingDef::new(
            "tools",
            "Replace broken tools (same tool type)",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "other",
            "Refill other items (arrows, pearls, totems...)",
            Kind::Bool(true),
        ),
        SettingDef::new("offhand", "Also refill the offhand", Kind::Bool(true)),
        SettingDef::new(
            "threshold",
            "Refill when fewer than (0 = when empty)",
            Kind::Number {
                default: 0.0,
                min: 0.0,
                max: 32.0,
                step: 1.0,
            },
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

/// First and last inventory-menu slot Auto Refill looks at.
pub const FIRST: usize = 9;
pub const LAST: usize = 45;

/// An item stack as the decision sees it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stack {
    /// `namespace:path`, "" = empty stack.
    pub id: String,
    pub count: i32,
    pub max_stack: i32,
    pub damageable: bool,
    pub block: bool,
    pub food: bool,
    /// Everything that makes two stacks of one item different (components,
    /// damage), compared as a whole.
    pub comp: String,
}

impl Stack {
    pub fn is_empty(&self) -> bool {
        self.id.is_empty() || self.count <= 0
    }
    fn stackable(&self) -> bool {
        self.max_stack > 1 && !self.damageable
    }
    fn same_item(&self, o: &Stack) -> bool {
        self.id == o.id
    }
    fn same_item_same_components(&self, o: &Stack) -> bool {
        self.same_item(o) && self.comp == o.comp
    }
}

/// The settings the decision reads.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub blocks: bool,
    pub food: bool,
    pub tools: bool,
    pub other: bool,
    pub offhand: bool,
    pub threshold: i32,
    pub safe_mode: bool,
}

/// `enabledFor`: the category of the stack, first match wins.
pub fn enabled_for(c: &Config, s: &Stack) -> bool {
    if s.damageable {
        c.tools
    } else if s.block {
        c.blocks
    } else if s.food {
        c.food
    } else {
        c.other
    }
}

/// `source`: the main-inventory slot (9-35) to take from, or None. `slot(i)`
/// is the stack in menu slot `i`.
pub fn source(slot: impl Fn(usize) -> Stack, prev: &Stack, empty: bool) -> Option<usize> {
    let (mut same_item, mut family) = (None, None);
    for i in 9..36 {
        let s = slot(i);
        if s.is_empty() {
            continue;
        }
        if s.same_item_same_components(prev) && !prev.damageable {
            return Some(i);
        }
        if s.same_item(prev) {
            same_item.get_or_insert(i);
        } else if empty
            && prev.damageable
            && s.damageable
            && family.is_none()
            && tool_family(&prev.id) == tool_family(&s.id)
        {
            family = Some(i);
        }
    }
    if !empty { None } else { same_item.or(family) }
}

fn totals<'a>(stacks: impl Iterator<Item = &'a Stack>) -> HashMap<&'a str, i64> {
    let mut m = HashMap::new();
    for s in stacks.filter(|s| !s.is_empty()) {
        *m.entry(s.id.as_str()).or_insert(0) += i64::from(s.count);
    }
    m
}

/// One tick's decision: `last` and `now` hold the stacks of menu slots 9..=45
/// (index = slot - 9). Returns `(source, target)` menu slots.
pub fn refill(last: &[Stack], now: &[Stack], c: &Config) -> Option<(usize, usize)> {
    let at = |v: &[Stack], slot: usize| v.get(slot - FIRST).cloned().unwrap_or_default();
    let (before, after) = (totals(last.iter()), totals(now.iter()));
    for slot in 36..=LAST {
        if slot == 45 && (!c.offhand || c.safe_mode) {
            continue;
        }
        let (prev, cur) = (at(last, slot), at(now, slot));
        if prev.is_empty() || !enabled_for(c, &prev) {
            continue;
        }
        let id = prev.id.as_str();
        if after.get(id).copied().unwrap_or(0) >= before.get(id).copied().unwrap_or(0) {
            continue;
        }
        let empty = cur.is_empty();
        let low = !empty
            && prev.stackable()
            && prev.same_item_same_components(&cur)
            && cur.count < c.threshold;
        if !empty && !low {
            continue;
        }
        if let Some(src) = source(|i| at(now, i), &prev, empty) {
            return Some((src, slot));
        }
    }
    None
}

#[derive(Default)]
struct State {
    last: Option<Vec<Stack>>,
    cooldown: i32,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn tick(host: &mut dyn Host) {
    let Some(view) = host.refill_view() else {
        return;
    };
    let cfg = Config {
        blocks: core::value(host, &MODULE, "blocks").on(),
        food: core::value(host, &MODULE, "food").on(),
        tools: core::value(host, &MODULE, "tools").on(),
        other: core::value(host, &MODULE, "other").on(),
        offhand: core::value(host, &MODULE, "offhand").on(),
        threshold: core::value(host, &MODULE, "threshold").num().round() as i32,
        safe_mode: core::safe_mode(host, &MODULE),
    };
    let active = core::active(host, &MODULE);
    let pick = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let mut pick = None;
        // `isActive() && cooldown-- <= 0 && ready && last != null`
        if active {
            let go = s.cooldown <= 0;
            s.cooldown -= 1;
            if go
                && view.ready
                && let Some(last) = &s.last
            {
                pick = refill(last, &view.slots, &cfg);
                if pick.is_some() {
                    s.cooldown = 2;
                }
            }
        }
        s.last = Some(view.slots);
        pick
    });
    if let Some((src, slot)) = pick {
        host.click_menu_slot(src);
        host.click_menu_slot(slot);
        if !host.carried_empty() {
            host.click_menu_slot(src);
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

    /// The fixtures' mini format: `id xCOUNT [block food dmg max=N comp=X]`.
    fn stack(text: &str) -> Stack {
        if text.is_empty() {
            return Stack::default();
        }
        let (head, flags) = text
            .split_once(" [")
            .map_or((text, ""), |(h, f)| (h, f.trim_end_matches(']')));
        let (id, count) = head.split_once(" x").unwrap();
        let mut s = Stack {
            id: id.to_owned(),
            count: count.parse().unwrap(),
            max_stack: 64,
            ..Stack::default()
        };
        for f in flags.split_whitespace() {
            match f {
                "block" => s.block = true,
                "food" => s.food = true,
                "dmg" => {
                    s.damageable = true;
                    s.max_stack = 1;
                }
                _ => {
                    if let Some(n) = f.strip_prefix("max=") {
                        s.max_stack = n.parse().unwrap();
                    } else if let Some(c) = f.strip_prefix("comp=") {
                        s.comp = c.to_owned();
                    }
                }
            }
        }
        s
    }

    fn slots(map: &Value) -> Vec<Stack> {
        let mut v = vec![Stack::default(); LAST - FIRST + 1];
        for (k, s) in map.as_object().unwrap() {
            v[k.parse::<usize>().unwrap() - FIRST] = stack(s.as_str().unwrap());
        }
        v
    }

    fn cfg(s: &Value) -> Config {
        let b = |k: &str| s[k].as_bool().unwrap_or(false);
        Config {
            blocks: b("blocks"),
            food: b("food"),
            tools: b("tools"),
            other: b("other"),
            offhand: b("offhand"),
            threshold: s["threshold"].as_i64().unwrap_or(0) as i32,
            safe_mode: b("safe_mode"),
        }
    }

    #[test]
    fn enabled_for_fixtures() {
        for c in fixtures()["refill_enabled_for"].as_array().unwrap() {
            let got = enabled_for(&cfg(&c["settings"]), &stack(c["stack"].as_str().unwrap()));
            assert_eq!(got, c["expect"].as_bool().unwrap(), "{c}");
        }
    }

    #[test]
    fn source_fixtures() {
        for c in fixtures()["refill_source"].as_array().unwrap() {
            let v = slots(&c["slots"]);
            let got = source(
                |i| v[i - FIRST].clone(),
                &stack(c["prev"].as_str().unwrap()),
                c["empty"].as_bool().unwrap(),
            );
            assert_eq!(
                got.map_or(-1, |i| i as i64),
                c["expect"].as_i64().unwrap(),
                "{c}"
            );
        }
    }

    #[test]
    fn tick_fixtures() {
        for c in fixtures()["refill_tick"].as_array().unwrap() {
            let got = refill(&slots(&c["last"]), &slots(&c["now"]), &cfg(&c["settings"]));
            let want: Vec<i64> = c["expect_clicks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap())
                .collect();
            let got: Vec<i64> = got.map_or_else(Vec::new, |(a, b)| vec![a as i64, b as i64]);
            assert_eq!(got, want, "{}", c["desc"]);
        }
    }

    #[test]
    fn registered_with_fair_variant() {
        let m = crate::omnitool_registry::MODULES
            .iter()
            .find(|m| m.id == "auto_refill")
            .unwrap();
        assert!(m.fair_variant && !m.cheaty);
    }
}
