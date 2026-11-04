// World ranges (docs/spec/omnitool/world-ranges.md): Spawn Spheres,
// Spawner Range and Beacon & Conduit Range. Three modules in one file; they
// share the ring geometry (`circle`, `sphere`, §0) and draw through the world
// lines. The glue feeds the tick with the block entities of the loaded
// chunks around the player and a block lookup. Added to the game as
// `game::omnitool_world_ranges`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, NONE, SettingDef};
use super::omnitool_world_lines::{Frame, Lines, color};
use std::cell::RefCell;

const fn module(
    id: &'static str,
    name: &'static str,
    description: &'static str,
    settings: &'static [SettingDef],
) -> ModuleInfo {
    ModuleInfo {
        id,
        name,
        category: Category::World,
        description,
        default_on: false,
        cheaty: false,
        beta: false,
        fair_variant: false,
        replaces: "",
        settings,
        command: None,
        key: NONE,
        hold: false,
        hud: None,
        tick: None,
        on_key: None,
        hud_right: None,
        on_action: None,
    }
}

pub const SPAWN_SPHERES: ModuleInfo = module(
    "spawn_spheres",
    "Spawn Spheres",
    "24 = no spawns closer, 32 = random despawn, 128 = instant despawn. Fixed center = where you enabled it (AFK spot).",
    &[
        SettingDef::new(
            "center",
            "Center",
            Kind::Enum {
                values: &["PLAYER", "FIXED"],
                default: 1,
            },
        ),
        SettingDef::new("near", "24 block sphere", Kind::Bool(true)),
        SettingDef::new("random_despawn", "32 block circle", Kind::Bool(false)),
        SettingDef::new("far", "128 block circle", Kind::Bool(true)),
        SettingDef::new("near_color", "24 color", Kind::Color(0xFFFF_6060u32 as i32)),
        SettingDef::new(
            "far_color",
            "32/128 color",
            Kind::Color(0xFF60_A0FFu32 as i32),
        ),
    ],
);

pub const SPAWNER_RANGE: ModuleInfo = module(
    "spawner_range",
    "Spawner Range",
    "Shows the 16 block activation sphere and the spawn area of nearby spawners.",
    &[
        SettingDef::new(
            "chunks",
            "Search radius (chunks)",
            Kind::Number {
                default: 4.0,
                min: 1.0,
                max: 12.0,
                step: 1.0,
            },
        ),
        SettingDef::new("activation", "Activation sphere (16)", Kind::Bool(true)),
        SettingDef::new(
            "activation_color",
            "Sphere color",
            Kind::Color(0xFFFF_40FFu32 as i32),
        ),
        SettingDef::new("spawn_area", "Spawn area (9x3x9)", Kind::Bool(true)),
        SettingDef::new(
            "area_color",
            "Spawn area color",
            Kind::Color(0xFFFF_A020u32 as i32),
        ),
    ],
);

pub const BEACON_RANGE: ModuleInfo = module(
    "beacon_range",
    "Beacon & Conduit Range",
    "Shows the effect range of beacons and conduits.",
    &[
        SettingDef::new(
            "chunks",
            "Search radius (chunks)",
            Kind::Number {
                default: 6.0,
                min: 1.0,
                max: 12.0,
                step: 1.0,
            },
        ),
        SettingDef::new("beacons", "Beacons", Kind::Bool(true)),
        SettingDef::new(
            "beacon_color",
            "Beacon color",
            Kind::Color(0xFF40_FFFFu32 as i32),
        ),
        SettingDef::new("conduits", "Conduits", Kind::Bool(true)),
        SettingDef::new(
            "conduit_color",
            "Conduit color",
            Kind::Color(0xFF40_80FFu32 as i32),
        ),
    ],
);

// ------------------------------------------------------------ geometry (§0)

/// `(int) max(24, min(128, r * 4))`.
pub fn segments(r: f64) -> usize {
    (r * 4.0).clamp(24.0, 128.0) as usize
}

