// Vanilla's crosshair (and its attack indicator sprites), dropped while Custom
// Crosshair draws its own.
fn omnitool_crosshair(value: Vec<GuiDraw>) -> Vec<GuiDraw> {
    if crate::screen_effects::omnitool_bridge::hide_crosshair() {
        Vec::new()
    } else {
        value
    }
}
