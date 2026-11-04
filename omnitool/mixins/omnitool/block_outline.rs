// Block Outline (docs/spec/omnitool/world-render.md §3): a coloured box
// around each outline box of the targeted block, 0.003 larger, drawn
// through the world lines. The game's own outline stays. Added to the game
// as `game::omnitool_block_outline`; also a module of this crate.
//
// Deviation: no fill (Horizon's gizmos draw no quads); the `fill` settings
// are kept for the config. `width` is the gizmo line width in pixels, so
// the `(1.21.9+)` label holds on Horizon too.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, NONE, SettingDef};
use super::omnitool_world_lines::{Frame, Lines, color};

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "block_outline",
    name: "Block Outline",
    category: Category::Render,
    description: "Colored outline and fill for the block you look at.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "",
    settings: &[
        SettingDef::new("color", "Outline color", Kind::Color(0xFF40_E0FFu32 as i32)),
        SettingDef::new(
            "width",
            "Line width (1.21.9+)",
            Kind::Number {
                default: 3.0,
                min: 1.0,
                max: 8.0,
                step: 0.5,
            },
        ),
        SettingDef::new("fill", "Fill", Kind::Bool(true)),
        SettingDef::new(
            "fill_color",
            "Fill color",
            Kind::Color(0x3040_E0FFu32 as i32),
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

/// Java's `renderWorld`.
pub fn draw(host: &dyn Host, frame: &Frame, out: &mut Lines) {
    let v = |id| core::value(host, &MODULE, id);
    out.width = v("width").num() as f32;
    let fill = v("fill").on().then(|| color(v("fill_color")));
    boxes(&frame.target, color(v("color")), fill, out);
}

pub fn boxes(target: &[[f64; 6]], c: u32, fill: Option<u32>, out: &mut Lines) {
    for b in target {
        let min = [b[0] - 0.003, b[1] - 0.003, b[2] - 0.003];
        let max = [b[3] + 0.003, b[4] + 0.003, b[5] + 0.003];
        out.cuboid(min, max, c);
        if let Some(f) = fill {
            out.filled_box(min, max, f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_box_per_shape_part() {
        let mut l = Lines {
            width: 3.0,
            ..Lines::default()
        };
        // stairs: two boxes
        let t = [
            [1.0, 2.0, 3.0, 2.0, 2.5, 4.0],
            [1.0, 2.5, 3.0, 2.0, 3.0, 3.5],
        ];
        boxes(&t, 5, Some(6), &mut l);
        assert_eq!(l.lines.len(), 24);
        assert_eq!(l.lines[0].start, [0.997, 1.997, 2.997]);
        assert!(l.lines.iter().all(|x| x.color == 5 && x.width == 3.0));
        let mut l = Lines::default();
        boxes(&[], 5, None, &mut l);
        assert!(l.lines.is_empty());
    }

    #[test]
    fn defaults() {
        assert_eq!(MODULE.settings.len(), 4);
        const { assert!(!MODULE.cheaty && !MODULE.beta && !MODULE.default_on) };
    }
}