/// `circle`: a horizontal ring at `y`.
pub fn circle(out: &mut Lines, c: [f64; 3], r: f64, argb: u32) {
    let n = segments(r);
    let mut prev = [c[0] + r, c[1], c[2]];
    for i in 1..=n {
        let a = std::f64::consts::TAU * i as f64 / n as f64;
        let p = [c[0] + a.cos() * r, c[1], c[2] + a.sin() * r];
        out.line(prev, p, argb);
        prev = p;
    }
}

/// `sphere`: three rings (XZ, YZ, XY) through the centre.
pub fn sphere(out: &mut Lines, c: [f64; 3], r: f64, argb: u32) {
    circle(out, c, r, argb);
    let n = segments(r);
    let tau = std::f64::consts::TAU;
    let mut prev = [c[0], c[1], c[2] + r];
    for i in 1..=n {
        let a = tau * i as f64 / n as f64;
        let p = [c[0], c[1] + a.sin() * r, c[2] + a.cos() * r];
        out.line(prev, p, argb);
        prev = p;
    }
    let mut prev = [c[0] + r, c[1], c[2]];
    for i in 1..=n {
        let a = tau * i as f64 / n as f64;
        let p = [c[0] + a.cos() * r, c[1] + a.sin() * r, c[2]];
        out.line(prev, p, argb);
        prev = p;
    }
}

// ------------------------------------------------------------ Spawn Spheres

#[derive(Default)]
struct Spheres {
    /// The switch seen last tick (None before the first tick, so a config
    /// loaded with the module on keeps (0,0,0) like Java).
    was_on: Option<bool>,
    fixed: [f64; 3],
}

thread_local! {
    static SPHERES: RefCell<Spheres> = RefCell::new(Spheres::default());
}

/// Java's `onEnable`: switching the module on stores the eye position.
pub fn spheres_tick(host: &dyn Host, eye: Option<[f64; 3]>) {
    let on = core::active(host, &SPAWN_SPHERES);
    SPHERES.with_borrow_mut(|s| {
        if s.was_on == Some(false)
            && on
            && let Some(e) = eye
        {
            s.fixed = e;
        }
        s.was_on = Some(on);
    });
}

pub fn draw_spheres(host: &dyn Host, frame: &Frame, out: &mut Lines) {
    let v = |id| core::value(host, &SPAWN_SPHERES, id);
    let c = if v("center").index() == 0 {
        let Some(e) = frame.eye else {
            return;
        };
        e
    } else {
        SPHERES.with_borrow(|s| s.fixed)
    };
    let (near, far) = (color(v("near_color")), color(v("far_color")));
    out.width = 2.0;
    if v("near").on() {
        sphere(out, c, 24.0, near);
    }
    if v("random_despawn").on() {
        circle(out, c, 32.0, far);
    }
    if v("far").on() {
        circle(out, c, 128.0, far);
    }
}

// ------------------------------------------------------------ scans

/// A block entity the scan looks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Found {
    Spawner,
    Beacon,
    Conduit,
}

/// What the ranges' tick reads from the world: the block entities of the
/// loaded chunks within `chunks` of the player's chunk (§1), and block ids.
pub trait RangeWorld {
    fn block_entities(&self, chunks: i32) -> Vec<([i32; 3], Found)>;
    /// The block id (`minecraft:iron_block`); air when not loaded.
    fn block(&self, pos: [i32; 3]) -> String;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub pos: [i32; 3],
    pub range: i32,
    pub beacon: bool,
}

#[derive(Default)]
struct Scans {
    spawner_timer: i32,
    spawners: Vec<[i32; 3]>,
    beacon_timer: i32,
    areas: Vec<Area>,
}

thread_local! {
    static SCANS: RefCell<Scans> = RefCell::new(Scans::default());
}

/// `timer-- > 0 ? wait : (timer = 40, scan)`: every 41 ticks, first at once.
fn due(timer: &mut i32) -> bool {
    *timer -= 1;
    if *timer + 1 > 0 {
        return false;
    }
    *timer = 40;
    true
}

