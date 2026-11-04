// The minimap Omnitool asks for.
fn omnitool_minimap(
    game: &crate::game::Game<'_>,
    minimap: &mut Option<crate::game::native_hooks::NativeMinimap>,
) {
    crate::game::omnitool_glue::minimap(game, minimap);
}
