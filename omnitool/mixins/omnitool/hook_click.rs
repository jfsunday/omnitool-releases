// A click on Omnitool's settings page.
fn omnitool_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    crate::game::omnitool_glue::settings_click(game, mod_id, button);
}