/// The ticks of Spawner Range and Beacon Range (only while active; the old
/// lists stay when off).
pub fn ranges_tick(host: &dyn Host, world: &dyn RangeWorld) {
    if core::active(host, &SPAWNER_RANGE) && SCANS.with_borrow_mut(|s| due(&mut s.spawner_timer)) {
        let r = core::value(host, &SPAWNER_RANGE, "chunks").num() as i32;
        let found = world
            .block_entities(r)
            .into_iter()
            .filter(|f| f.1 == Found::Spawner)
            .map(|f| f.0)
            .collect();
        SCANS.with_borrow_mut(|s| s.spawners = found);
    }
    if core::active(host, &BEACON_RANGE) && SCANS.with_borrow_mut(|s| due(&mut s.beacon_timer)) {
        let v = |id| core::value(host, &BEACON_RANGE, id);
        let (beacons, conduits) = (v("beacons").on(), v("conduits").on());
        let mut found = Vec::new();
        for (pos, kind) in world.block_entities(v("chunks").num() as i32) {
            match kind {
                Found::Beacon if beacons => {
                    if let Some(range) = beacon_range(beacon_levels(world, pos)) {
                        found.push(Area {
                            pos,
                            range,
                            beacon: true,
                        });
                    }
                }
                Found::Conduit if conduits => {
                    if let Some(range) = conduit_range(conduit_frame(world, pos)) {
                        found.push(Area {
                            pos,
                            range,
                            beacon: false,
                        });
                    }
                }
                _ => {}
            }
        }
        SCANS.with_borrow_mut(|s| s.areas = found);
    }
}

const BEACON_BASE: [&str; 5] = [
    "minecraft:iron_block",
    "minecraft:gold_block",
    "minecraft:diamond_block",
    "minecraft:emerald_block",
    "minecraft:netherite_block",
];

const CONDUIT_FRAME: [&str; 4] = [
    "minecraft:prismarine",
    "minecraft:prismarine_bricks",
    "minecraft:sea_lantern",
    "minecraft:dark_prismarine",
];

/// `beaconLevels`: complete pyramid layers below the beacon (0..4).
pub fn beacon_levels(world: &dyn RangeWorld, p: [i32; 3]) -> i32 {
    let mut levels = 0;
    for l in 1..=4 {
        let y = p[1] - l;
        for x in p[0] - l..=p[0] + l {
            for z in p[2] - l..=p[2] + l {
                if !BEACON_BASE.contains(&world.block([x, y, z]).as_str()) {
                    return levels;
                }
            }
        }
        levels = l;
    }
    levels
}

pub fn beacon_range(levels: i32) -> Option<i32> {
    (levels > 0).then_some(levels * 10 + 10)
}

/// `conduitFrame`: frame blocks on the three frame planes of the 5x5x5 cube.
pub fn conduit_frame(world: &dyn RangeWorld, p: [i32; 3]) -> i32 {
    let mut n = 0;
    for i in -2i32..=2 {
        for j in -2i32..=2 {
            for k in -2i32..=2 {
                let outer = i.abs() > 1 || j.abs() > 1 || k.abs() > 1;
                let plane = (i == 0 && (j.abs() == 2 || k.abs() == 2))
                    || (j == 0 && (i.abs() == 2 || k.abs() == 2))
                    || (k == 0 && (i.abs() == 2 || j.abs() == 2));
                if outer
                    && plane
                    && CONDUIT_FRAME.contains(&world.block([p[0] + i, p[1] + j, p[2] + k]).as_str())
                {
                    n += 1;
                }
            }
        }
    }
    n
}

pub fn conduit_range(frame: i32) -> Option<i32> {
    (frame >= 16).then_some(frame / 7 * 16)
}

