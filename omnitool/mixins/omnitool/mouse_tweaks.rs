// Mouse Tweaks (docs/spec/omnitool/inventory.md, section 3; Java's
// modules/MouseTweaks, beta): the settings and the pure logic of dragging,
// scrolling and the planned clicks. Hooking into the container screen's mouse
// needs an injection Horizon doesn't offer as a stable hook, so the module
// does nothing in the game yet (see the task result for the target). Added to
// the game as `game::omnitool_mouse_tweaks`; also a module of this crate.

use super::omnitool_core::{Category, ModuleInfo};
use super::omnitool_setting::{Kind, SettingDef};
use super::omnitool_slot_tweaks::{
    self as st, SlotView, Wheel, can_drop_one, gather_clicks, insert_target, pull_source,
};
use std::collections::HashSet;

/// GLFW `KEY_LEFT_ALT`.
const KEY_LALT: i32 = 342;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "mouse_tweaks",
    name: "Mouse Tweaks",
    category: Category::Misc,
    description: "Drag to spread or collect items, scroll to move items, Ctrl/Alt-click to move many at once.",
    default_on: true,
    cheaty: false,
    beta: true,
    fair_variant: false,
    replaces: "Mouse Tweaks, Item Scroller",
    settings: &[
        SettingDef::new(
            "right_drag",
            "Right-click drag: one item into each slot",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "left_drag",
            "Left-click drag with an item: collect the same item",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "shift_drag",
            "Shift + left drag: shift-click every slot",
            Kind::Bool(true),
        ),
        SettingDef::new("wheel", "Scroll wheel moves items", Kind::Bool(true)),
        SettingDef::new(
            "wheel_dir",
            "Scroll direction",
            Kind::Enum {
                values: &["DOWN_PUSHES", "UP_PUSHES", "BY_POSITION"],
                default: 0,
            },
        ),
        SettingDef::new(
            "wheel_stacks",
            "Scroll moves whole stacks (Shift inverts)",
            Kind::Bool(false),
        ),
        SettingDef::new(
            "move_type",
            "Ctrl + Shift-click: move all items of that type",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "move_all",
            "Key + click: move everything (not the hotbar)",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "move_all_key",
            "Key for move everything",
            Kind::Key(KEY_LALT),
        ),
        SettingDef::new(
            "drop_all",
            "Ctrl + drop key: drop all of that type",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "sort_click",
            "Shift-click an empty slot: sort that chest / inventory",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "clicks",
            "Max clicks per tick (server-friendly)",
            Kind::Number {
                default: 10.0,
                min: 1.0,
                max: 40.0,
                step: 1.0,
            },
        ),
    ],
    command: None,
    key: super::omnitool_setting::NONE,
    hold: false,
    hud: None,
    tick: None,
    on_key: None,
    hud_right: None,
    on_action: None,
};

/// A click the tweaks send: slot id (menu index), button, and the kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Click {
    /// Left (0) or right (1) pickup click.
    Pickup(i32, u8),
    QuickMove(i32),
    Throw(i32),
}

/// The scroll remainder between events (fractional wheel and touchpad
/// values add up).
#[derive(Clone, Copy, Debug, Default)]
#[allow(dead_code)]
pub struct ScrollAcc(f64);

impl ScrollAcc {
    /// Adds `amount` (positive = up); the whole notches to act on, negative
    /// = down. A change of direction drops the remainder.
    #[allow(dead_code)]
    pub fn add(&mut self, amount: f64) -> i32 {
        if amount.signum() != self.0.signum() {
            self.0 = 0.0;
        }
        self.0 += amount;
        let notches = self.0.trunc() as i32;
        self.0 -= f64::from(notches);
        notches
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.0 = 0.0;
    }
}

/// What a mouse drag does with each slot it enters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum DragKind {
    Shift,
    Right,
    Gather,
}

/// A drag in progress (`startDrag`, `advance`).
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Drag {
    pub kind: DragKind,
    pub button: i32,
    pub start: i32,
    pub moved: bool,
    visited: HashSet<i32>,
    last: (f64, f64),
}

/// What entering a slot asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Entered {
    /// Shift-click the slot if the cursor is empty and the slot filled.
    QuickMove(i32),
    /// One item from the cursor into the slot, if it fits.
    DropOne(i32),
    /// Collect into the cursor from the slot.
    Gather(i32),
}

impl Drag {
    #[allow(dead_code)]
    pub fn start(kind: DragKind, button: i32, start: i32, x: f64, y: f64) -> Drag {
        Drag {
            kind,
            button,
            start,
            moved: false,
            visited: HashSet::from([start]),
            last: (x, y),
        }
    }

    /// `advance`: the entries for the slots crossed on the way to (x, y).
    #[allow(dead_code)]
    pub fn advance(&mut self, slots: &[SlotView], x: f64, y: f64) -> Vec<Entered> {
        let crossed = st::slots_along(slots, self.last.0, self.last.1, x, y);
        self.last = (x, y);
        let mut out = Vec::new();
        for id in crossed {
            if !self.visited.insert(id) {
                continue;
            }
            match self.kind {
                DragKind::Shift => out.push(Entered::QuickMove(id)),
                DragKind::Right => out.push(Entered::DropOne(id)),
                DragKind::Gather => {
                    if !self.moved {
                        self.moved = true;
                        if self.start >= 0 {
                            out.push(Entered::Gather(self.start));
                        }
                    }
                    out.push(Entered::Gather(id));
                }
            }
        }
        out
    }
}

