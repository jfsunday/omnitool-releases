// Light Overlay (docs/spec/omnitool/world-render.md §1): marks the blocks a
// zombie could spawn on. The scan runs in the tick (the glue passes a probe
// that applies Horizon's spawn rules and reads the light); the frame draws
// the spots through the world lines. `spawn_kind` is the one spawn-light
// rule, shared with Omni Overlay's `spawn_here` (§1.4). Added to the game
// as `game::omnitool_light_overlay`; also a module of this crate.
//
// Deviation: Horizon's gizmos draw no quads, so FILL draws the block's
// square outline (alpha fixed at 0x50 like Java's fill) instead of a
// filled square.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, NONE, SettingDef};
use super::omnitool_world_lines::{Frame, Lines, color};
use std::cell::RefCell;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "light_overlay",
    name: "Light Overlay",
    category: Category::Render,
    description: "Red = too dark, mobs can spawn. Green = lit. Detailed colors: yellow = only at night (open sky).",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "Light Overlay",
    settings: &[
        SettingDef::new(
            "radius",
            "Horizontal radius",
            Kind::Number {
                default: 16.0,
                min: 4.0,
                max: 48.0,
                step: 1.0,
            },
        ),
        SettingDef::new(
            "vertical",
            "Vertical range",
            Kind::Number {
                default: 8.0,
                min: 2.0,
                max: 32.0,
                step: 1.0,
            },
        ),
        SettingDef::new(
            "style",
            "Style",
            Kind::Enum {
                values: &["CROSS", "FILL", "BOTH"],
                default: 0,
            },
        ),
        SettingDef::new(
            "colors",
            "Colors (Simple = like Light Overlay mod)",
            Kind::Enum {
                values: &["SIMPLE", "DETAILED"],
                default: 0,
            },
        ),
        SettingDef::new(
            "show_night",
            "Show night-only spots (yellow)",
            Kind::Bool(true),
        ),
        SettingDef::new("show_safe", "Show lit spots (green)", Kind::Bool(true)),
        SettingDef::new(
            "always_color",
            "Mobs can spawn color",
            Kind::Color(0xFFFF_0000u32 as i32),
        ),
        SettingDef::new(
            "night_color",
            "Night only color",
            Kind::Color(0xFFFF_D020u32 as i32),
        ),
        SettingDef::new(
            "safe_color",
            "Lit (safe) color",
            Kind::Color(0xFF00_FF00u32 as i32),
        ),
        SettingDef::new(
            "interval",
            "Update every N ticks",
            Kind::Number {
                default: 5.0,
                min: 1.0,
                max: 40.0,
                step: 1.0,
            },
        ),
    ],
    command: None,
    key: NONE,
    hold: false,
    hud: None,
    tick: None,
    on_key: None,
    hud_right: None,
    on_action: None,
};

/// How a spawnable block is lit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnKind {
    /// Block light above the limit: no monster spawns (green).
    Lit,
    /// Dark for blocks but under open sky: only at night (yellow).
    Night,
    /// Monsters can spawn (red).
    Spawn,
}

