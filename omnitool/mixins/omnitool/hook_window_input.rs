// A window event before the game sees it: while an Omnitool screen is open
// it takes the keys, clicks and the wheel (the event becomes a no-op).
fn omnitool_window_input(event: &mut winit::event::WindowEvent) {
    crate::game::omnitool_glue::window_input(event);
}
