// The frame's camera: Freelook's rotation / Freecam's pose. The original
// runs through the re-entry guard in the glue; the return type is spelled
// as the target's.
fn omnitool_camera_view(
    game: &mut crate::game::Game<'_>,
    partial: f32,
) -> core::ops::ControlFlow<Option<(glam::DVec3, f32, f32)>> {
    crate::game::omnitool_glue::camera_view(game, partial)
}
