// Zoom (docs/spec/omnitool/movement-camera.md, section 1): the factor maths
// as pure functions, checked against tests/fixtures/movement-camera.json, and
// the module around them. Horizon's `native_zoom` takes the factor and the
// scale of mouse turning; `native_wheel` gives the wheel. Added to the game
// as `game::omnitool_zoom`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, SettingDef};
use std::cell::Cell;
use std::time::Instant;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "zoom",
    name: "Zoom",
    category: Category::Player,
    description: "Zoom like OptiFine. Default: hold C.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "Ok Zoomer, Zoomify",
    key: 67,
    hold: true,
    settings: &[
        SettingDef::new(
            "factor",
            "Zoom factor",
            Kind::Number {
                default: 4.0,
                min: 1.5,
                max: 10.0,
                step: 0.5,
            },
        ),
        SettingDef::new("scroll", "Mouse wheel adjusts zoom", Kind::Bool(true)),
        SettingDef::new("smooth", "Smooth zoom animation", Kind::Bool(true)),
        SettingDef::new(
            "smooth_speed",
            "Animation speed",
            Kind::Number {
                default: 12.0,
                min: 4.0,
                max: 30.0,
                step: 1.0,
            },
        ),
        // Horizon has no hook for the game's cinematic camera: stored, no effect.
        SettingDef::new(
            "cinematic",
            "Smooth camera while zooming",
            Kind::Bool(false),
        ),
        SettingDef::new("slow_mouse", "Lower mouse sensitivity", Kind::Bool(true)),
        SettingDef::new(
            "sensitivity",
            "Sensitivity while zoomed",
            Kind::Number {
                default: 1.0,
                min: 0.2,
                max: 2.0,
                step: 0.1,
            },
        ),
    ],
    hud: None,
    tick: None,
    command: None,
    on_key: None,
    hud_right: None,
    on_action: None,
};

/// Upper limit of the whole factor (Java `Zoom.MAX`).
const MAX: f64 = 10.0;

// ------------------------------------------------------------ pure maths

/// Java's `target()`: the factor aimed at.
pub fn target(active: bool, factor: f64, wheel: f64) -> f64 {
    if active {
        (factor * wheel).clamp(1.0, MAX)
    } else {
        1.0
    }
}

/// Java's `onScroll` for an active module with `scroll` on: the new wheel
/// factor, or None when `amount` is 0 (the wheel is not used).
pub fn scrolled(factor: f64, wheel: f64, amount: f64) -> Option<f64> {
    if amount == 0.0 {
        return None;
    }
    let t = (factor * wheel * if amount > 0.0 { 1.25 } else { 0.8 }).clamp(1.0, MAX);
    Some(t / factor)
}

/// Java's `current()` after the time step: `dt` seconds (0 on the first call,
/// at most 0.25) towards `target`, eased in log space.
pub fn eased(smooth: bool, current: f64, target: f64, dt: f64, speed: f64) -> f64 {
    if !smooth {
        return target;
    }
    let k = 1.0 - (-dt * speed).exp();
    let now = (current.ln() + (target.ln() - current.ln()) * k).exp();
    if (now - target).abs() < 0.002 {
        target
    } else {
        now
    }
}

/// Java's `mouseScale()`.
pub fn mouse_scale(slow_mouse: bool, sensitivity: f64, current: f64) -> f64 {
    if !slow_mouse || current <= 1.001 {
        1.0
    } else {
        (sensitivity / current).min(1.0)
    }
}

/// Java's `fovMultiplier()`.
#[allow(dead_code)] // Horizon applies the factor itself; the fixtures check this
pub fn fov_multiplier(current: f64) -> f32 {
    (1.0 / current) as f32
}

// ------------------------------------------------------------ module

#[derive(Clone, Copy)]
struct Zoom {
    /// The wheel's factor on the base factor.
    wheel: f64,
    /// The animated factor.
    current: f64,
    last: Option<Instant>,
    /// The switch as the last call saw it: onEnable/onDisable reset `wheel`.
    was_on: bool,
}

thread_local! {
    static ZOOM: Cell<Zoom> = const { Cell::new(Zoom { wheel: 1.0, current: 1.0, last: None, was_on: false }) };
}

fn num(host: &dyn Host, id: &str) -> f64 {
    core::value(host, &MODULE, id).num()
}

fn flag(host: &dyn Host, id: &str) -> bool {
    core::value(host, &MODULE, id).on()
}

/// Java's onEnable/onDisable: `wheel = 1` whenever the switch changed.
fn sync(host: &dyn Host, z: &mut Zoom) {
    let on = core::enabled(host, &MODULE);
    if on != z.was_on || !on {
        z.was_on = on;
        z.wheel = 1.0;
    }
}

/// Java's `onScroll`: whether the module used the wheel (then it is kept
/// from the hotbar).
pub fn wheel(host: &dyn Host, steps: f32) -> bool {
    let mut z = ZOOM.get();
    sync(host, &mut z);
    let used = core::active(host, &MODULE)
        && flag(host, "scroll")
        && scrolled(num(host, "factor"), z.wheel, steps as f64)
            .inspect(|w| z.wheel = *w)
            .is_some();
    ZOOM.set(z);
    used
}

