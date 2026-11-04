// Keystrokes (docs/spec/omnitool/hud.md, section 5): WASD block, mouse
// buttons with CPS, jump and sneak bars, from `native_mapping`. Deviation:
// Horizon's text hook cannot scale, so the CPS line is drawn at full size
// (Java: 0.75) one row below the label. Added to the game as
// `game::omnitool_keystrokes`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_hud::{Canvas, ClickCounter, HudBox, HudCorner, LINE};
use super::omnitool_setting::{Kind, SettingDef, Value};
use std::cell::RefCell;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "keystrokes",
    name: "Keystrokes",
    category: Category::Hud,
    description: "Shows pressed movement keys and mouse buttons with CPS.",
    default_on: false,
    cheaty: false,
    beta: true,
    fair_variant: false,
    replaces: "",
    settings: &[
        SettingDef::new("mouse", "Mouse buttons", Kind::Bool(true)),
        SettingDef::new("cps", "Clicks per second", Kind::Bool(true)),
        SettingDef::new("space", "Jump bar", Kind::Bool(true)),
        SettingDef::new("sneak", "Sneak bar", Kind::Bool(false)),
        SettingDef::new(
            "pressed",
            "Pressed color",
            Kind::Color(0xC0FFFFFFu32 as i32),
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

const K: f32 = 20.0;
const GAP: f32 = 2.0;

#[derive(Default)]
struct Clicks {
    left: ClickCounter,
    right: ClickCounter,
}

thread_local! {
    static CLICKS: RefCell<Clicks> = RefCell::new(Clicks::default());
}

/// A key's label as Java cuts it: at most 5 characters.
pub fn label(name: &str) -> String {
    name.chars().take(5).collect()
}

/// One key: background, label (or `sub` for a thin bar), optional CPS line.
fn key(
    c: &mut Canvas,
    state: &(bool, String),
    rect: [f32; 4],
    pressed: u32,
    sub: Option<&str>,
    cps: Option<usize>,
) {
    let [x, y, w, h] = rect;
    let (down, name) = state;
    let down = *down;
    c.fill(x, y, x + w, y + h, if down { pressed } else { 0x9000_0000 });
    let color = if down { 0x000000 } else { 0xFFFFFF };
    let text = match sub {
        Some(s) if h < K => s.to_owned(),
        _ => label(name),
    };
    let tw = c.width(&text);
    match cps {
        Some(n) if h >= K => {
            let line = format!("{n} CPS");
            c.text(&text, x + (w - tw) / 2.0, y + 1.0, color, false);
            c.text(
                &line,
                x + (w - c.width(&line)) / 2.0,
                y + 1.0 + LINE,
                color,
                false,
            );
        }
        _ => c.text(
            &text,
            x + (w - tw) / 2.0,
            y + (h - LINE) / 2.0 + 1.0,
            color,
            false,
        ),
    }
}

fn hud(host: &mut dyn Host) -> Option<HudBox> {
    let on = |h: &dyn Host, id| core::value(h, &MODULE, id).on();
    let (mouse, cps, space, sneak) = (
        on(host, "mouse"),
        on(host, "cps"),
        on(host, "space"),
        on(host, "sneak"),
    );
    let pressed = match core::value(host, &MODULE, "pressed") {
        Value::Color(c) => c as u32,
        _ => 0xC0FF_FFFF,
    };
    let now = host.now_ms();
    let mut counts = (0, 0);
    if mouse && cps {
        let left = host.mapping("key.attack").is_some_and(|m| m.0);
        let right = host.mapping("key.use").is_some_and(|m| m.0);
        counts = CLICKS.with(|c| {
            let mut c = c.borrow_mut();
            c.left.sample(left, now);
            c.right.sample(right, now);
            (c.left.cps(now), c.right.cps(now))
        });
    }
    let mut states = std::collections::HashMap::new();
    for m in [
        "key.forward",
        "key.left",
        "key.back",
        "key.right",
        "key.attack",
        "key.use",
        "key.jump",
        "key.sneak",
    ] {
        states.insert(m, host.mapping(m).unwrap_or((false, String::new())));
    }
    let full = 3.0 * K + 2.0 * GAP;
    let mut c = Canvas::new(host);
    let mut cy = 0.0;
    key(
        &mut c,
        &states["key.forward"],
        [K + GAP, cy, K, K],
        pressed,
        None,
        None,
    );
    cy += K + GAP;
    for (i, m) in ["key.left", "key.back", "key.right"]
        .into_iter()
        .enumerate()
    {
        key(
            &mut c,
            &states[m],
            [i as f32 * (K + GAP), cy, K, K],
            pressed,
            None,
            None,
        );
    }
    cy += K + GAP;
    if mouse {
        let half = ((full - GAP) / 2.0).floor();
        let (l, r) = (cps.then_some(counts.0), cps.then_some(counts.1));
        key(
            &mut c,
            &states["key.attack"],
            [0.0, cy, half, K],
            pressed,
            None,
            l,
        );
        key(
            &mut c,
            &states["key.use"],
            [half + GAP, cy, full - half - GAP, K],
            pressed,
            None,
            r,
        );
        cy += K + GAP;
    }
    if space {
        key(
            &mut c,
            &states["key.jump"],
            [0.0, cy, full, 12.0],
            pressed,
            Some("———"),
            None,
        );
        cy += 12.0 + GAP;
    }
    if sneak {
        key(
            &mut c,
            &states["key.sneak"],
            [0.0, cy, full, 12.0],
            pressed,
            None,
            None,
        );
        cy += 12.0 + GAP;
    }
    Some(HudBox {
        corner: HudCorner::BottomLeft,
        w: full,
        h: cy - GAP,
        ops: c.ops,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};
    use crate::omnitool_hud::HudOp;

    fn host() -> Fake {
        restart();
        let mut h = Fake::default();
        core::set_enabled(&mut h, &MODULE, true);
        for (m, label) in [
            ("key.forward", "W"),
            ("key.left", "A"),
            ("key.back", "S"),
            ("key.right", "D"),
            ("key.attack", "Left Button"),
            ("key.use", "Right Button"),
            ("key.jump", "Space"),
            ("key.sneak", "Left Shift"),
        ] {
            h.mappings.push((m.into(), false, label.into()));
        }
        h
    }

    #[test]
    fn labels_cut_at_five() {
        assert_eq!(label("Left Button"), "Left ");
        assert_eq!(label("W"), "W");
    }

    #[test]
    fn default_layout_size() {
        let mut h = host();
        let b = hud(&mut h).unwrap();
        // 20+2 + 20+2 + 20+2 + 12 (jump bar) = 78
        assert_eq!((b.w, b.h), (64.0, 78.0));
    }

    #[test]
    fn pressed_key_and_cps() {
        let mut h = host();
        h.mappings[0].1 = true;
        h.mappings[4].1 = true;
        let b = hud(&mut h).unwrap();
        assert!(matches!(
            b.ops[0],
            HudOp::Fill {
                argb: 0xC0FF_FFFF,
                ..
            }
        ));
        let texts: Vec<&str> = b
            .ops
            .iter()
            .filter_map(|o| match o {
                HudOp::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&"1 CPS") && texts.contains(&"0 CPS"));
        assert!(texts.contains(&"———"));
    }

    #[test]
    fn options_change_height() {
        let mut h = host();
        core::with(&mut h, |s, _| {
            let v = &mut s.state_mut(&MODULE).values;
            v[0] = Value::Bool(false); // mouse
            v[3] = Value::Bool(true); // sneak
        });
        let b = hud(&mut h).unwrap();
        // 22 + 22 + 14 (jump) + 12 (sneak) = 70
        assert_eq!(b.h, 70.0);
    }
}
