// The craft calculator panel's action button.
fn omnitool_panel_event(
    game: &mut crate::game::Game<'_>,
    mod_id: &str,
    event: &crate::game::native_hooks::NativePanelEvent,
) {
    crate::game::omnitool_glue::panel_event(game, mod_id, event);
}