/// The clicks a job `Entered` sends given the cursor and the slot, as the
/// job decides when it runs. `cursor_count`/`cursor_max`: 0 = empty cursor.
#[allow(dead_code)]
pub fn entered_clicks(
    e: Entered,
    slot: &SlotView,
    cursor_count: i32,
    cursor_max: i32,
    may_pickup: bool,
) -> Vec<Click> {
    match e {
        Entered::QuickMove(id) => {
            if cursor_count == 0 && !slot.empty() {
                vec![Click::QuickMove(id)]
            } else {
                vec![]
            }
        }
        Entered::DropOne(id) => {
            if cursor_count > 0 && can_drop_one(slot) {
                vec![Click::Pickup(id, 1)]
            } else {
                vec![]
            }
        }
        Entered::Gather(id) => {
            if cursor_count == 0 || !may_pickup {
                return vec![];
            }
            vec![Click::Pickup(id, 0); gather_clicks(cursor_count, cursor_max, slot) as usize]
        }
    }
}

/// `wheelMove`: the clicks for one notch on slot `id`. `slots` is the view
/// for the hovered stack (kind relative to it); `hovered_here` its own
/// entry for pulling; `may_pickup(id)` tells whether a slot allows taking.
/// `cursor_left` is what stays on the cursor afterwards (the caller decides
/// from the real game; the plan only has the final return click).
#[allow(dead_code)]
pub fn wheel_move(
    slots: &[SlotView],
    id: i32,
    other: i32,
    push: bool,
    whole: bool,
    may_pickup: impl Fn(i32) -> bool,
) -> Option<(Vec<Click>, Click)> {
    let btn = if whole { 0 } else { 1 };
    if push {
        if !may_pickup(id) {
            return None;
        }
        let target = insert_target(slots, other, id);
        if target < 0 {
            return None;
        }
        Some((
            vec![Click::Pickup(id, 0), Click::Pickup(target, btn)],
            Click::Pickup(id, 0),
        ))
    } else {
        let here = slots.iter().find(|s| s.id == id)?;
        if !here.accepts || here.count >= here.max {
            return None;
        }
        let src = pull_source(slots, other, id);
        if src < 0 || !may_pickup(src) {
            return None;
        }
        Some((
            vec![Click::Pickup(src, 0), Click::Pickup(id, btn)],
            Click::Pickup(src, 0),
        ))
    }
}

/// `Wheel` from the setting's enum index.
#[allow(dead_code)]
pub fn wheel_mode(index: usize) -> Wheel {
    Wheel::ALL.get(index).copied().unwrap_or(Wheel::DownPushes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(id: i32, section: i32, kind: i32, count: i32, x: i32) -> SlotView {
        SlotView {
            id,
            section,
            kind,
            count,
            max: 64,
            accepts: true,
            hotbar: false,
            x,
            y: 0,
        }
    }

    #[test]
    fn scroll_accumulates_and_flips() {
        let mut a = ScrollAcc::default();
        assert_eq!(a.add(0.4), 0);
        assert_eq!(a.add(0.4), 0);
        assert_eq!(a.add(0.4), 1);
        assert_eq!(a.add(-1.0), -1, "direction change drops the 0.2 left");
        assert_eq!(a.add(2.0), 2);
    }

    #[test]
    fn defaults_match_the_spec() {
        assert_eq!(MODULE.settings.len(), 12);
        const { assert!(MODULE.beta && MODULE.default_on && !MODULE.cheaty) };
    }

    #[test]
    fn drag_visits_each_slot_once() {
        let slots: Vec<SlotView> = (0..4).map(|i| slot(i, 1, st::SAME, 5, 18 * i)).collect();
        let mut d = Drag::start(DragKind::Gather, 0, 0, 8.0, 8.0);
        let first = d.advance(&slots, 26.0, 8.0);
        assert_eq!(first, [Entered::Gather(0), Entered::Gather(1)]);
        assert!(d.moved);
        assert!(d.advance(&slots, 26.0, 8.0).is_empty());
        assert_eq!(d.advance(&slots, 44.0, 8.0), [Entered::Gather(2)]);
        let mut r = Drag::start(DragKind::Right, 1, 0, 8.0, 8.0);
        assert_eq!(r.advance(&slots, 62.0, 8.0).len(), 3);
    }

    #[test]
    fn entered_clicks_follow_the_spec() {
        let s = slot(3, 1, st::SAME, 20, 0);
        assert_eq!(
            entered_clicks(Entered::QuickMove(3), &s, 0, 0, true),
            [Click::QuickMove(3)]
        );
        assert!(entered_clicks(Entered::QuickMove(3), &s, 5, 64, true).is_empty());
        assert_eq!(
            entered_clicks(Entered::DropOne(3), &s, 5, 64, true),
            [Click::Pickup(3, 1)]
        );
        assert_eq!(
            entered_clicks(Entered::Gather(3), &s, 10, 64, true).len(),
            2
        );
        assert!(entered_clicks(Entered::Gather(3), &s, 50, 64, true).is_empty());
        assert!(entered_clicks(Entered::Gather(3), &s, 10, 64, false).is_empty());
    }

    #[test]
    fn wheel_move_push_and_pull() {
        let slots = vec![
            slot(0, st::CONTAINER, st::SAME, 10, 0),
            slot(1, st::PLAYER, st::SAME, 3, 18),
            slot(2, st::PLAYER, st::EMPTY, 0, 36),
        ];
        let (clicks, back) = wheel_move(&slots, 0, st::PLAYER, true, false, |_| true).unwrap();
        assert_eq!(clicks, [Click::Pickup(0, 0), Click::Pickup(1, 1)]);
        assert_eq!(back, Click::Pickup(0, 0));
        let (clicks, _) = wheel_move(&slots, 0, st::PLAYER, false, true, |_| true).unwrap();
        assert_eq!(clicks, [Click::Pickup(1, 0), Click::Pickup(0, 0)]);
        assert!(wheel_move(&slots, 0, st::PLAYER, true, false, |_| false).is_none());
    }
}
