// No Fog (docs/spec/omnitool/render-tweaks.md, section 2). The dimension's
// distance fog goes through Horizon's own fog switch (`settings.horizon.fog`
// -> `fog::clear_distance_fog`, game/render_tick.rs:141) every tick via
// `Host::set_distance_fog_removed`. The environment fogs (water, lava, powder
// snow, blindness, darkness) are the Mixin part: Horizon computes them in
// `horizon_client::fog::setup_fog` (render_tick.rs:139) from `FogInput`; a
// RETURN injection there would clear the start/end of the fog this module's
// `current()` names. Until then `remove_environment` only has its logic and
// tests. Added to the game as `game::omnitool_no_fog`; also a module of this
// crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, NONE, SettingDef};

const fn remove(id: &'static str, name: &'static str, default: bool) -> SettingDef {
    SettingDef::new(id, name, Kind::Bool(default))
}

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "no_fog",
    name: "No Fog",
    category: Category::Render,
    description: "Removes fog, separately for each fog type.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: true,
    replaces: "NoFog",
    settings: &[
        remove("overworld", "Remove Overworld fog", true),
        remove("nether", "Remove Nether fog", true),
        remove("end", "Remove End fog", true),
        remove("water", "Remove water fog", false),
        remove("lava", "Remove lava fog", false),
        remove("powder_snow", "Remove powder snow fog", false),
        remove(
            "blindness",
            "Remove blindness fog (never with Safe Mode)",
            false,
        ),
        remove(
            "darkness",
            "Remove darkness fog (never with Safe Mode)",
            false,
        ),
    ],
    command: None,
    key: NONE,
    hold: false,
    hud: None,
    tick: Some(tick),
    on_key: None,
    hud_right: None,
    on_action: None,
};

/// Java's `Kind`: the fog types, named like the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FogKind {
    Overworld,
    Nether,
    End,
    Water,
    Lava,
    PowderSnow,
    Blindness,
    Darkness,
}

impl FogKind {
    fn setting(self) -> &'static str {
        match self {
            FogKind::Overworld => "overworld",
            FogKind::Nether => "nether",
            FogKind::End => "end",
            FogKind::Water => "water",
            FogKind::Lava => "lava",
            FogKind::PowderSnow => "powder_snow",
            FogKind::Blindness => "blindness",
            FogKind::Darkness => "darkness",
        }
    }
}

/// The fluid at the camera (Java's `getFluidInCamera`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fluid {
    #[allow(dead_code)]
    None,
    Water,
    Lava,
    PowderSnow,
}

/// Java's `dimension()`: the Nether and the End by id, everything else
/// (modded dimensions too) is the Overworld.
pub fn dimension(id: &str) -> FogKind {
    match id {
        "minecraft:the_nether" => FogKind::Nether,
        "minecraft:the_end" => FogKind::End,
        _ => FogKind::Overworld,
    }
}

/// Java's `current()`, the fog in charge, in vanilla's priority: lava,
/// blindness, darkness, powder snow, water, then the dimension.
pub fn current(fluid: Fluid, blindness: bool, darkness: bool, dimension_id: &str) -> FogKind {
    if fluid == Fluid::Lava {
        FogKind::Lava
    } else if blindness {
        FogKind::Blindness
    } else if darkness {
        FogKind::Darkness
    } else if fluid == Fluid::PowderSnow {
        FogKind::PowderSnow
    } else if fluid == Fluid::Water {
        FogKind::Water
    } else {
        dimension(dimension_id)
    }
}

/// Java's `remove(kind)`: blindness and darkness never under Safe Mode.
pub fn removes(host: &dyn Host, kind: FogKind) -> bool {
    if matches!(kind, FogKind::Blindness | FogKind::Darkness) && core::safe_mode(host, &MODULE) {
        return false;
    }
    core::value(host, &MODULE, kind.setting()).on()
}

/// Java's `removeDistance()`: only the dimension counts.
pub fn remove_distance(host: &dyn Host) -> bool {
    let Some(p) = host.player() else {
        return false;
    };
    core::active(host, &MODULE) && removes(host, dimension(&p.dimension))
}

/// Java's `removeEnvironment()` for the fog in charge right now.
#[allow(dead_code)] // the environment fogs need the `setup_fog` injection
pub fn remove_environment(host: &dyn Host, fluid: Fluid, blindness: bool, darkness: bool) -> bool {
    let Some(p) = host.player() else {
        return false;
    };
    core::active(host, &MODULE) && removes(host, current(fluid, blindness, darkness, &p.dimension))
}

fn tick(host: &mut dyn Host) {
    let removed = remove_distance(host);
    host.set_distance_fog_removed(removed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::Player;
    use crate::omnitool_core::tests::{Fake, restart};

    fn fixtures() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/render-tweaks.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn name(k: FogKind) -> &'static str {
        match k {
            FogKind::Overworld => "OVERWORLD",
            FogKind::Nether => "NETHER",
            FogKind::End => "END",
            FogKind::Water => "WATER",
            FogKind::Lava => "LAVA",
            FogKind::PowderSnow => "POWDER_SNOW",
            FogKind::Blindness => "BLINDNESS",
            FogKind::Darkness => "DARKNESS",
        }
    }

    #[test]
    fn priority_fixtures() {
        for c in fixtures()["no_fog_priority"].as_array().unwrap() {
            let fluid = match c["fluid"].as_str().unwrap() {
                "LAVA" => Fluid::Lava,
                "WATER" => Fluid::Water,
                "POWDER_SNOW" => Fluid::PowderSnow,
                _ => Fluid::None,
            };
            let flag = |k: &str| c[k].as_bool().unwrap_or(false);
            let dim = c["dimension"].as_str().unwrap_or("minecraft:overworld");
            let got = current(fluid, flag("blindness"), flag("darkness"), dim);
            assert_eq!(name(got), c["expect"].as_str().unwrap(), "{c}");
        }
    }

    fn in_dimension(id: &str) -> Fake {
        Fake {
            player: Some(Player {
                x: 0.0,
                y: 64.0,
                z: 0.0,
                yaw: 0.0,
                pitch: 0.0,
                dimension: id.into(),
            }),
            ..Fake::default()
        }
    }

    #[test]
    fn defaults() {
        assert_eq!(MODULE.settings.len(), 8);
        const { assert!(MODULE.fair_variant) };
        let on: Vec<_> = MODULE
            .settings
            .iter()
            .filter(|d| matches!(d.kind, Kind::Bool(true)))
            .map(|d| d.id)
            .collect();
        assert_eq!(on, ["overworld", "nether", "end"]);
    }

    #[test]
    fn distance_fog_follows_dimension() {
        restart();
        let mut h = in_dimension("minecraft:the_nether");
        tick(&mut h);
        assert!(!h.fog_removed, "module is off");
        core::set_enabled(&mut h, &MODULE, true);
        tick(&mut h);
        assert!(h.fog_removed);
        let mut h2 = in_dimension("custom:dim");
        tick(&mut h2);
        assert!(h2.fog_removed, "modded dimensions count as Overworld");
        let mut none = Fake::default();
        tick(&mut none);
        assert!(!none.fog_removed, "no level, no dimension");
    }

    #[test]
    fn environment_fog_needs_its_setting() {
        restart();
        let mut h = in_dimension("minecraft:overworld");
        core::set_enabled(&mut h, &MODULE, true);
        assert!(remove_environment(&h, Fluid::None, false, false));
        assert!(!remove_environment(&h, Fluid::Water, false, false));
        assert!(!remove_environment(&h, Fluid::None, true, false));
    }
}