/// The 9x3x9 spawn area of a spawner.
pub fn spawner_box(p: [i32; 3]) -> ([f64; 3], [f64; 3]) {
    let [x, y, z] = p.map(f64::from);
    ([x - 4.0, y - 1.0, z - 4.0], [x + 5.0, y + 2.0, z + 5.0])
}

/// A beacon's box: ±r around the block, from `y - r` up to Y 320.
pub fn beacon_box(p: [i32; 3], r: i32) -> ([f64; 3], [f64; 3]) {
    let [x, y, z] = p.map(f64::from);
    let r = f64::from(r);
    ([x - r, y - r, z - r], [x + r + 1.0, 320.0, z + r + 1.0])
}

fn center(p: [i32; 3]) -> [f64; 3] {
    p.map(|v| f64::from(v) + 0.5)
}

pub fn draw_spawners(host: &dyn Host, _frame: &Frame, out: &mut Lines) {
    let v = |id| core::value(host, &SPAWNER_RANGE, id);
    let sphere_c = v("activation").on().then(|| color(v("activation_color")));
    let area_c = v("spawn_area").on().then(|| color(v("area_color")));
    out.width = 2.0;
    SCANS.with_borrow(|s| {
        for &p in &s.spawners {
            if let Some(c) = sphere_c {
                sphere(out, center(p), 16.0, c);
            }
            if let Some(c) = area_c {
                let (min, max) = spawner_box(p);
                out.cuboid(min, max, c);
            }
        }
    });
}

