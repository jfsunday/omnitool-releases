// Mouse turn: Freecam and Freelook may take it (Break) before the player turns.
fn omnitool_turn_player(
    game: &mut crate::game::Game<'_>,
    xo: f64,
    yo: f64,
) -> core::ops::ControlFlow<()> {
    crate::game::omnitool_glue::turn_player(game, xo, yo)
}
