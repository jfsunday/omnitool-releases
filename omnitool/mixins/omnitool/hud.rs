// HUD basics (docs/spec/omnitool/hud.md, sections 0, 5.3 and 6.3): Java's
// HudElement table, HudFormat, ClickCounter and PickupFeed as pure logic, the
// draw list a HUD module returns (Horizon places the boxes by corner, see
// `HudCorner`), and the game data the HUD modules read through `Host`. Added
// to the game as `game::omnitool_hud`; also a module of this crate.

use super::omnitool_core::Host;
use std::collections::VecDeque;

/// Horizon places HUD boxes in the screen's corners (`Corners::place`), not at
/// free x/y: each element has a corner instead of Java's default position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// One thing to draw, relative to the box's top-left (GUI pixels).
#[derive(Clone, Debug, PartialEq)]
pub enum HudOp {
    Fill {
        rect: [f32; 4],
        argb: u32,
    },
    Text {
        text: String,
        at: [f32; 2],
        rgb: u32,
        shadow: bool,
    },
}

/// What a module's HUD element shows this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct HudBox {
    pub corner: HudCorner,
    pub w: f32,
    pub h: f32,
    pub ops: Vec<HudOp>,
}

/// Java's `HudElement`: config key, display name, default position (kept for
/// the config; Horizon places by `corner`).
#[allow(dead_code)]
pub struct Element {
    pub id: &'static str,
    pub module: &'static str,
    pub name: &'static str,
    pub x: i32,
    pub y: i32,
    pub corner: HudCorner,
}

pub const ELEMENTS: &[Element] = &[
    Element {
        id: "armor",
        module: "armor_hud",
        name: "Armor",
        x: 2,
        y: 60,
        corner: HudCorner::BottomLeft,
    },
    Element {
        id: "modules",
        module: "module_list",
        name: "Module List",
        x: 300,
        y: 2,
        corner: HudCorner::TopRight,
    },
    Element {
        id: "durability",
        module: "durability_warning",
        name: "Durability Warning",
        x: 150,
        y: 40,
        corner: HudCorner::TopLeft,
    },
    Element {
        id: "effects",
        module: "effect_timers",
        name: "Effects",
        x: 2,
        y: 140,
        corner: HudCorner::TopLeft,
    },
    Element {
        id: "keys",
        module: "keystrokes",
        name: "Keystrokes",
        x: 2,
        y: 180,
        corner: HudCorner::BottomLeft,
    },
    Element {
        id: "pickups",
        module: "item_pickups",
        name: "Item pickups",
        x: 2,
        y: 260,
        corner: HudCorner::BottomRight,
    },
    Element {
        id: "info",
        module: "omni_overlay",
        name: "Overlay (left)",
        x: 2,
        y: 2,
        corner: HudCorner::TopLeft,
    },
    Element {
        id: "overlay_right",
        module: "omni_overlay",
        name: "Overlay (right)",
        x: 100000,
        y: 2,
        corner: HudCorner::TopRight,
    },
    Element {
        id: "waypoints",
        module: "waypoints",
        name: "Waypoints",
        x: 2,
        y: 120,
        corner: HudCorner::TopLeft,
    },
];

/// Java's `Draw` for a box: collects ops, measures text through the host.
pub struct Canvas<'a> {
    host: &'a dyn Host,
    pub ops: Vec<HudOp>,
}

/// Vanilla font line height.
pub const LINE: f32 = 9.0;

impl<'a> Canvas<'a> {
    pub fn new(host: &'a dyn Host) -> Canvas<'a> {
        Canvas {
            host,
            ops: Vec::new(),
        }
    }
    pub fn width(&self, text: &str) -> f32 {
        self.host.text_width(text)
    }
    pub fn fill(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, argb: u32) {
        self.ops.push(HudOp::Fill {
            rect: [x1, y1, x2 - x1, y2 - y1],
            argb,
        });
    }
    pub fn text(&mut self, text: &str, x: f32, y: f32, rgb: u32, shadow: bool) {
        self.ops.push(HudOp::Text {
            text: text.to_owned(),
            at: [x, y],
            rgb,
            shadow,
        });
    }
    /// Java's `HudElement.lines`: the box and its lines (`background` 0x90000000).
    pub fn lines(mut self, corner: HudCorner, lines: &[(String, u32)], background: bool) -> HudBox {
        let w = lines.iter().map(|l| self.width(&l.0)).fold(0.0, f32::max);
        let lh = LINE + 1.0;
        let width = (w + 4.0).max(10.0);
        let height = (lines.len() as f32 * lh + 3.0).max(10.0);
        if background {
            self.fill(0.0, 0.0, width, height, 0x9000_0000);
        }
        for (i, (text, color)) in lines.iter().enumerate() {
            self.text(text, 2.0, 2.0 + i as f32 * lh, *color, true);
        }
        HudBox {
            corner,
            w: width,
            h: height,
            ops: self.ops,
        }
    }
}

// ------------------------------------------------------------ game data

/// An item stack as the HUD reads it. `max_damage` 0 = not damageable.
#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    pub name: String,
    pub max_damage: i32,
    pub damage: i32,
}

