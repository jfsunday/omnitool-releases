// World time packets: feeds the Server TPS tracker. Hooked at the head of
// `remote_clock_tick` because `handle_packet` contains macros, which
// horizon-mixins cannot inject into. `remote_clock_tick` runs after each
// client tick (game time + 1) and right after `ClientboundSetTimePacket`
// set the game time; any other step than +1 is the server's time.
fn omnitool_set_time(game: &mut crate::game::Game<'_>) {
    let Some(r) = game.remote.as_ref() else {
        return;
    };
    if game.world.game_time - r.clock_game_time != 1 {
        crate::game::omnitool_glue::set_time(game, game.world.game_time);
    }
}
