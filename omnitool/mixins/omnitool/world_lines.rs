// World lines (docs/spec/omnitool/world-render.md §0, Java `RenderQueue`):
// the one layer every world-drawing module hangs on. Each frame the game
// asks `draw` for its lines; it walks `DRAWERS` and lets every active module
// add lines in world coordinates. The glue turns them into Horizon gizmo
// lines (`horizon_client::gizmos::Line`, depth-tested, width in pixels).
// Added to the game as `game::omnitool_world_lines`; also a module of this
// crate.
//
// Deviation: Horizon's gizmos only draw lines, no quads. `flat` (a filled
// horizontal quad in Java) becomes the quad's outline in the fill colour,
// `filled_box` draws nothing (Java's callers always draw the box's outline
// too, or a `flat` stands for the fill).

use super::omnitool_core::{self as core, Host, ModuleInfo};
use super::omnitool_setting::Value;

/// One line: world-space ends, ARGB colour, width in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldLine {
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub color: u32,
    pub width: f32,
}

/// Java's `RenderQueue`: lines plus the current line width.
#[derive(Clone, Debug, PartialEq)]
pub struct Lines {
    pub lines: Vec<WorldLine>,
    /// Java's `lineWidth`: reset to 2 for each module, used by every line
    /// added after it is set.
    pub width: f32,
}

impl Default for Lines {
    fn default() -> Self {
        Lines {
            lines: Vec::new(),
            width: 2.0,
        }
    }
}

impl Lines {
    pub fn line(&mut self, start: [f64; 3], end: [f64; 3], argb: u32) {
        self.lines.push(WorldLine {
            start,
            end,
            color: argb,
            width: self.width,
        });
    }

    /// Java's `box`: the twelve edges (bottom rectangle, top rectangle, the
    /// four uprights).
    pub fn cuboid(&mut self, min: [f64; 3], max: [f64; 3], argb: u32) {
        let [x1, y1, z1] = min;
        let [x2, y2, z2] = max;
        for y in [y1, y2] {
            self.line([x1, y, z1], [x2, y, z1], argb);
            self.line([x2, y, z1], [x2, y, z2], argb);
            self.line([x2, y, z2], [x1, y, z2], argb);
            self.line([x1, y, z2], [x1, y, z1], argb);
        }
        for (x, z) in [(x1, z1), (x2, z1), (x2, z2), (x1, z2)] {
            self.line([x, y1, z], [x, y2, z], argb);
        }
    }

    /// Java's `flat`: the horizontal quad `(x1,z1),(x1,z2),(x2,z2),(x2,z1)`
    /// at height `y`; drawn as its outline (see the deviation above).
    pub fn flat(&mut self, x1: f64, z1: f64, x2: f64, z2: f64, y: f64, argb: u32) {
        self.line([x1, y, z1], [x1, y, z2], argb);
        self.line([x1, y, z2], [x2, y, z2], argb);
        self.line([x2, y, z2], [x2, y, z1], argb);
        self.line([x2, y, z1], [x1, y, z1], argb);
    }

    /// Java's `filledBox`: nothing (no quads, see the deviation above).
    pub fn filled_box(&mut self, _min: [f64; 3], _max: [f64; 3], _argb: u32) {}
}

/// What the game knows this frame besides the host: read by the glue.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    /// The player's position (not interpolated, Java's `getX/Y/Z`).
    pub player: Option<[f64; 3]>,
    /// The player's eye position, interpolated (`getEyePosition(pt)`).
    pub eye: Option<[f64; 3]>,
    /// The dimension id (`minecraft:overworld`).
    pub dim: String,
    /// The world seed when the game hosts the world (singleplayer).
    pub seed: Option<i64>,
    /// The outline boxes `[x1,y1,z1,x2,y2,z2]` of the block the player looks
    /// at (`hitResult` of type BLOCK, world space); empty otherwise.
    pub target: Vec<[f64; 6]>,
}

/// A module's frame: adds its lines.
pub type DrawFn = fn(&dyn Host, &Frame, &mut Lines);

/// The modules that draw in the world, in module order (Java
/// `Modules.renderWorld`: only active modules).
pub const DRAWERS: &[(&ModuleInfo, DrawFn)] = &[
    (
        &super::omnitool_light_overlay::MODULE,
        super::omnitool_light_overlay::draw,
    ),
    (
        &super::omnitool_chunk_borders::MODULE,
        super::omnitool_chunk_borders::draw,
    ),
    (
        &super::omnitool_block_outline::MODULE,
        super::omnitool_block_outline::draw,
    ),
    (
        &super::omnitool_world_ranges::SPAWN_SPHERES,
        super::omnitool_world_ranges::draw_spheres,
    ),
    (
        &super::omnitool_world_ranges::SPAWNER_RANGE,
        super::omnitool_world_ranges::draw_spawners,
    ),
    (
        &super::omnitool_world_ranges::BEACON_RANGE,
        super::omnitool_world_ranges::draw_beacons,
    ),
];

/// All lines of this frame. Nothing without a player.
pub fn draw(host: &dyn Host, frame: &Frame) -> Vec<WorldLine> {
    let mut out = Lines::default();
    if frame.player.is_none() {
        return out.lines;
    }
    for (m, f) in DRAWERS {
        if core::active(host, m) {
            out.width = 2.0;
            f(host, frame, &mut out);
        }
    }
    out.lines
}

/// A colour setting's ARGB.
pub fn color(v: Value) -> u32 {
    match v {
        Value::Color(c) => c as u32,
        _ => 0xFFFF_FFFF,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuboid_has_twelve_edges() {
        let mut l = Lines {
            width: 3.0,
            ..Lines::default()
        };
        l.cuboid([0.0; 3], [1.0; 3], 7);
        assert_eq!(l.lines.len(), 12);
        assert!(l.lines.iter().all(|x| x.width == 3.0 && x.color == 7));
        // every edge is axis-aligned and one block long
        for x in &l.lines {
            let d: f64 = (0..3).map(|i| (x.end[i] - x.start[i]).abs()).sum();
            assert_eq!(d, 1.0);
        }
    }

    #[test]
    fn flat_is_its_outline_and_filled_box_nothing() {
        let mut l = Lines::default();
        l.flat(0.0, 0.0, 16.0, 16.0, 4.05, 1);
        assert_eq!(l.lines.len(), 4);
        assert!(
            l.lines
                .iter()
                .all(|x| x.start[1] == 4.05 && x.end[1] == 4.05)
        );
        l.filled_box([0.0; 3], [1.0; 3], 1);
        assert_eq!(l.lines.len(), 4);
    }
}
