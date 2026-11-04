// Fullbright (docs/spec/omnitool/render-tweaks.md, section 1). Java overrides
// the gamma option (`OptionInstance.get` -> 16.0) or adds a hidden Night
// Vision effect. Horizon has a full-brightness flag of its own (`Extras`
// `fullbright`, applied to the renderer by `apply_ui_settings`), so both modes
// switch that flag every tick through `Host::set_fullbright` and the player's
// saved option is never touched. Deviation: NIGHT_VISION behaves like GAMMA
// (no fake effect is added). Added to the game as `game::omnitool_fullbright`;
// also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, NONE, SettingDef};

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "fullbright",
    name: "Fullbright",
    category: Category::Render,
    description: "See everything as if it were day.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "Gamma Utils, Fullbright",
    settings: &[SettingDef::new(
        "mode",
        "Mode",
        Kind::Enum {
            values: &["GAMMA", "NIGHT_VISION"],
            default: 0,
        },
    )],
    command: None,
    key: NONE,
    hold: false,
    hud: None,
    tick: Some(tick),
    on_key: None,
    hud_right: None,
    on_action: None,
};

/// Whether the renderer is at full brightness because of this module.
pub fn wanted(host: &dyn Host) -> bool {
    core::active(host, &MODULE)
}

fn tick(host: &mut dyn Host) {
    let on = wanted(host);
    host.set_fullbright(on);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};

    #[test]
    fn defaults() {
        assert_eq!(MODULE.settings.len(), 1);
        assert_eq!(MODULE.category, Category::Render);
        const { assert!(!MODULE.cheaty && !MODULE.beta && !MODULE.default_on) };
    }

    #[test]
    fn follows_enabled() {
        restart();
        let mut h = Fake::default();
        tick(&mut h);
        assert!(!h.fullbright);
        core::set_enabled(&mut h, &MODULE, true);
        tick(&mut h);
        assert!(h.fullbright);
        core::set_enabled(&mut h, &MODULE, false);
        tick(&mut h);
        assert!(!h.fullbright);
    }
}
