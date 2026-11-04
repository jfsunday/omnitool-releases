// The HUD: the modules' boxes in the corners.
fn omnitool_hud(
    game: &mut crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
    corners: &mut horizon_client::extras_hud::Corners,
) {
    crate::game::omnitool_glue::hud(game, batch, corners);
}