impl Stack {
    pub fn damageable(&self) -> bool {
        self.max_damage > 0
    }
    pub fn left(&self) -> i32 {
        self.max_damage - self.damage
    }
}

/// The equipment slots, in Java's order of the HUD (head to offhand).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Worn {
    pub head: Option<Stack>,
    pub chest: Option<Stack>,
    pub legs: Option<Stack>,
    pub feet: Option<Stack>,
    pub main: Option<Stack>,
    pub off: Option<Stack>,
}

impl Worn {
    /// HEAD, CHEST, LEGS, FEET, MAINHAND, OFFHAND.
    pub fn hud_order(&self) -> [&Option<Stack>; 6] {
        [
            &self.head,
            &self.chest,
            &self.legs,
            &self.feet,
            &self.main,
            &self.off,
        ]
    }
    /// Java's `EquipmentSlot` order for Durability Warning: MAINHAND, OFFHAND,
    /// FEET, LEGS, CHEST, HEAD.
    pub fn warn_order(&self) -> [&Option<Stack>; 6] {
        [
            &self.main,
            &self.off,
            &self.feet,
            &self.legs,
            &self.chest,
            &self.head,
        ]
    }
}

/// An active effect. `duration` -1 = infinite.
#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    pub name: String,
    pub amplifier: i32,
    pub duration: i32,
    pub color: u32,
    pub harmful: bool,
    pub ambient: bool,
}

/// The inventory totals Item Pickups counts, and what stops it counting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inventory {
    /// Item name and count per slot (all slots incl. armor and offhand).
    pub slots: Vec<(String, u32)>,
    /// Any screen open (chest, chat, pause ...).
    pub screen_open: bool,
    /// Creative or spectator.
    pub creative: bool,
}

// ------------------------------------------------------------ HudFormat

/// `HudFormat.duration`.
pub fn duration(ticks: i64) -> String {
    if ticks < 0 {
        return "∞".to_owned();
    }
    let s = (ticks + 19) / 20;
    let (h, m, sec) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}

/// `HudFormat.level`.
pub fn level(amplifier: i32) -> String {
    const ROMAN: [&str; 10] = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"];
    let lvl = amplifier + 1;
    if (1..=10).contains(&lvl) {
        ROMAN[(lvl - 1) as usize].to_owned()
    } else {
        lvl.to_string()
    }
}

/// `DurabilityWarning.color`: green above 50 % left, yellow above 20 %, else red.
pub fn durability_color(damage: i32, max_damage: i32) -> u32 {
    let f = 1.0 - damage as f32 / max_damage as f32;
    if f > 0.5 {
        0xFF60_FF60
    } else if f > 0.2 {
        0xFFFF_D040
    } else {
        0xFFFF_4040
    }
}

// ------------------------------------------------------------ ClickCounter

/// Java's `ClickCounter`: clicks (rising edges) of the last second.
#[derive(Default)]
#[allow(dead_code)] // Horizon counts clicks itself (`native_clicks`); the fixtures check this
pub struct ClickCounter {
    clicks: VecDeque<u64>,
    down: bool,
}

#[allow(dead_code)]
impl ClickCounter {
    pub fn sample(&mut self, pressed: bool, now_ms: u64) {
        if pressed && !self.down {
            self.clicks.push_back(now_ms);
        }
        self.down = pressed;
        self.prune(now_ms);
    }
    pub fn cps(&mut self, now_ms: u64) -> usize {
        self.prune(now_ms);
        self.clicks.len()
    }
    fn prune(&mut self, now_ms: u64) {
        while self.clicks.front().is_some_and(|t| now_ms - t >= 1000) {
            self.clicks.pop_front();
        }
    }
}

// ------------------------------------------------------------ PickupFeed

/// One line of the feed.
#[derive(Clone, Debug, PartialEq)]
pub struct Pickup {
    pub key: String,
    pub amount: u32,
    pub time: u64,
}

/// Java's `PickupFeed`, keyed by item name.
#[derive(Default)]
pub struct PickupFeed {
    entries: Vec<Pickup>,
    last: Option<Vec<(String, u32)>>,
}

impl PickupFeed {
    pub fn reset(&mut self) {
        self.last = None;
    }

    /// `totals`: count per item (any order). Gains since the last call show up.
    pub fn update(&mut self, totals: &[(String, u32)], now_ms: u64, max_entries: usize) {
        if let Some(last) = &self.last {
            let gains: Vec<(String, u32)> = totals
                .iter()
                .filter_map(|(k, t)| {
                    let before = last.iter().find(|(o, _)| o == k).map_or(0, |o| o.1);
                    (*t > before).then(|| (k.clone(), t - before))
                })
                .collect();
            for (k, gain) in gains {
                self.add(&k, gain, now_ms, max_entries);
            }
        }
        self.last = Some(totals.to_vec());
    }

