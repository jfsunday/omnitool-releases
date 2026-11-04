// Slot tweaks (docs/spec/omnitool/inventory.md, section 4; Java's
// core/SlotTweaks): the pure slot logic behind Mouse Tweaks and the sorting
// of containers. No game types; checked against
// tests/fixtures/inventory-slot-tweaks.json. Added to the game as
// `game::omnitool_slot_tweaks`; also a module of this crate.

/// Sections of a menu (`section`).
#[allow(dead_code)]
pub const NONE: i32 = -1;
#[allow(dead_code)]
pub const CONTAINER: i32 = 0;
#[allow(dead_code)]
pub const PLAYER: i32 = 1;
#[allow(dead_code)]
pub const MAIN: i32 = 2;
#[allow(dead_code)]
pub const HOTBAR: i32 = 3;
/// Kinds relative to the reference stack (`kind`).
#[allow(dead_code)]
pub const EMPTY: i32 = -1;
#[allow(dead_code)]
pub const SAME: i32 = 0;
#[allow(dead_code)]
pub const OTHER: i32 = 1;
/// Edge of the square that hits a slot.
#[allow(dead_code)]
pub const HIT: f64 = 18.0;

/// How scrolling decides the direction (`SlotTweaks.Wheel`, ordinal order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Wheel {
    DownPushes,
    UpPushes,
    ByPosition,
}

impl Wheel {
    #[allow(dead_code)]
    pub const ALL: [Wheel; 3] = [Wheel::DownPushes, Wheel::UpPushes, Wheel::ByPosition];

    /// The saved name (`DOWN_PUSHES`, ...).
    #[allow(dead_code)]
    pub fn name(self) -> &'static str {
        match self {
            Wheel::DownPushes => "DOWN_PUSHES",
            Wheel::UpPushes => "UP_PUSHES",
            Wheel::ByPosition => "BY_POSITION",
        }
    }

    #[allow(dead_code)]
    pub fn from_name(name: &str) -> Option<Wheel> {
        Wheel::ALL.into_iter().find(|w| w.name() == name)
    }
}

/// One active slot of a menu, as the tweaks see it (`SlotView`).
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub struct SlotView {
    pub id: i32,
    pub section: i32,
    pub kind: i32,
    pub count: i32,
    pub max: i32,
    pub accepts: bool,
    pub hotbar: bool,
    pub x: i32,
    pub y: i32,
}

impl SlotView {
    #[allow(dead_code)]
    pub fn empty(&self) -> bool {
        self.kind == EMPTY || self.count <= 0
    }
}

/// `section`: which part of the menu a slot belongs to. `player_index` is the
/// slot's index in the player inventory (0-8 hotbar, 9-35 main, 36+ armor and
/// offhand) or negative for slots of another container.
#[allow(dead_code)]
pub fn section(player_index: i32, player_menu: bool, output: bool) -> i32 {
    if player_index >= 0 {
        if player_index > 35 {
            NONE
        } else if !player_menu {
            PLAYER
        } else if player_index < 9 {
            HOTBAR
        } else {
            MAIN
        }
    } else if player_menu || output {
        NONE
    } else {
        CONTAINER
    }
}

/// `other`: the section items move to and from.
#[allow(dead_code)]
pub fn other(section: i32) -> i32 {
    match section {
        CONTAINER => PLAYER,
        PLAYER => CONTAINER,
        MAIN => HOTBAR,
        HOTBAR => MAIN,
        _ => NONE,
    }
}

/// `pushes`: whether this scroll moves items out of the hovered section.
#[allow(dead_code)]
pub fn pushes(scrolled_down: bool, mode: Wheel, hovered_y: f64, other_y: f64) -> bool {
    match mode {
        Wheel::DownPushes => scrolled_down,
        Wheel::UpPushes => !scrolled_down,
        Wheel::ByPosition => scrolled_down == (other_y > hovered_y),
    }
}

/// `sectionY`: mean `y` of the section's slots, NaN without slots.
#[allow(dead_code)]
pub fn section_y(slots: &[SlotView], section: i32) -> f64 {
    let ys: Vec<f64> = slots
        .iter()
        .filter(|s| s.section == section)
        .map(|s| f64::from(s.y))
        .collect();
    if ys.is_empty() {
        f64::NAN
    } else {
        ys.iter().sum::<f64>() / ys.len() as f64
    }
}

