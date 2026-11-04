// The camera overlays after Clear Water, Low Fire and No Pumpkin Overlay: the
// original runs with the hidden parts removed from its input, then the fire is
// moved, scaled and faded.
fn omnitool_quads(this: &ScreenEffects, input: &ScreenInputs) -> std::ops::ControlFlow<Vec<ScreenQuad>> {
    let o = crate::screen_effects::omnitool_bridge::overlays();
    if !o.any() || !crate::screen_effects::omnitool_bridge::enter() {
        return std::ops::ControlFlow::Continue(());
    }
    let overlays: Vec<GuiDraw> = input
        .gui_overlays
        .iter()
        .filter(|d| match d {
            GuiDraw::Blit { texture, .. } => {
                !((o.hide_pumpkin && texture == "misc/pumpkinblur")
                    || (o.hide_powder_snow && texture == "misc/powder_snow_outline"))
            }
            GuiDraw::AtlasSprite { sprite, .. } => !(o.hide_portal && sprite == "block/nether_portal"),
            _ => true,
        })
        .cloned()
        .collect();
    let patched = ScreenInputs {
        sprites: input.sprites,
        flags: input.flags,
        aspect: input.aspect,
        partial: input.partial,
        block_overlay: input.block_overlay.clone(),
        water: if o.hide_water { None } else { input.water },
        on_fire: input.on_fire && !o.hide_fire,
        gui_overlays: &overlays,
    };
    let mut quads = this.build(&patched);
    crate::screen_effects::omnitool_bridge::leave();
    if input.on_fire
        && !o.hide_fire
        && let Some(r) = input.sprites.get("block/fire_1")
    {
        let rect = [r.u0, r.v0, r.u1, r.v1];
        for quad in quads.iter_mut().filter(|q| q.vertices[0].rect == rect) {
            for v in quad.vertices.iter_mut() {
                let (pos, alpha) = crate::screen_effects::omnitool_bridge::fire_vertex(&o, v.pos, v.color[3]);
                v.pos = pos;
                v.color[3] = alpha;
            }
        }
    }
    std::ops::ControlFlow::Break(quads)
}
