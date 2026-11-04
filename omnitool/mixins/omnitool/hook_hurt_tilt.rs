// The damage tilt strength No Hurt Cam scales.
fn omnitool_hurt_tilt(value: f64) -> f64 {
    value * crate::screen_effects::omnitool_bridge::camera().hurt_tilt
}