pub fn draw_beacons(host: &dyn Host, _frame: &Frame, out: &mut Lines) {
    let v = |id| core::value(host, &BEACON_RANGE, id);
    let (bc, cc) = (color(v("beacon_color")), color(v("conduit_color")));
    out.width = 2.0;
    SCANS.with_borrow(|s| {
        for a in &s.areas {
            if a.beacon {
                let (min, max) = beacon_box(a.pos, a.range);
                out.cuboid(min, max, bc);
            } else {
                sphere(out, center(a.pos), f64::from(a.range), cc);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};
    use serde_json::Value;
    use std::collections::HashMap;

    fn fixtures() -> Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/world-ranges.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn arr(v: &Value) -> [f64; 3] {
        std::array::from_fn(|i| v[i].as_f64().unwrap())
    }

    fn ipos(v: &Value) -> [i32; 3] {
        std::array::from_fn(|i| v[i].as_i64().unwrap() as i32)
    }

    #[test]
    fn formula_fixtures() {
        let f = fixtures();
        for c in f["segments"].as_array().unwrap() {
            assert_eq!(
                segments(c["radius"].as_f64().unwrap()),
                c["segments"].as_u64().unwrap() as usize
            );
        }
        for c in f["beacon_range"].as_array().unwrap() {
            let want = c["range"].as_i64().map(|v| v as i32);
            assert_eq!(beacon_range(c["levels"].as_i64().unwrap() as i32), want);
        }
        for c in f["conduit_range"].as_array().unwrap() {
            let want = c["range"].as_i64().map(|v| v as i32);
            assert_eq!(conduit_range(c["frame"].as_i64().unwrap() as i32), want);
        }
        let b = &f["beacon_box"];
        let r = beacon_range(b["levels"].as_i64().unwrap() as i32).unwrap();
        assert_eq!(
            beacon_box(ipos(&b["pos"]), r),
            (arr(&b["min"]), arr(&b["max"]))
        );
        let s = &f["spawner_box"];
        let p = ipos(&s["pos"]);
        assert_eq!(spawner_box(p), (arr(&s["min"]), arr(&s["max"])));
        assert_eq!(center(p), arr(&s["sphere_center"]));
    }

    #[test]
    fn ring_line_counts() {
        let mut l = Lines::default();
        circle(&mut l, [0.0; 3], 16.0, 1);
        assert_eq!(l.lines.len(), 64);
        // closed: the last point is the start
        let last = l.lines.last().unwrap().end;
        assert!((last[0] - 16.0).abs() < 1e-9 && last[2].abs() < 1e-9);
        let mut l = Lines::default();
        sphere(&mut l, [0.0; 3], 24.0, 1);
        assert_eq!(l.lines.len(), 3 * 96);
    }

    struct World {
        blocks: HashMap<[i32; 3], &'static str>,
        found: Vec<([i32; 3], Found)>,
    }

    impl RangeWorld for World {
        fn block_entities(&self, _chunks: i32) -> Vec<([i32; 3], Found)> {
            self.found.clone()
        }
        fn block(&self, pos: [i32; 3]) -> String {
            self.blocks
                .get(&pos)
                .copied()
                .unwrap_or("minecraft:air")
                .to_owned()
        }
    }

    #[test]
    fn beacon_pyramid_and_conduit_frame() {
        let mut w = World {
            blocks: HashMap::new(),
            found: vec![],
        };
        // two complete layers below (0, 70, 0)
        for l in 1..=2 {
            for x in -l..=l {
                for z in -l..=l {
                    w.blocks.insert([x, 70 - l, z], "minecraft:iron_block");
                }
            }
        }
        assert_eq!(beacon_levels(&w, [0, 70, 0]), 2);
        w.blocks.insert([1, 69, 1], "minecraft:stone");
        assert_eq!(beacon_levels(&w, [0, 70, 0]), 0);
        // a full conduit frame has 42 blocks
        let mut c = World {
            blocks: HashMap::new(),
            found: vec![],
        };
        for i in -2..=2 {
            for j in -2..=2 {
                for k in -2..=2 {
                    c.blocks.insert([i, 50 + j, k], "minecraft:prismarine");
                }
            }
        }
        assert_eq!(conduit_frame(&c, [0, 50, 0]), 42);
    }

    #[test]
    fn ticks_scan_every_41_ticks() {
        let mut h = Fake::default();
        restart();
        SCANS.take();
        core::with(&mut h, |s, _| s.state_mut(&SPAWNER_RANGE).enabled = true);
        let w = World {
            blocks: HashMap::new(),
            found: vec![([10, 64, -3], Found::Spawner), ([0, 0, 0], Found::Beacon)],
        };
        ranges_tick(&h, &w);
        assert_eq!(
            SCANS.with_borrow(|s| s.spawners.clone()),
            vec![[10, 64, -3]]
        );
        let w2 = World {
            blocks: HashMap::new(),
            found: vec![],
        };
        for _ in 0..40 {
            ranges_tick(&h, &w2);
        }
        assert_eq!(SCANS.with_borrow(|s| s.spawners.len()), 1);
        ranges_tick(&h, &w2);
        assert!(SCANS.with_borrow(|s| s.spawners.is_empty()));
        // beacon without pyramid: no area
        core::with(&mut h, |s, _| s.state_mut(&BEACON_RANGE).enabled = true);
        ranges_tick(&h, &w);
        assert!(SCANS.with_borrow(|s| s.areas.is_empty()));
    }

    #[test]
    fn spheres_fixed_center_on_enable() {
        let mut h = Fake::default();
        restart();
        SPHERES.take();
        spheres_tick(&h, Some([1.0, 2.0, 3.0]));
        core::with(&mut h, |s, _| s.state_mut(&SPAWN_SPHERES).enabled = true);
        spheres_tick(&h, Some([5.0, 66.6, 7.0]));
        assert_eq!(SPHERES.with_borrow(|s| s.fixed), [5.0, 66.6, 7.0]);
        let mut l = Lines::default();
        draw_spheres(&h, &Frame::default(), &mut l);
        // 24 sphere (3 x 96) + 128 ring (128)
        assert_eq!(l.lines.len(), 3 * 96 + 128);
        assert_eq!(l.lines[0].start, [29.0, 66.6, 7.0]);
    }

    #[test]
    fn defaults() {
        assert_eq!(SPAWN_SPHERES.settings.len(), 6);
        assert_eq!(SPAWNER_RANGE.settings.len(), 5);
        assert_eq!(BEACON_RANGE.settings.len(), 5);
    }
}
