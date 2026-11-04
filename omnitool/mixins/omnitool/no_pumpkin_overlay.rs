// No Pumpkin Overlay (docs/spec/omnitool/render-tweaks.md, section 6): which
// camera overlays to hide. Mixin needed to apply it: Horizon builds the
// full-screen overlays in horizon-client-render `hud_overlays.rs`
// `camera_overlays` (HEAD: clear the overlay fields when `hide_texture` /
// `hide_portal` / `hide_nausea` hold). Added to the game as
// `game::omnitool_no_pumpkin_overlay`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, NONE, SettingDef};

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "no_pumpkin_overlay",
    name: "No Pumpkin Overlay",
    category: Category::Render,
    description: "Removes the carved pumpkin overlay and optionally the powder snow, portal and nausea overlays.",
    default_on: false,
    cheaty: false,
    beta: true,
    fair_variant: false,
    replaces: "No Pumpkin Overlay",
    settings: &[
        SettingDef::new(
            "pumpkin",
            "Hide pumpkin and other helmet overlays",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "powder_snow",
            "Hide powder snow frost overlay",
            Kind::Bool(true),
        ),
        SettingDef::new("portal", "Hide Nether portal overlay", Kind::Bool(false)),
        SettingDef::new("nausea", "Hide nausea overlay", Kind::Bool(false)),
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

fn on(host: &dyn Host, id: &str) -> bool {
    core::active(host, &MODULE) && core::value(host, &MODULE, id).on()
}

/// Java's `hideTexture(path)`: powder snow first, then nausea, anything else
/// counts as a helmet overlay. No path hides nothing.
#[allow(dead_code)] // needs the `camera_overlays` injection
pub fn hide_texture(host: &dyn Host, path: Option<&str>) -> bool {
    let Some(path) = path else {
        return false;
    };
    if path.contains("powder_snow") {
        on(host, "powder_snow")
    } else if path.contains("nausea") {
        on(host, "nausea")
    } else {
        on(host, "pumpkin")
    }
}

/// Java's `hidePortal()`.
#[allow(dead_code)] // needs the `camera_overlays` injection
pub fn hide_portal(host: &dyn Host) -> bool {
    on(host, "portal")
}

/// Java's `hideNausea()`.
#[allow(dead_code)] // needs the `camera_overlays` injection
pub fn hide_nausea(host: &dyn Host) -> bool {
    on(host, "nausea")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};

    #[test]
    fn fixture_cases() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/render-tweaks.json"
        );
        let f: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for c in f["pumpkin_hide_texture"].as_array().unwrap() {
            restart();
            let mut h = Fake::default();
            core::set_enabled(&mut h, &MODULE, true);
            for id in ["pumpkin", "powder_snow", "nausea"] {
                let v = c["settings"][id].as_bool().unwrap();
                core::with(&mut h, |s, _| {
                    s.set_setting(&MODULE, id, if v { "true" } else { "false" })
                })
                .unwrap();
            }
            let got = hide_texture(&h, c["path"].as_str());
            assert_eq!(got, c["hide"].as_bool().unwrap(), "{c}");
        }
    }

    #[test]
    fn inactive_hides_nothing() {
        restart();
        let h = Fake::default();
        assert_eq!(MODULE.settings.len(), 4);
        assert!(!hide_texture(&h, Some("textures/misc/pumpkinblur.png")));
        assert!(!hide_portal(&h));
        assert!(!hide_nausea(&h));
    }
}
