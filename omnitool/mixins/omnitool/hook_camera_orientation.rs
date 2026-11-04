// Billboards face the camera's rotation while Freelook/Freecam run.
fn omnitool_camera_orientation(game: &crate::game::Game<'_>) -> core::ops::ControlFlow<glam::Quat> {
    crate::game::omnitool_glue::camera_orientation(game)
}
