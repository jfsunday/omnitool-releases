// World lines: the frame's gizmo lines (`fishing_line`) plus Omnitool's.
// The original runs through the re-entry guard in the glue. The return
// type is spelled as the target's (`Line` is imported in game/fishing.rs).
fn omnitool_world_lines(
    game: &crate::game::Game<'_>,
    alpha: f64,
    size: [u32; 2],
) -> core::ops::ControlFlow<Vec<Line>> {
    crate::game::omnitool_glue::world_lines(game, alpha, size)
}
