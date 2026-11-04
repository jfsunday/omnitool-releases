// No Hurt Cam (docs/spec/omnitool/render-tweaks.md, section 5): the tilt
// factor and the bobbing switch. Mixin needed to apply them: Horizon computes
// the hurt tilt in horizon-client-render `camera.rs` `bob_hurt` (called from
// the horizon-client camera setup, `damage_tilt_strength` argument), where
// `tilt` replaces the 14-degree constant; the view bobbing is skipped there
// while `skip_bobbing` holds. Added to the game as `game::omnitool_no_hurt_cam`;
// also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, NONE, SettingDef};

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "no_hurt_cam",
    name: "No Hurt Cam",
    category: Category::Render,
    description: "Removes or weakens the camera shake when you take damage.",
    default_on: false,
    cheaty: false,
    beta: true,
    fair_variant: false,
    replaces: "No Hurt Cam",
    settings: &[
        SettingDef::new(
            "strength",
            "Hurt tilt strength (%)",
            Kind::Number {
                default: 0.0,
                min: 0.0,
                max: 100.0,
                step: 1.0,
            },
        ),
        SettingDef::new(
            "no_bobbing",
            "No view bobbing while taking damage",
            Kind::Bool(false),
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

/// Java's `tilt(v)` for vanilla's constant `v` (14.0).
#[allow(dead_code)] // needs the `bob_hurt` injection
pub fn tilt(host: &dyn Host, v: f64) -> f64 {
    if core::active(host, &MODULE) {
        v * core::value(host, &MODULE, "strength").num() / 100.0
    } else {
        v
    }
}

/// Java's `skipBobbing()`; `hurt_time` is the player's `hurtTime`.
#[allow(dead_code)] // needs the bobbing injection
pub fn skip_bobbing(host: &dyn Host, hurt_time: i32) -> bool {
    core::active(host, &MODULE)
        && core::value(host, &MODULE, "no_bobbing").on()
        && host.player().is_some()
        && hurt_time > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::Player;
    use crate::omnitool_core::tests::{Fake, restart};

    #[test]
    fn fixture_cases() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/render-tweaks.json"
        );
        let f: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for c in f["hurt_tilt"].as_array().unwrap() {
            let v = c["vanilla"].as_f64().unwrap();
            let strength = c["strength"].as_f64().unwrap();
            let got = v * strength / 100.0;
            assert!((got - c["out"].as_f64().unwrap()).abs() < 1e-9, "{c}");
        }
    }

    #[test]
    fn gates() {
        restart();
        let mut h = Fake::default();
        assert_eq!(MODULE.settings.len(), 2);
        assert_eq!(tilt(&h, 14.0), 14.0, "off: vanilla");
        core::set_enabled(&mut h, &MODULE, true);
        assert_eq!(tilt(&h, 14.0), 0.0);
        assert!(!skip_bobbing(&h, 5), "no_bobbing is off");
        h.player = Some(Player {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            dimension: "minecraft:overworld".into(),
        });
        assert!(!skip_bobbing(&h, 5));
    }
}