/// Java's `current()` at time `now`, then the request for Horizon:
/// the factor, and the scale of mouse turning. None while there is no zoom.
pub fn request_at(host: &dyn Host, now: Instant) -> Option<(f32, f32)> {
    let mut z = ZOOM.get();
    sync(host, &mut z);
    let t = target(core::active(host, &MODULE), num(host, "factor"), z.wheel);
    let dt = z.last.map_or(0.0, |l| {
        now.saturating_duration_since(l).as_secs_f64().min(0.25)
    });
    z.last = Some(now);
    z.current = eased(
        flag(host, "smooth"),
        z.current,
        t,
        dt,
        num(host, "smooth_speed"),
    );
    ZOOM.set(z);
    if z.current <= 1.0 {
        return None;
    }
    let scale = mouse_scale(
        flag(host, "slow_mouse"),
        num(host, "sensitivity"),
        z.current,
    );
    Some((z.current as f32, scale as f32))
}

/// `native_zoom`: the zoom to ask Horizon for, as (factor, turn).
pub fn request(host: &dyn Host) -> Option<(f32, f32)> {
    request_at(host, Instant::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};
    use std::time::Duration;

    fn fixtures() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/movement-camera.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn scroll_fixtures() {
        for case in fixtures()["zoom_scroll"].as_array().unwrap() {
            let factor = case["factor"].as_f64().unwrap();
            let mut wheel = case["wheel"].as_f64().unwrap();
            for op in case["ops"].as_array().unwrap() {
                let used = scrolled(factor, wheel, op["amount"].as_f64().unwrap());
                assert_eq!(used.is_some(), op["used"].as_bool().unwrap(), "{case}");
                if let Some(w) = used {
                    wheel = w;
                }
                assert!(close(wheel, op["wheel"].as_f64().unwrap()), "{case}");
                assert!(
                    close(target(true, factor, wheel), op["target"].as_f64().unwrap()),
                    "{case}"
                );
            }
        }
    }

    #[test]
    fn smooth_fixtures() {
        for c in fixtures()["zoom_smooth"].as_array().unwrap() {
            let got = eased(
                c["smooth"].as_bool().unwrap(),
                c["current"].as_f64().unwrap(),
                c["target"].as_f64().unwrap(),
                c["dt"].as_f64().unwrap(),
                c["speed"].as_f64().unwrap(),
            );
            assert!(close(got, c["expect"].as_f64().unwrap()), "{c}: {got}");
        }
    }

    #[test]
    fn mouse_scale_and_fov_fixtures() {
        for c in fixtures()["zoom_mouse_scale"].as_array().unwrap() {
            let got = mouse_scale(
                c["slow_mouse"].as_bool().unwrap(),
                c["sensitivity"].as_f64().unwrap(),
                c["current"].as_f64().unwrap(),
            );
            assert!(close(got, c["expect"].as_f64().unwrap()), "{c}: {got}");
        }
        for c in fixtures()["zoom_fov_multiplier"].as_array().unwrap() {
            let got = fov_multiplier(c["current"].as_f64().unwrap()) as f64;
            assert!(close(got, c["expect"].as_f64().unwrap()), "{c}");
        }
    }

    #[test]
    fn defaults_match_the_spec() {
        assert_eq!((MODULE.key, MODULE.hold), (67, true));
        assert_eq!(MODULE.settings.len(), 7);
        let m = std::hint::black_box(&MODULE);
        assert!(!m.cheaty && !m.beta && !m.default_on);
    }

    #[test]
    fn hold_key_zooms_and_resets_the_wheel() {
        restart();
        ZOOM.set(Zoom {
            wheel: 1.0,
            current: 1.0,
            last: None,
            was_on: false,
        });
        let host = Fake::default();
        let t0 = Instant::now();
        assert_eq!(request_at(&host, t0), None);
        // not on: the wheel is not used
        assert!(!wheel(&host, 1.0));
        let mut host = host;
        core::key_event(&mut host, "key.keyboard.c", true, true);
        assert!(core::enabled(&host, &MODULE));
        // first call: dt 0, no jump
        assert_eq!(request_at(&host, t0).map(|r| r.0), None);
        let (f, turn) = request_at(&host, t0 + Duration::from_millis(250)).unwrap();
        assert!((f as f64 - 3.7332338016).abs() < 1e-4 && (turn - 1.0 / f).abs() < 1e-3);
        assert!(wheel(&host, 1.0));
        core::key_event(&mut host, "key.keyboard.c", false, false);
        assert!(!core::enabled(&host, &MODULE));
        // switching off reset the wheel: back to the base factor next time
        request_at(&host, t0 + Duration::from_millis(300));
        core::key_event(&mut host, "key.keyboard.c", true, true);
        let mut f = 0.0;
        for i in 1..=20 {
            f = request_at(&host, t0 + Duration::from_millis(250 * (i + 2)))
                .unwrap()
                .0;
        }
        assert!((f - 4.0).abs() < 0.01, "{f}");
    }

    #[test]
    fn keys_only_press_while_playing() {
        restart();
        let mut host = Fake::default();
        core::key_event(&mut host, "key.keyboard.c", true, false);
        assert!(!core::enabled(&host, &MODULE));
        core::key_event(&mut host, "key.keyboard.x", true, true);
        assert!(!core::enabled(&host, &MODULE));
    }
}
