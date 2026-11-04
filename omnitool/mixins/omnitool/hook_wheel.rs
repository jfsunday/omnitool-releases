// The mouse wheel while playing: Zoom takes it.
fn omnitool_wheel(game: &mut crate::game::Game<'_>, steps: f32, claimed: &mut bool) {
    crate::game::omnitool_glue::wheel(game, steps, claimed);
}
