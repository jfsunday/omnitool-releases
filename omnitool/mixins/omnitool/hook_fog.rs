// The frame's fog with No Fog's and Clear Water's changes.
fn omnitool_fog(
    game: &crate::game::Game<'_>,
    camera: glam::DVec3,
    sky: &horizon_client::environment::Sky,
    biome: u16,
    water_fog_color: [f32; 3],
) -> std::ops::ControlFlow<FogData> {
    if !crate::game::omnitool_glue::fog_wanted(game) {
        return std::ops::ControlFlow::Continue(());
    }
    let Some(_busy) = crate::game::omnitool_glue::FogBusy::enter() else {
        return std::ops::ControlFlow::Continue(());
    };
    let fluid = match fog_type(
        &game.world,
        game.res.facts(),
        [camera.x, camera.y, camera.z],
        game.forward(),
    ) {
        FogType::Water => crate::game::omnitool_no_fog::Fluid::Water,
        FogType::Lava => crate::game::omnitool_no_fog::Fluid::Lava,
        FogType::PowderSnow => crate::game::omnitool_no_fog::Fluid::PowderSnow,
        FogType::Atmospheric => crate::game::omnitool_no_fog::Fluid::None,
    };
    let blindness = game.potions.effects.get("blindness").is_some();
    let darkness = game.darkness_fog().is_some();
    // Runs the original body (the hook is busy, so it continues).
    let mut fog = game.frame_fog(camera, sky, biome, water_fog_color);
    crate::game::omnitool_glue::edit_fog(
        game,
        fluid,
        blindness,
        darkness,
        &mut fog.environmental_start,
        &mut fog.environmental_end,
    );
    std::ops::ControlFlow::Break(fog)
}
