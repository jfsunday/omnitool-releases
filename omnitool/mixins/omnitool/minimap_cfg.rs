// Minimap Config (docs/spec/omnitool/minimap.md): the settings of Horizon's
// built-in minimap, handed over through `native_minimap`, plus keys that step
// the zoom. Added to the game as `game::omnitool_minimap_cfg`; also a module
// of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_hud::HudCorner;
use super::omnitool_setting::{Kind, NONE, SettingDef};
use std::cell::Cell;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "minimap_cfg",
    name: "Minimap Config",
    category: Category::Hud,
    description: "Configures Horizon's built-in minimap: corner, size, zoom, rotation, shape.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "Xaero's Minimap",
    key: NONE,
    hold: false,
    settings: &[
        SettingDef::new(
            "corner",
            "Corner",
            Kind::Enum {
                values: &["TOP_LEFT", "TOP_RIGHT", "BOTTOM_LEFT", "BOTTOM_RIGHT"],
                default: 1,
            },
        ),
        SettingDef::new(
            "size",
            "Map size",
            Kind::Number {
                default: 128.0,
                min: MIN_SIZE,
                max: MAX_SIZE,
                step: 8.0,
            },
        ),
        SettingDef::new(
            "blocks_per_pixel",
            "Blocks per pixel",
            Kind::Number {
                default: 1.0,
                min: MIN_BPP,
                max: MAX_BPP,
                step: 0.5,
            },
        ),
        SettingDef::new("rotate", "Rotate with player", Kind::Bool(true)),
        SettingDef::new("round", "Round map", Kind::Bool(false)),
        SettingDef::new("zoom_in_key", "Zoom in key", Kind::Key(NONE)),
        SettingDef::new("zoom_out_key", "Zoom out key", Kind::Key(NONE)),
    ],
    hud: None,
    tick: None,
    command: None,
    on_key: Some(key_event),
    hud_right: None,
    on_action: None,
};

/// Horizon's limits for `NativeMinimap` (it clamps to them itself).
const MIN_SIZE: f64 = 16.0;
const MAX_SIZE: f64 = 256.0;
const MIN_BPP: f64 = 0.5;
const MAX_BPP: f64 = 4.0;

/// What `native_minimap` is asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    pub corner: HudCorner,
    pub size: f32,
    pub blocks_per_pixel: f32,
    pub rotate: bool,
    pub round: bool,
}

/// The zoom the keys set: blocks per pixel, and the setting's value at that
/// time (a changed setting wins again).
#[derive(Clone, Copy)]
struct Zoom {
    bpp: f64,
    base: f64,
}

thread_local! {
    static ZOOM: Cell<Option<Zoom>> = const { Cell::new(None) };
}

/// The corner a `corner` setting index stands for.
pub fn corner(index: usize) -> HudCorner {
    match index {
        0 => HudCorner::TopLeft,
        2 => HudCorner::BottomLeft,
        3 => HudCorner::BottomRight,
        _ => HudCorner::TopRight,
    }
}

/// One zoom step: in halves the blocks per pixel, out doubles them, within
/// the limits.
pub fn zoomed(bpp: f64, zoom_in: bool) -> f64 {
    let next = if zoom_in { bpp / 2.0 } else { bpp * 2.0 };
    next.clamp(MIN_BPP, MAX_BPP)
}

fn num(host: &dyn Host, id: &str) -> f64 {
    core::value(host, &MODULE, id).num()
}

/// The blocks per pixel in force: the keys' zoom while the setting is as it
/// was then, else the setting.
fn current(host: &dyn Host) -> f64 {
    let base = num(host, "blocks_per_pixel");
    match ZOOM.get() {
        Some(z) if z.base == base => z.bpp,
        _ => base,
    }
}

/// `native_minimap`: None while the module is not active.
pub fn request(host: &dyn Host) -> Option<Config> {
    if !core::active(host, &MODULE) {
        ZOOM.set(None);
        return None;
    }
    Some(Config {
        corner: corner(core::value(host, &MODULE, "corner").index()),
        size: num(host, "size").clamp(MIN_SIZE, MAX_SIZE) as f32,
        blocks_per_pixel: current(host).clamp(MIN_BPP, MAX_BPP) as f32,
        rotate: core::value(host, &MODULE, "rotate").on(),
        round: core::value(host, &MODULE, "round").on(),
    })
}

