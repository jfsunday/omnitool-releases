// Freelook (docs/spec/omnitool/movement-camera.md, section 2): an own camera
// rotation that takes the mouse turn while the player keeps its rotation.
// The glue calls `sync` and `turn` from the injections into `turn_player`
// and `camera_view` (game/perspective.rs) and renders from `view`. Added to
// the game as `game::omnitool_freelook`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, SettingDef};
use std::cell::Cell;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "freelook",
    name: "Freelook",
    category: Category::Player,
    description: "Look around without changing where you walk.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "Freelook, Perspective Mod",
    // GLFW_KEY_LEFT_ALT.
    key: 342,
    hold: true,
    settings: &[SettingDef::new(
        "third_person",
        "Switch to third person",
        Kind::Bool(true),
    )],
    hud: None,
    tick: None,
    command: None,
    on_key: None,
    hud_right: None,
    on_action: None,
};

/// The game's camera type (Java `CameraType`, Horizon `perspective::CameraType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    First,
    Back,
    Front,
}

/// Java's `onTurn` maths: the raw mouse deltas times 0.15 (as `Entity.turn`),
/// pitch clamped to ±90, yaw not wrapped. Float maths like Java.
pub fn turned(yaw: f32, pitch: f32, dyaw: f64, dpitch: f64) -> (f32, f32) {
    (
        yaw + dyaw as f32 * 0.15,
        (pitch + dpitch as f32 * 0.15).clamp(-90.0, 90.0),
    )
}

/// The module's state: on (enabled and started with a player), the camera
/// rotation and the camera type to go back to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub on: bool,
    pub yaw: f32,
    pub pitch: f32,
    pub saved: Option<View>,
}

impl Look {
    pub const fn new() -> Self {
        Look {
            on: false,
            yaw: 0.0,
            pitch: 0.0,
            saved: None,
        }
    }

    /// Java's `onEnable`/`onDisable` on the edges of `active`; returns the
    /// camera type to set. Without a player the start waits until there is
    /// one (Java leaves 0/0 there; the spec asks Rust to start from the
    /// player's rotation instead).
    pub fn sync(
        &mut self,
        active: bool,
        third_person: bool,
        player: Option<(f32, f32)>,
        camera: View,
    ) -> Option<View> {
        if active && !self.on {
            let (yaw, pitch) = player?;
            self.on = true;
            self.yaw = yaw;
            self.pitch = pitch;
            if third_person {
                self.saved = Some(camera);
                return Some(View::Back);
            }
        } else if !active && self.on {
            self.on = false;
            return self.saved.take();
        }
        None
    }

    /// Java's `onTurn`: true = the turn is used up (the player keeps its
    /// rotation).
    pub fn turn(&mut self, dyaw: f64, dpitch: f64) -> bool {
        if !self.on {
            return false;
        }
        (self.yaw, self.pitch) = turned(self.yaw, self.pitch, dyaw, dpitch);
        true
    }

    /// The rotation to render from (`applyCamera`), None = the player's.
    pub fn view(&self) -> Option<(f32, f32)> {
        self.on.then_some((self.yaw, self.pitch))
    }
}

impl Default for Look {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    static LOOK: Cell<Look> = const { Cell::new(Look::new()) };
}

/// Start or stop on the edges of the module's state; returns the camera type
/// the game should switch to.
pub fn sync(host: &dyn Host, camera: View) -> Option<View> {
    let active = core::active(host, &MODULE);
    let third = active && core::value(host, &MODULE, "third_person").on();
    let player = host.player().map(|p| (p.yaw, p.pitch));
    let mut l = LOOK.get();
    let set = l.sync(active, third, player, camera);
    LOOK.set(l);
    set
}

/// The mouse turn (raw deltas, as `turn_player` gets them).
pub fn turn(dyaw: f64, dpitch: f64) -> bool {
    let mut l = LOOK.get();
    let used = l.turn(dyaw, dpitch);
    LOOK.set(l);
    used
}

/// The camera rotation while Freelook runs.
pub fn view() -> Option<(f32, f32)> {
    LOOK.get().view()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/movement-camera.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn turn_fixtures() {
        for case in fixtures()["freelook_turn"].as_array().unwrap() {
            let mut l = Look::new();
            l.sync(
                true,
                false,
                Some((
                    case["yaw"].as_f64().unwrap() as f32,
                    case["pitch"].as_f64().unwrap() as f32,
                )),
                View::First,
            );
            for op in case["ops"].as_array().unwrap() {
                assert!(l.turn(op["dyaw"].as_f64().unwrap(), op["dpitch"].as_f64().unwrap()));
                let (yaw, pitch) = l.view().unwrap();
                assert!(
                    (yaw as f64 - op["yaw"].as_f64().unwrap()).abs() < 1e-4,
                    "{case}"
                );
                assert!(
                    (pitch as f64 - op["pitch"].as_f64().unwrap()).abs() < 1e-4,
                    "{case}"
                );
            }
        }
    }

    #[test]
    fn third_person_saved_and_restored() {
        let mut l = Look::new();
        assert_eq!(
            l.sync(true, true, Some((10.0, 5.0)), View::Front),
            Some(View::Back)
        );
        assert_eq!(l.view(), Some((10.0, 5.0)));
        assert_eq!(l.sync(true, true, Some((99.0, 0.0)), View::Back), None);
        assert_eq!(
            l.sync(false, true, Some((10.0, 5.0)), View::Back),
            Some(View::Front)
        );
        assert_eq!(l.view(), None);
        assert!(!l.turn(10.0, 10.0));
    }

    #[test]
    fn no_third_person_keeps_camera() {
        let mut l = Look::new();
        assert_eq!(l.sync(true, false, Some((0.0, 0.0)), View::First), None);
        assert_eq!(l.sync(false, false, None, View::First), None);
    }

    #[test]
    fn waits_for_player() {
        let mut l = Look::new();
        assert_eq!(l.sync(true, true, None, View::First), None);
        assert!(!l.on);
        assert_eq!(
            l.sync(true, true, Some((45.0, -10.0)), View::First),
            Some(View::Back)
        );
        assert_eq!(l.view(), Some((45.0, -10.0)));
    }

    #[test]
    fn third_person_off_while_on_still_restores() {
        // Spec 2.5: `savedCamera` stays set and is restored on disable.
        let mut l = Look::new();
        l.sync(true, true, Some((0.0, 0.0)), View::First);
        assert_eq!(l.sync(true, false, Some((0.0, 0.0)), View::Back), None);
        assert_eq!(l.sync(false, false, None, View::Back), Some(View::First));
    }
}
