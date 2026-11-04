// Omnitool's settings page: one button per module.
fn omnitool_page(game: &mut crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    crate::game::omnitool_glue::settings(game, mod_id, buttons);
}
