// Chat Tweaks: every line a connection adds to the chat (`chat_line`) goes
// through the glue first, which may hide it or add timestamp / `(xN)` text.
fn omnitool_chat_line(
    game: &mut crate::game::Game<'_>,
    content: &horizon_client::text::Text,
    source: horizon_client::chat::ChatSource,
) -> core::ops::ControlFlow<bool> {
    crate::game::omnitool_glue::chat_line(game, content, source)
}
