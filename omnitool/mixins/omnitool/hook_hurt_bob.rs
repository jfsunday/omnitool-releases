// View bobbing, off for a hit while No Hurt Cam says so.
fn omnitool_hurt_bob(state: &BobState) -> std::ops::ControlFlow<()> {
    if crate::screen_effects::omnitool_bridge::camera().no_bobbing
        && state.living.is_some_and(|l| l.hurt_time > 0.0)
    {
        std::ops::ControlFlow::Break(())
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
