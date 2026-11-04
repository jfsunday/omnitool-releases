// The zoom Omnitool asks for.
fn omnitool_zoom(
    game: &crate::game::Game<'_>,
    zoom: &mut Option<crate::game::native_hooks::NativeZoom>,
) {
    crate::game::omnitool_glue::zoom(game, zoom);
}