    fn add(&mut self, key: &str, gain: u32, now_ms: u64, max_entries: usize) {
        if let Some(i) = self.entries.iter().position(|e| e.key == key) {
            let mut e = self.entries.remove(i);
            e.amount += gain;
            e.time = now_ms;
            self.entries.insert(0, e);
        } else {
            self.entries.insert(
                0,
                Pickup {
                    key: key.to_owned(),
                    amount: gain,
                    time: now_ms,
                },
            );
            self.entries.truncate(max_entries.max(1));
        }
    }

    /// Drops expired lines; newest first.
    pub fn visible(&mut self, now_ms: u64, show_ms: u64) -> &[Pickup] {
        self.entries.retain(|e| now_ms - e.time < show_ms);
        &self.entries
    }
}

/// `PickupFeed.alpha`.
pub fn alpha(age_ms: i64, show_ms: i64, fade_ms: i64) -> f32 {
    let left = show_ms - age_ms;
    if left <= 0 {
        0.0
    } else if left >= fade_ms {
        1.0
    } else {
        left as f32 / fade_ms as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::Fake;

    fn fixtures() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/hud-format.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn duration_matches_java() {
        for row in fixtures()["duration"].as_array().unwrap() {
            let ticks = row[0].as_i64().unwrap();
            assert_eq!(duration(ticks), row[1].as_str().unwrap(), "{ticks}");
        }
    }

    #[test]
    fn level_matches_java() {
        for row in fixtures()["level"].as_array().unwrap() {
            let amp = row[0].as_i64().unwrap() as i32;
            assert_eq!(level(amp), row[1].as_str().unwrap(), "{amp}");
        }
    }

    #[test]
    fn cps_matches_java() {
        for case in fixtures()["cps"].as_array().unwrap() {
            let mut c = ClickCounter::default();
            for op in case["ops"].as_array().unwrap() {
                let ms = op[if op[0] == "sample" { 2 } else { 1 }].as_u64().unwrap();
                if op[0] == "sample" {
                    c.sample(op[1].as_bool().unwrap(), ms);
                } else {
                    let want = op[2].as_u64().unwrap() as usize;
                    assert_eq!(c.cps(ms), want, "{} at {ms}", case["name"]);
                }
            }
        }
    }

    #[test]
    fn alpha_matches_java() {
        for row in fixtures()["alpha"].as_array().unwrap() {
            let v = |i: usize| row[i].as_i64().unwrap();
            let want = row[3].as_f64().unwrap();
            assert!(
                (alpha(v(0), v(1), v(2)) as f64 - want).abs() < 1e-6,
                "{row}"
            );
        }
    }

    #[test]
    fn durability_colors() {
        assert_eq!(durability_color(0, 100), 0xFF60_FF60);
        assert_eq!(durability_color(50, 100), 0xFFFF_D040);
        assert_eq!(durability_color(80, 100), 0xFFFF_4040);
    }

    fn totals(v: &[(&str, u32)]) -> Vec<(String, u32)> {
        v.iter().map(|(k, n)| (k.to_string(), *n)).collect()
    }

    #[test]
    fn feed_merges_gains_and_ignores_losses() {
        let mut f = PickupFeed::default();
        f.update(&totals(&[("log", 5)]), 0, 6);
        f.update(&totals(&[("log", 8), ("dirt", 1)]), 1, 6);
        f.update(&totals(&[("log", 10), ("dirt", 1)]), 2, 6);
        f.update(&totals(&[("log", 4), ("dirt", 1)]), 3, 6);
        let v: Vec<(&str, u32)> = f
            .visible(4, 3000)
            .iter()
            .map(|e| (e.key.as_str(), e.amount))
            .collect();
        assert_eq!(v, [("log", 5), ("dirt", 1)]);
    }

    #[test]
    fn feed_limits_and_expires() {
        let mut f = PickupFeed::default();
        f.update(&[], 0, 2);
        f.update(&totals(&[("a", 1), ("b", 1), ("c", 1)]), 10, 2);
        assert_eq!(f.visible(10, 3000).len(), 2);
        assert_eq!(f.visible(3010, 3000).len(), 0);
    }

    #[test]
    fn canvas_lines_box() {
        let h = Fake::default();
        let b = Canvas::new(&h).lines(
            HudCorner::TopLeft,
            &[("abc".into(), 0xFFFF_FFFF), ("a".into(), 0)],
            true,
        );
        assert_eq!(b.h, 2.0 * 10.0 + 3.0);
        assert_eq!(b.w, 3.0 * 6.0 + 4.0);
        assert_eq!(b.ops.len(), 3);
    }
}
