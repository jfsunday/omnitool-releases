// Effect Timers (docs/spec/omnitool/hud.md, section 4): active effects with
// level and remaining time. `Host::effects` has no hook yet (stub, empty), so
// in the game the box stays empty until Horizon exposes the player's effects.
// Added to the game as `game::omnitool_effect_timers`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_hud::{Canvas, Effect, HudBox, HudCorner, duration, level};
use super::omnitool_setting::{Kind, SettingDef};

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "effect_timers",
    name: "Effect Timers",
    category: Category::Hud,
    description: "Active effects with level and remaining time.",
    default_on: false,
    cheaty: false,
    beta: true,
    fair_variant: false,
    replaces: "",
    settings: &[
        SettingDef::new("background", "Background", Kind::Bool(true)),
        SettingDef::new("effect_color", "Effect colors", Kind::Bool(true)),
        SettingDef::new("sort_time", "Sort by remaining time", Kind::Bool(true)),
        SettingDef::new(
            "hide_ambient",
            "Hide beacon/conduit effects",
            Kind::Bool(false),
        ),
        SettingDef::new(
            "warn",
            "Blink below seconds (0 = off)",
            Kind::Number {
                default: 10.0,
                min: 0.0,
                max: 60.0,
                step: 1.0,
            },
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

/// One line: name, level from II up, two spaces, time.
pub fn line(e: &Effect) -> String {
    let mut name = e.name.clone();
    if e.amplifier > 0 {
        name.push(' ');
        name.push_str(&level(e.amplifier));
    }
    format!("{name}  {}", duration(e.duration.into()))
}

/// The lines with their colours at `now_ms`.
pub fn lines(
    mut effects: Vec<Effect>,
    sort_time: bool,
    effect_color: bool,
    warn: f64,
    now_ms: u64,
) -> Vec<(String, u32)> {
    if sort_time {
        // Infinite (-1) goes last; the sort is stable.
        effects.sort_by_key(|e| if e.duration < 0 { i32::MAX } else { e.duration });
    }
    effects
        .iter()
        .map(|e| {
            let base = if effect_color {
                e.color & 0x00FF_FFFF
            } else if e.harmful {
                0xFF6060
            } else {
                0xFFFFFF
            };
            let blink = e.duration >= 0
                && warn > 0.0
                && (e.duration as f64) < warn * 20.0
                && (now_ms / 250).is_multiple_of(2);
            (line(e), if blink { 0x808080 } else { base })
        })
        .collect()
}

fn hud(host: &mut dyn Host) -> Option<HudBox> {
    let background = core::value(host, &MODULE, "background").on();
    let hide = core::value(host, &MODULE, "hide_ambient").on();
    let mut list = host.effects();
    if hide {
        list.retain(|e| !e.ambient);
    }
    if list.is_empty() {
        return None;
    }
    let ls = lines(
        list,
        core::value(host, &MODULE, "sort_time").on(),
        core::value(host, &MODULE, "effect_color").on(),
        core::value(host, &MODULE, "warn").num(),
        host.now_ms(),
    );
    let mut b = Canvas::new(host).lines(HudCorner::TopLeft, &ls, background);
    // Java's box is `n * lh + 3` high, without the 10 px minimum.
    b.h = ls.len() as f32 * 10.0 + 3.0;
    Some(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eff(name: &str, amp: i32, dur: i32) -> Effect {
        Effect {
            name: name.into(),
            amplifier: amp,
            duration: dur,
            color: 0xFF33EBFF,
            harmful: false,
            ambient: false,
        }
    }

    #[test]
    fn line_format() {
        assert_eq!(line(&eff("Speed", 1, 1660)), "Speed II  1:23");
        assert_eq!(line(&eff("Night Vision", 0, 8000)), "Night Vision  6:40");
        assert_eq!(line(&eff("Fire Resistance", 0, -1)), "Fire Resistance  ∞");
    }

    #[test]
    fn sorted_by_time_infinite_last_and_blinking() {
        let v = vec![eff("A", 0, -1), eff("B", 0, 100), eff("C", 0, 5000)];
        let l = lines(v.clone(), true, true, 10.0, 0);
        let names: Vec<_> = l.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(names, ["B  0:05", "C  4:10", "A  ∞"]);
        assert_eq!(l[0].1, 0x808080); // 100 ticks < 200, phase on
        assert_eq!(l[1].1, 0x33EBFF);
        assert_eq!(lines(v.clone(), true, true, 10.0, 250)[0].1, 0x33EBFF);
        assert_eq!(lines(v, false, true, 0.0, 0)[1].1, 0x33EBFF);
    }

    #[test]
    fn plain_colors() {
        let mut bad = eff("Poison", 0, 5000);
        bad.harmful = true;
        let l = lines(vec![bad, eff("Haste", 0, 5000)], false, false, 0.0, 0);
        assert_eq!((l[0].1, l[1].1), (0xFF6060, 0xFFFFFF));
    }
}