/// `insertTarget`: a partial stack of the same item first, else an empty
/// slot; -1 when there is none.
#[allow(dead_code)]
pub fn insert_target(slots: &[SlotView], section: i32, exclude: i32) -> i32 {
    let mut ok = slots
        .iter()
        .filter(|s| s.section == section && s.id != exclude && s.accepts);
    let partial = ok.clone().find(|s| s.kind == SAME && s.count < s.max);
    partial
        .or_else(|| ok.find(|s| s.empty()))
        .map_or(-1, |s| s.id)
}

/// `pullSource`: the smallest stack of the same item in the section (the
/// first on a tie); -1 when there is none.
#[allow(dead_code)]
pub fn pull_source(slots: &[SlotView], section: i32, exclude: i32) -> i32 {
    let mut best: Option<&SlotView> = None;
    for s in slots {
        if s.section == section
            && s.id != exclude
            && s.kind == SAME
            && s.count > 0
            && best.is_none_or(|b| s.count < b.count)
        {
            best = Some(s);
        }
    }
    best.map_or(-1, |s| s.id)
}

/// `canDropOne`: one item of the cursor stack fits into the slot.
#[allow(dead_code)]
pub fn can_drop_one(slot: &SlotView) -> bool {
    slot.accepts && (slot.empty() || (slot.kind == SAME && slot.count < slot.max))
}

/// `gatherClicks`: 2 left clicks when the slot's stack and the cursor stack
/// fit together (put down, pick up again), else 0.
#[allow(dead_code)]
pub fn gather_clicks(cursor_count: i32, cursor_max: i32, slot: &SlotView) -> i32 {
    if slot.kind != SAME || slot.count <= 0 || cursor_count <= 0 {
        0
    } else if cursor_count + slot.count <= cursor_max {
        2
    } else {
        0
    }
}

/// `sameKind`: ids of the section's slots with the same kind of item.
#[allow(dead_code)]
pub fn same_kind(slots: &[SlotView], section: i32) -> Vec<i32> {
    slots
        .iter()
        .filter(|s| s.section == section && s.kind == SAME && s.count > 0)
        .map(|s| s.id)
        .collect()
}

/// `moveAll`: ids of the section's filled slots outside the hotbar.
#[allow(dead_code)]
pub fn move_all(slots: &[SlotView], section: i32) -> Vec<i32> {
    slots
        .iter()
        .filter(|s| s.section == section && !s.empty() && !s.hotbar)
        .map(|s| s.id)
        .collect()
}

/// `slotAt`: the first slot whose 18x18 square (one pixel around the icon)
/// holds the point; -1 when there is none.
#[allow(dead_code)]
pub fn slot_at(slots: &[SlotView], px: f64, py: f64) -> i32 {
    for s in slots {
        let (x, y) = (f64::from(s.x) - 1.0, f64::from(s.y) - 1.0);
        if px >= x && px < x + HIT && py >= y && py < y + HIT {
            return s.id;
        }
    }
    -1
}

