// Clear Water (docs/spec/omnitool/render-tweaks.md, section 3): the maths of
// `start`/`end` for the water and lava fog and the overlay switch. Nothing
// applies them in the game yet (Mixin needed): the fog is built in
// `horizon_client::fog::setup_fog` (game/render_tick.rs:139, Horizon fbd7288),
// so a RETURN injection there would run `start`/`end` on the returned fog
// when `fluid()` is not NONE; the underwater overlay is drawn by
// horizon-client-render `screen_effects.rs` (water overlay) and would be
// skipped when `hide_overlay` holds. Added to the game as
// `game::omnitool_clear_water`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_no_fog::Fluid;
use super::omnitool_setting::{Kind, NONE, SettingDef};

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "clear_water",
    name: "Clear Water",
    category: Category::Render,
    description: "See further under water and in lava, without the underwater overlay.",
    default_on: false,
    cheaty: false,
    beta: true,
    fair_variant: false,
    replaces: "",
    settings: &[
        SettingDef::new(
            "water",
            "Water view distance (%)",
            Kind::Number {
                default: 300.0,
                min: 100.0,
                max: 1000.0,
                step: 10.0,
            },
        ),
        SettingDef::new(
            "lava",
            "Lava view distance (blocks)",
            Kind::Number {
                default: 8.0,
                min: 1.0,
                max: 64.0,
                step: 1.0,
            },
        ),
        SettingDef::new(
            "overlay",
            "Hide underwater overlay texture",
            Kind::Bool(true),
        ),
    ],
    command: None,
    key: NONE,
    hold: false,
    hud: None,
    tick: None,
    on_key: None,
    hud_right: None,
    on_action: None,
};

/// Java's `fluid()`: nothing when inactive, and No Fog wins when it removes
/// the fog in charge (`no_fog_removes`).
pub fn fluid(host: &dyn Host, no_fog_removes: bool, camera: Fluid) -> Fluid {
    if !core::active(host, &MODULE) || no_fog_removes {
        Fluid::None
    } else {
        camera
    }
}

/// Java's `start(v)`: lava fog starts at the camera.
pub fn start_for(fluid: Fluid, v: f32) -> f32 {
    if fluid == Fluid::Lava { v.min(0.0) } else { v }
}

/// Java's `end(v)`: water scales by `water` %, lava is at least `lava` blocks.
pub fn end_for(fluid: Fluid, water: f64, lava: f64, v: f32) -> f32 {
    match fluid {
        Fluid::Water => v * (water / 100.0) as f32,
        Fluid::Lava => v.max(lava as f32),
        _ => v,
    }
}

pub fn start(host: &dyn Host, fluid: Fluid, v: f32) -> f32 {
    let _ = host;
    start_for(fluid, v)
}

pub fn end(host: &dyn Host, fluid: Fluid, v: f32) -> f32 {
    let water = core::value(host, &MODULE, "water").num();
    let lava = core::value(host, &MODULE, "lava").num();
    end_for(fluid, water, lava, v)
}

/// The fog's environmental start and end after No Fog and Clear Water, when
/// they change anything. No Fog removes the fog in charge, but only an
/// environment one (fluid, blindness, darkness): the dimension's fog is
/// Horizon's own option (`set_distance_fog_removed`). Otherwise Clear Water
/// stretches water, lava follows `start`/`end`.
pub fn fog_edit(
    host: &dyn Host,
    fluid: Fluid,
    blindness: bool,
    darkness: bool,
    start: f32,
    end: f32,
) -> Option<(f32, f32)> {
    let removes = super::omnitool_no_fog::remove_environment(host, fluid, blindness, darkness);
    if removes && (fluid != Fluid::None || blindness || darkness) {
        return Some((FAR_START, FAR_END));
    }
    let f = self::fluid(host, removes, fluid);
    let edited = (self::start(host, f, start), self::end(host, f, end));
    ((start, end) != edited).then_some(edited)
}

/// Where a removed fog starts and ends (the distances Horizon's Fog option
/// uses for "no fog").
pub const FAR_START: f32 = 1e9;
pub const FAR_END: f32 = 2e9;

/// Java's `hideOverlay()`.
pub fn hide_overlay(host: &dyn Host) -> bool {
    core::active(host, &MODULE) && core::value(host, &MODULE, "overlay").on()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};

    fn fixtures() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/render-tweaks.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn fixture_cases() {
        for c in fixtures()["clear_water"].as_array().unwrap() {
            let fluid = match c["fluid"].as_str().unwrap() {
                "WATER" => Fluid::Water,
                "LAVA" => Fluid::Lava,
                _ => Fluid::None,
            };
            let water = c["water"].as_f64().unwrap_or(300.0);
            let lava = c["lava"].as_f64().unwrap_or(8.0);
            if let Some(v) = c["start_in"].as_f64() {
                let out = start_for(fluid, v as f32);
                assert_eq!(out, c["start_out"].as_f64().unwrap() as f32, "{c}");
            }
            let out = end_for(fluid, water, lava, c["end_in"].as_f64().unwrap() as f32);
            assert_eq!(out, c["end_out"].as_f64().unwrap() as f32, "{c}");
        }
    }

    #[test]
    fn module_gates() {
        restart();
        let mut h = Fake::default();
        assert_eq!(MODULE.settings.len(), 3);
        assert!(!hide_overlay(&h));
        assert_eq!(fluid(&h, false, Fluid::Water), Fluid::None);
        core::set_enabled(&mut h, &MODULE, true);
        assert!(hide_overlay(&h));
        assert_eq!(fluid(&h, false, Fluid::Water), Fluid::Water);
        assert_eq!(fluid(&h, true, Fluid::Water), Fluid::None, "No Fog wins");
        assert_eq!(end(&h, Fluid::Water, 96.0), 288.0);
        assert_eq!(start(&h, Fluid::Lava, 0.25), 0.0);
    }

    #[test]
    fn fog_edit_needs_a_module() {
        use crate::omnitool_core::Player;
        restart();
        let mut h = Fake {
            player: Some(Player {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                yaw: 0.0,
                pitch: 0.0,
                dimension: "minecraft:overworld".to_owned(),
            }),
            ..Fake::default()
        };
        assert_eq!(fog_edit(&h, Fluid::Water, false, false, 0.0, 96.0), None);
        core::set_enabled(&mut h, &MODULE, true);
        assert_eq!(
            fog_edit(&h, Fluid::Water, false, false, 0.0, 96.0),
            Some((0.0, 288.0))
        );
        assert_eq!(fog_edit(&h, Fluid::None, false, false, 0.0, 96.0), None);
        // lava: starts at the camera, at least 8 blocks
        assert_eq!(
            fog_edit(&h, Fluid::Lava, false, false, 0.25, 1.0),
            Some((0.0, 8.0))
        );
    }
}
