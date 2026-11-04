// Item tooltips with the lines of Better Tooltips, Shulker Box Tooltip and
// AppleSkin. Follows `draw_stack_tooltip_with` (game/stations.rs:2353) at
// fbd7288 line by line, with the extended lines instead of the game's own; it
// only takes over while Omnitool is on.
fn omnitool_tooltip(
    game: &mut crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
    stack: &horizon_client::items::ItemStack,
    creative: bool,
    x: f32,
    y: f32,
    after_name: &Vec<(horizon_client::text::Text, u32)>,
) -> std::ops::ControlFlow<()> {
    if !crate::game::omnitool_glue::tooltip_active(game) {
        return std::ops::ControlFlow::Continue(());
    }
    let lines = game.stack_tooltip_lines(stack, false, creative);
    let mut lines = crate::game::omnitool_glue::tooltip_lines(game, stack, lines);
    if lines.is_empty() {
        return std::ops::ControlFlow::Break(());
    }
    lines.splice(1..1, after_name.iter().cloned());
    let lines = crate::game::Game::resolved(&lines);
    let probe = horizon_client::bundle::tooltip_layout(game.items(), stack, 0.0, 0.0, 96.0);
    let extra = probe.as_ref().map_or(0.0, |l| l.height + 2.0);
    let atlas = &game.gui.atlas;
    let (x, y, w, h) = crate::game::Game::tooltip_geometry(
        atlas,
        &lines,
        (x, y),
        (batch.width, batch.height),
        extra,
    );
    crate::game::Game::tooltip_box(batch, atlas, x, y, w, h);
    let text_h = h - extra;
    crate::game::Game::tooltip_text(batch, atlas, &lines, x, y);
    if let Some(layout) =
        horizon_client::bundle::tooltip_layout(game.items(), stack, x, y + text_h + 2.0, w)
    {
        game.draw_bundle_tooltip(batch, &layout);
    }
    std::ops::ControlFlow::Break(())
}