/// A key press (presses arrive only while playing): the zoom keys.
fn key_event(host: &mut dyn Host, key: &str, pressed: bool) {
    if !pressed || !core::active(host, &MODULE) {
        return;
    }
    let named = |id: &str| {
        let code = core::value(host, &MODULE, id).key();
        code != NONE && core::vanilla_key_name(code).as_deref() == Some(key)
    };
    let zoom_in = named("zoom_in_key");
    if !zoom_in && !named("zoom_out_key") {
        return;
    }
    ZOOM.set(Some(Zoom {
        bpp: zoomed(current(host), zoom_in),
        base: num(host, "blocks_per_pixel"),
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};

    fn fixtures() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/minimap.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn on(host: &mut Fake) {
        core::set_enabled(host, &MODULE, true);
    }

    fn set(host: &mut Fake, id: &str, v: &str) {
        core::with(host, |s, _| s.set_setting(&MODULE, id, v).unwrap());
    }

    #[test]
    fn zoom_fixtures() {
        for c in fixtures()["zoom"].as_array().unwrap() {
            let got = zoomed(c["bpp"].as_f64().unwrap(), c["in"].as_bool().unwrap());
            assert_eq!(got, c["expect"].as_f64().unwrap(), "{c}");
        }
    }

    #[test]
    fn config_fixtures() {
        for c in fixtures()["config"].as_array().unwrap() {
            restart();
            let mut host = Fake::default();
            on(&mut host);
            for (k, v) in c["settings"].as_object().unwrap() {
                set(&mut host, k, v.as_str().unwrap());
            }
            let got = request(&host).unwrap();
            let e = &c["expect"];
            let name = match got.corner {
                HudCorner::TopLeft => "TOP_LEFT",
                HudCorner::TopRight => "TOP_RIGHT",
                HudCorner::BottomLeft => "BOTTOM_LEFT",
                HudCorner::BottomRight => "BOTTOM_RIGHT",
            };
            assert_eq!(name, e["corner"].as_str().unwrap(), "{c}");
            assert_eq!(got.size as f64, e["size"].as_f64().unwrap(), "{c}");
            assert_eq!(
                got.blocks_per_pixel as f64,
                e["bpp"].as_f64().unwrap(),
                "{c}"
            );
            assert_eq!(got.rotate, e["rotate"].as_bool().unwrap(), "{c}");
            assert_eq!(got.round, e["round"].as_bool().unwrap(), "{c}");
        }
    }

    #[test]
    fn defaults_match_the_spec() {
        assert_eq!(MODULE.settings.len(), 7);
        let m = std::hint::black_box(&MODULE);
        assert!(!m.cheaty && !m.beta && !m.default_on);
        restart();
        let mut host = Fake::default();
        assert_eq!(request(&host), None);
        on(&mut host);
        assert_eq!(
            request(&host),
            Some(Config {
                corner: HudCorner::TopRight,
                size: 128.0,
                blocks_per_pixel: 1.0,
                rotate: true,
                round: false,
            })
        );
    }

    #[test]
    fn zoom_keys_step_until_the_setting_changes() {
        restart();
        let mut host = Fake::default();
        on(&mut host);
        set(&mut host, "zoom_in_key", "73");
        set(&mut host, "zoom_out_key", "79");
        let bpp = |h: &Fake| request(h).unwrap().blocks_per_pixel;
        key_event(&mut host, "key.keyboard.i", true);
        assert_eq!(bpp(&host), 0.5);
        key_event(&mut host, "key.keyboard.i", true);
        assert_eq!(bpp(&host), 0.5);
        key_event(&mut host, "key.keyboard.i", false);
        key_event(&mut host, "key.keyboard.o", true);
        key_event(&mut host, "key.keyboard.o", true);
        assert_eq!(bpp(&host), 2.0);
        key_event(&mut host, "key.keyboard.x", true);
        assert_eq!(bpp(&host), 2.0);
        set(&mut host, "blocks_per_pixel", "3");
        assert_eq!(bpp(&host), 3.0);
        // off: the zoom is forgotten
        core::set_enabled(&mut host, &MODULE, false);
        assert_eq!(request(&host), None);
        on(&mut host);
        assert_eq!(bpp(&host), 3.0);
    }
}
