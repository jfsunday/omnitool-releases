// A command typed in chat: Omnitool's, when it is one.
fn omnitool_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    crate::game::omnitool_glue::command(game, command, ran);
}