/// The shared spawn-light rule (Light Overlay `scan`, Omni Overlay
/// `spawn_here`): block light above the limit (11 in the Nether, else 0)
/// is lit; else, where night spawns are told apart (`night`), sky light
/// above 7 means only at night; else monsters spawn.
pub fn spawn_kind(block_light: i32, sky_light: i32, nether: bool, night: bool) -> SpawnKind {
    let limit = if nether { 11 } else { 0 };
    if block_light > limit {
        SpawnKind::Lit
    } else if night && sky_light > 7 {
        SpawnKind::Night
    } else {
        SpawnKind::Spawn
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spot {
    pub pos: [i32; 3],
    pub kind: SpawnKind,
}

/// What the scan needs at a block: None when no zombie could spawn there
/// (`isValidSpawn` below, `isValidEmptySpawnBlock` here), else its
/// (block light, sky light).
pub type Probe<'a> = dyn FnMut([i32; 3]) -> Option<(i32, i32)> + 'a;

#[derive(Default)]
struct Scan {
    timer: i32,
    spots: Vec<Spot>,
}

thread_local! {
    static SCAN: RefCell<Scan> = RefCell::new(Scan::default());
}

/// Java's `tick`, each client tick with a player in a world: inactive
/// clears the spots (the timer stands still); otherwise a scan now and
/// then every `interval + 1` ticks.
pub fn tick(host: &dyn Host, center: [i32; 3], dim: &str, probe: &mut Probe<'_>) {
    if !core::active(host, &MODULE) {
        SCAN.with_borrow_mut(|s| s.spots.clear());
        return;
    }
    let due = SCAN.with_borrow_mut(|s| {
        s.timer -= 1;
        if s.timer + 1 > 0 {
            return false;
        }
        s.timer = core::value(host, &MODULE, "interval").num() as i32;
        true
    });
    if due {
        let spots = scan(host, center, dim, probe);
        SCAN.with_borrow_mut(|s| s.spots = spots);
    }
}

/// Java's `scan` around `center` (the player's block).
pub fn scan(host: &dyn Host, center: [i32; 3], dim: &str, probe: &mut Probe<'_>) -> Vec<Spot> {
    let v = |id| core::value(host, &MODULE, id);
    let r = v("radius").num() as i32;
    let vr = v("vertical").num() as i32;
    let detailed = v("colors").index() == 1;
    let show_night = v("show_night").on();
    let show_safe = v("show_safe").on();
    let nether = dim.contains("the_nether");
    let no_sky = nether || dim.contains("the_end");
    let mut out = Vec::new();
    for dx in -r..=r {
        for dz in -r..=r {
            for dy in -vr..=vr {
                let p = [center[0] + dx, center[1] + dy, center[2] + dz];
                let Some((block, sky)) = probe(p) else {
                    continue;
                };
                let kind = spawn_kind(block, sky, nether, detailed && !no_sky);
                let shown = match kind {
                    SpawnKind::Lit => show_safe,
                    SpawnKind::Night => show_night,
                    SpawnKind::Spawn => true,
                };
                if shown {
                    out.push(Spot { pos: p, kind });
                }
            }
        }
    }
    out
}

/// The spots of the last scan.
#[cfg(test)]
pub fn spots() -> Vec<Spot> {
    SCAN.with_borrow(|s| s.spots.clone())
}

/// Java's `renderWorld`: a cross and/or the fill square per spot, 0.02
/// above the floor, width 1.5.
pub fn draw(host: &dyn Host, _frame: &Frame, out: &mut Lines) {
    let v = |id| core::value(host, &MODULE, id);
    let style = v("style").index();
    let (cross, fill) = (style != 1, style != 0);
    let colors = [
        color(v("always_color")),
        color(v("night_color")),
        color(v("safe_color")),
    ];
    out.width = 1.5;
    SCAN.with_borrow(|s| {
        for spot in &s.spots {
            draw_spot(out, spot, colors, cross, fill);
        }
    });
}

fn draw_spot(out: &mut Lines, spot: &Spot, colors: [u32; 3], cross: bool, fill: bool) {
    let c = match spot.kind {
        SpawnKind::Spawn => colors[0],
        SpawnKind::Night => colors[1],
        SpawnKind::Lit => colors[2],
    };
    let [x, y, z] = spot.pos.map(f64::from);
    let y = y + 0.02;
    if cross {
        out.line([x + 0.1, y, z + 0.1], [x + 0.9, y, z + 0.9], c);
        out.line([x + 0.9, y, z + 0.1], [x + 0.1, y, z + 0.9], c);
    }
    if fill {
        out.flat(x, z, x + 1.0, z + 1.0, y, (c & 0x00FF_FFFF) | 0x5000_0000);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};
    use crate::omnitool_setting::Value;

    fn on(host: &mut Fake) {
        core::with(host, |s, _| s.state_mut(&MODULE).enabled = true);
    }

    #[test]
    fn spawn_rule() {
        assert_eq!(spawn_kind(1, 0, false, true), SpawnKind::Lit);
        assert_eq!(spawn_kind(0, 15, false, true), SpawnKind::Night);
        assert_eq!(spawn_kind(0, 15, false, false), SpawnKind::Spawn);
        assert_eq!(spawn_kind(0, 7, false, true), SpawnKind::Spawn);
        // Nether: block light up to 11 still spawns
        assert_eq!(spawn_kind(11, 0, true, false), SpawnKind::Spawn);
        assert_eq!(spawn_kind(12, 0, true, false), SpawnKind::Lit);
    }

    #[test]
    fn defaults() {
        assert_eq!(MODULE.settings.len(), 10);
        const { assert!(!MODULE.cheaty && !MODULE.beta && !MODULE.default_on) };
    }

    /// A floor at y = 64 everywhere; lit (block light 5) for x >= 0, sky 15.
    fn floor(p: [i32; 3]) -> Option<(i32, i32)> {
        (p[1] == 65).then_some((if p[0] >= 0 { 5 } else { 0 }, 15))
    }

    #[test]
    fn scan_classifies_and_filters() {
        let mut h = Fake::default();
        restart();
        on(&mut h);
        let s = scan(&h, [0, 65, 0], "minecraft:overworld", &mut floor);
        // 33 x 33 floor blocks; x >= 0 lit (17 columns), x < 0 spawn
        assert_eq!(s.len(), 33 * 33);
        assert_eq!(
            s.iter().filter(|x| x.kind == SpawnKind::Lit).count(),
            17 * 33
        );
        assert!(s.iter().all(|x| x.kind != SpawnKind::Night));
        core::with(&mut h, |st, _| {
            let st = st.state_mut(&MODULE);
            st.values[3] = Value::Enum(1); // DETAILED
            st.values[5] = Value::Bool(false); // no lit spots
        });
        let s = scan(&h, [0, 65, 0], "minecraft:overworld", &mut floor);
        assert_eq!(s.len(), 16 * 33);
        assert!(s.iter().all(|x| x.kind == SpawnKind::Night));
        // the End has no sky: no night spots
        let s = scan(&h, [0, 65, 0], "minecraft:the_end", &mut floor);
        assert!(s.iter().all(|x| x.kind == SpawnKind::Spawn));
    }

    #[test]
    fn tick_scans_every_interval_plus_one() {
        let mut h = Fake::default();
        restart();
        SCAN.take();
        let mut calls = 0;
        let mut probe = |p: [i32; 3]| {
            calls += 1;
            floor(p)
        };
        tick(&h, [0, 65, 0], "minecraft:overworld", &mut probe);
        assert!(spots().is_empty());
        on(&mut h);
        let mut scans = 0;
        for _ in 0..13 {
            let before = calls;
            tick(&h, [0, 65, 0], "minecraft:overworld", &mut |p| {
                calls += 1;
                floor(p)
            });
            if calls > before {
                scans += 1;
            }
        }
        // ticks 0, 6, 12 scan (interval 5)
        assert_eq!(scans, 3);
        assert!(!spots().is_empty());
        core::with(&mut h, |s, _| s.state_mut(&MODULE).enabled = false);
        tick(&h, [0, 65, 0], "minecraft:overworld", &mut floor);
        assert!(spots().is_empty());
    }

    #[test]
    fn draw_cross_and_fill() {
        let spot = Spot {
            pos: [2, 64, -3],
            kind: SpawnKind::Spawn,
        };
        let mut l = Lines::default();
        draw_spot(&mut l, &spot, [0xFFFF_0000, 0, 0], true, true);
        assert_eq!(l.lines.len(), 6);
        assert_eq!(l.lines[0].start, [2.1, 64.02, -2.9]);
        assert_eq!(l.lines[0].end, [2.9, 64.02, -2.1]);
        assert_eq!(l.lines[2].color, 0x50FF_0000);
    }
}