/// `slotsAlong`: the slots a mouse move crosses, in order, without the start
/// slot and without repeats (sampled every 2 px at most).
#[allow(dead_code)]
pub fn slots_along(slots: &[SlotView], x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<i32> {
    let start = slot_at(slots, x0, y0);
    let (dx, dy) = (x1 - x0, y1 - y0);
    let len = dx.hypot(dy);
    let steps = ((len / 2.0).ceil() as i32).max(1);
    let mut last = start;
    let mut out = Vec::new();
    for i in 1..=steps {
        let t = f64::from(i) / f64::from(steps);
        let id = slot_at(slots, x0 + dx * t, y0 + dy * t);
        if id < 0 || id == last {
            continue;
        }
        last = id;
        if id != start && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_inventory_plan::ClickQueue;
    use serde_json::Value;

    fn fixtures() -> Value {
        serde_json::from_str(
            &std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../tests/fixtures/inventory-slot-tweaks.json"
            ))
            .unwrap(),
        )
        .unwrap()
    }

    fn view(v: &Value) -> SlotView {
        let i = |k: &str| v[k].as_i64().unwrap() as i32;
        SlotView {
            id: i("id"),
            section: i("section"),
            kind: i("kind"),
            count: i("count"),
            max: i("max"),
            accepts: v["accepts"].as_bool().unwrap(),
            hotbar: v["hotbar"].as_bool().unwrap(),
            x: i("x"),
            y: i("y"),
        }
    }

    fn views(v: &Value) -> Vec<SlotView> {
        v.as_array().unwrap().iter().map(view).collect()
    }

    fn int(v: &Value) -> i32 {
        v.as_i64().unwrap() as i32
    }

    fn ids(v: &Value) -> Vec<i32> {
        v.as_array().unwrap().iter().map(int).collect()
    }

    #[test]
    fn section_and_other() {
        let f = fixtures();
        for c in f["section"].as_array().unwrap() {
            let got = section(
                int(&c["playerIndex"]),
                c["playerMenu"].as_bool().unwrap(),
                c["output"].as_bool().unwrap(),
            );
            assert_eq!(got, int(&c["out"]), "{}", c["name"]);
        }
        for c in f["other"].as_array().unwrap() {
            assert_eq!(other(int(&c["section"])), int(&c["out"]));
        }
    }

    #[test]
    fn pushes_fixtures() {
        for c in fixtures()["pushes"].as_array().unwrap() {
            let mode = Wheel::from_name(c["mode"].as_str().unwrap()).unwrap();
            let got = pushes(
                c["scrolledDown"].as_bool().unwrap(),
                mode,
                c["hoveredY"].as_f64().unwrap(),
                c["otherY"].as_f64().unwrap(),
            );
            assert_eq!(got, c["out"].as_bool().unwrap(), "{c}");
        }
    }

    #[test]
    fn targets_and_clicks() {
        let f = fixtures();
        for c in f["insertTarget"].as_array().unwrap() {
            let got = insert_target(
                &views(&c["slots"]),
                int(&c["section"]),
                int(&c["excludeId"]),
            );
            assert_eq!(got, int(&c["out"]), "{}", c["name"]);
        }
        for c in f["pullSource"].as_array().unwrap() {
            let got = pull_source(
                &views(&c["slots"]),
                int(&c["section"]),
                int(&c["excludeId"]),
            );
            assert_eq!(got, int(&c["out"]), "{}", c["name"]);
        }
        for c in f["canDropOne"].as_array().unwrap() {
            assert_eq!(can_drop_one(&view(&c["slot"])), c["out"].as_bool().unwrap());
        }
        for c in f["gatherClicks"].as_array().unwrap() {
            let got = gather_clicks(
                int(&c["cursorCount"]),
                int(&c["cursorMax"]),
                &view(&c["slot"]),
            );
            assert_eq!(got, int(&c["out"]), "{}", c["name"]);
        }
    }

    #[test]
    fn bulk_fixtures() {
        let b = &fixtures()["bulk"];
        let slots = views(&b["slots"]);
        for c in b["sameKind"].as_array().unwrap() {
            assert_eq!(same_kind(&slots, int(&c["section"])), ids(&c["out"]));
        }
        for c in b["moveAll"].as_array().unwrap() {
            assert_eq!(move_all(&slots, int(&c["section"])), ids(&c["out"]));
        }
    }

    #[test]
    fn geometry_fixtures() {
        let f = fixtures();
        let at = &f["slotAt"];
        let slots = views(&at["slots"]);
        for c in at["cases"].as_array().unwrap() {
            let got = slot_at(&slots, c["x"].as_f64().unwrap(), c["y"].as_f64().unwrap());
            assert_eq!(got, int(&c["out"]), "{c}");
        }
        let along = &f["slotsAlong"];
        let slots = views(&along["slots"]);
        for c in along["cases"].as_array().unwrap() {
            let n = |k: &str| c[k].as_f64().unwrap();
            let got = slots_along(&slots, n("x0"), n("y0"), n("x1"), n("y1"));
            assert_eq!(got, ids(&c["out"]), "{}", c["note"]);
        }
        let sy = &f["sectionY"];
        let slots = views(&sy["slots"]);
        for c in sy["cases"].as_array().unwrap() {
            let got = section_y(&slots, int(&c["section"]));
            match c["out"].as_f64() {
                Some(want) => assert_eq!(got, want),
                None => assert!(got.is_nan()),
            }
        }
    }

    #[test]
    fn click_queue_fixtures() {
        let q = &fixtures()["clickQueue"];
        let mut sets = vec![q.clone()];
        sets.extend(q["more"].as_array().unwrap().iter().cloned());
        for set in sets {
            let mut queue = ClickQueue::default();
            for j in set["jobs"].as_array().unwrap() {
                queue.add(int(j));
            }
            let mut total = 0;
            for t in set["ticks"].as_array().unwrap() {
                queue.refill(int(&t["refill"]));
                queue.run(|clicks| {
                    total += clicks;
                    clicks
                });
                assert_eq!(total, int(&t["clicksTotal"]), "{t}");
                assert_eq!(queue.len() as i32, int(&t["queued"]), "{t}");
                assert_eq!(queue.budget(), int(&t["budget"]), "{t}");
            }
        }
    }
}
