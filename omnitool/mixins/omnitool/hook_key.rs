// A key or mouse button went down or up: the modules' shortcut keys.
fn omnitool_key(game: &mut crate::game::Game<'_>, key: &str, pressed: bool) {
    crate::game::omnitool_glue::key(game, key, pressed);
}
