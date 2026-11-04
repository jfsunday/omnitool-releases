// Once per tick: Omnitool's tick work (config autosave).
fn omnitool_tick(game: &mut crate::game::Game<'_>) {
    crate::game::omnitool_glue::tick(game);
}
