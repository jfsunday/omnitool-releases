// Chunk Borders (docs/spec/omnitool/world-render.md §2): chunk corner
// columns around the player, a grid on the current chunk and, with a seed,
// slime chunks. Drawn through the world lines. Added to the game as
// `game::omnitool_chunk_borders`; also a module of this crate.
//
// The slime seed (§6.4): singleplayer's world seed, else this module's
// `seed` setting (Slime Chunks, the first fallback, is not ported yet).
// Deviation: the slime fill is drawn as the chunk's outline (no quads).

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_overlay_entries::is_slime_chunk;
use super::omnitool_setting::{Kind, NONE, SettingDef};
use super::omnitool_world_lines::{Frame, Lines, color};

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "chunk_borders",
    name: "Chunk Borders",
    category: Category::Render,
    description: "Chunk borders like F3+G (no seed needed), optionally with slime chunks.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "",
    settings: &[
        SettingDef::new(
            "radius",
            "Neighbour chunks",
            Kind::Number {
                default: 2.0,
                min: 0.0,
                max: 6.0,
                step: 1.0,
            },
        ),
        SettingDef::new("color", "Border color", Kind::Color(0xFFFF_FF40u32 as i32)),
        SettingDef::new("grid", "Grid on current chunk", Kind::Bool(true)),
        SettingDef::new(
            "grid_color",
            "Grid color",
            Kind::Color(0x8040_C0FFu32 as i32),
        ),
        SettingDef::new(
            "slime",
            "Show slime chunks (servers: needs the world seed)",
            Kind::Bool(false),
        ),
        SettingDef::new(
            "seed",
            "World seed (only for slime chunks on servers)",
            Kind::Text(""),
        ),
        SettingDef::new(
            "slime_color",
            "Slime chunk color",
            Kind::Color(0x5040_FF40u32 as i32),
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

/// Java's `SlimeChunk.parseSeed`: trimmed; empty is none; a Java long, else
/// the text's `String.hashCode` (like the Create World screen).
pub fn parse_seed(text: &str) -> Option<i64> {
    let v = text.trim();
    if v.is_empty() {
        return None;
    }
    if let Ok(n) = v.parse::<i64>() {
        return Some(n);
    }
    let mut h: i32 = 0;
    for c in v.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(i32::from(c));
    }
    Some(i64::from(h))
}

/// The slime seed: the singleplayer world's, else the `seed` setting's.
pub fn slime_seed(host: &dyn Host, frame: &Frame) -> Option<i64> {
    frame
        .seed
        .or_else(|| parse_seed(core::value(host, &MODULE, "seed").text()))
}

pub struct Options {
    pub radius: i32,
    pub color: u32,
    pub grid: Option<u32>,
    /// The seed and colour, when slime chunks are shown in this dimension.
    pub slime: Option<(i64, u32)>,
}

/// Java's `renderWorld`.
pub fn draw(host: &dyn Host, frame: &Frame, out: &mut Lines) {
    let Some(pos) = frame.player else {
        return;
    };
    let v = |id| core::value(host, &MODULE, id);
    let slime = if v("slime").on() && frame.dim.contains("overworld") {
        slime_seed(host, frame).map(|s| (s, color(v("slime_color"))))
    } else {
        None
    };
    let o = Options {
        radius: v("radius").num() as i32,
        color: color(v("color")),
        grid: v("grid").on().then(|| color(v("grid_color"))),
        slime,
    };
    lines(&o, pos, out);
}

/// The geometry around the player at `pos`.
pub fn lines(o: &Options, pos: [f64; 3], out: &mut Lines) {
    let cx = (pos[0].floor() as i32) >> 4;
    let cz = (pos[2].floor() as i32) >> 4;
    let py = pos[1];
    let (y1, y2) = (py - 32.0, py + 32.0);
    let r = o.radius;
    out.width = 1.5;
    for x in cx - r..=cx + r + 1 {
        for z in cz - r..=cz + r + 1 {
            let (bx, bz) = (f64::from(16 * x), f64::from(16 * z));
            out.line([bx, y1, bz], [bx, y2, bz], o.color);
        }
    }
    if let Some(gc) = o.grid {
        let (bx, bz) = (f64::from(16 * cx), f64::from(16 * cz));
        for i in (0..=16).step_by(2) {
            let i = f64::from(i);
            out.line([bx + i, y1, bz], [bx + i, y2, bz], gc);
            out.line([bx + i, y1, bz + 16.0], [bx + i, y2, bz + 16.0], gc);
            out.line([bx, y1, bz + i], [bx, y2, bz + i], gc);
            out.line([bx + 16.0, y1, bz + i], [bx + 16.0, y2, bz + i], gc);
        }
        let mut y = y1.floor();
        while y <= y2 {
            out.line([bx, y, bz], [bx + 16.0, y, bz], gc);
            out.line([bx, y, bz + 16.0], [bx + 16.0, y, bz + 16.0], gc);
            out.line([bx, y, bz], [bx, y, bz + 16.0], gc);
            out.line([bx + 16.0, y, bz], [bx + 16.0, y, bz + 16.0], gc);
            y += 4.0;
        }
    }
    if let Some((seed, sc)) = o.slime {
        let sr = r.max(4);
        let fy = py.floor() + 0.05;
        for x in cx - sr..=cx + sr {
            for z in cz - sr..=cz + sr {
                if is_slime_chunk(seed, x, z) {
                    let (x1, z1) = (f64::from(16 * x), f64::from(16 * z));
                    out.flat(x1, z1, x1 + 16.0, z1 + 16.0, fy, sc);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn parse_seed_fixtures() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/slime-chunk.json"
        );
        let f: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let cases = f["parse_seed"].as_array().unwrap();
        assert!(cases.len() > 5);
        for c in cases {
            let input = c["input"].as_str().unwrap();
            assert_eq!(parse_seed(input), c["output"].as_i64(), "{input:?}");
        }
    }

    fn opts(radius: i32, grid: bool) -> Options {
        Options {
            radius,
            color: 1,
            grid: grid.then_some(2),
            slime: None,
        }
    }

    #[test]
    fn corner_columns() {
        let mut l = Lines::default();
        lines(&opts(2, false), [-0.5, 70.0, 20.0], &mut l);
        assert_eq!(l.lines.len(), 36);
        assert!(
            l.lines
                .iter()
                .all(|x| x.width == 1.5 && x.start[1] == 38.0 && x.end[1] == 102.0)
        );
        // player in chunk (-1, 1): columns from x = -48 to 32
        let xs: Vec<f64> = l.lines.iter().map(|x| x.start[0]).collect();
        assert_eq!(xs.iter().cloned().fold(f64::MAX, f64::min), -48.0);
        assert_eq!(xs.iter().cloned().fold(f64::MIN, f64::max), 32.0);
        let mut l = Lines::default();
        lines(&opts(0, false), [5.0, 0.0, 5.0], &mut l);
        assert_eq!(l.lines.len(), 4);
    }

    #[test]
    fn grid() {
        let mut l = Lines::default();
        lines(&opts(0, true), [5.0, 64.5, 5.0], &mut l);
        // 4 columns + 36 grid uprights + rings from floor(32.5)=32 to 96.5 every 4: 17 rings x 4
        assert_eq!(l.lines.len(), 4 + 36 + 17 * 4);
        let ring = &l.lines[40];
        assert_eq!(ring.start, [0.0, 32.0, 0.0]);
    }

    #[test]
    fn slime_chunks_follow_formula() {
        let mut l = Lines::default();
        let o = Options {
            radius: 0,
            color: 1,
            grid: None,
            slime: Some((0, 3)),
        };
        lines(&o, [0.0, 64.7, 0.0], &mut l);
        let flats = l.lines.iter().filter(|x| x.color == 3).count();
        let want = (-4..=4)
            .flat_map(|x| (-4..=4).map(move |z| (x, z)))
            .filter(|&(x, z)| is_slime_chunk(0, x, z))
            .count();
        assert_eq!(flats, want * 4);
        assert!(
            l.lines
                .iter()
                .filter(|x| x.color == 3)
                .all(|x| x.start[1] == 64.05)
        );
    }
}
