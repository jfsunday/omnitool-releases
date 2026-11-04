// Toggle Sneak (docs/spec/omnitool/movement-camera.md, section 4): holds the
// sneak key every tick while the module is active and no screen is open; the
// key is released while a screen is open or the module is off. The key press
// state machine (`latch`) is shared with Toggle Sprint. Added to the game as
// `game::omnitool_toggle_sneak`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_workflows::Input;
use std::cell::Cell;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "toggle_sneak",
    name: "Toggle Sneak",
    category: Category::Player,
    description: "Always sneak while enabled.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "",
    settings: &[],
    command: None,
    key: super::omnitool_setting::NONE,
    hold: false,
    hud: None,
    tick: Some(tick),
    on_key: None,
    hud_right: None,
    on_action: None,
};

thread_local! {
    /// Java's `applied`: the key was pressed by this module and not yet released.
    static APPLIED: Cell<bool> = const { Cell::new(false) };
}

/// One tick of Java's `applied` pattern: `want` presses `key` (and returns
/// true); otherwise a key this module pressed is released once. Returns the
/// new `applied`.
pub fn latch(key: &str, applied: bool, want: bool, input: &mut dyn Input) -> bool {
    if want {
        input.set_key(key, true);
        true
    } else {
        if applied {
            input.set_key(key, false);
        }
        false
    }
}

fn tick(host: &mut dyn Host) {
    let want = core::active(host, &MODULE) && !host.screen_open();
    if let Some(input) = host.input() {
        APPLIED.with(|a| a.set(latch("sneak", a.get(), want, input)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Keys(Vec<(String, bool)>);

    impl Input for Keys {
        fn set_key(&mut self, key: &str, down: bool) {
            self.0.push((key.to_owned(), down));
        }
    }

    #[test]
    fn presses_every_tick_and_releases_once() {
        let mut k = Keys::default();
        let mut applied = false;
        for want in [true, true, false, false] {
            applied = latch("sneak", applied, want, &mut k);
        }
        let s = |d| ("sneak".to_owned(), d);
        assert_eq!(k.0, [s(true), s(true), s(false)]);
        assert!(!applied);
    }

    #[test]
    fn never_pressed_never_released() {
        let mut k = Keys::default();
        assert!(!latch("sprint", false, false, &mut k));
        assert!(k.0.is_empty());
    }

    #[test]
    fn screen_releases_and_reclaims() {
        let mut k = Keys::default();
        let mut applied = latch("sneak", false, true, &mut k);
        applied = latch("sneak", applied, false, &mut k); // screen opened
        applied = latch("sneak", applied, true, &mut k); // closed again
        assert!(applied);
        let downs: Vec<bool> = k.0.iter().map(|e| e.1).collect();
        assert_eq!(downs, [true, false, true]);
    }

    #[test]
    fn registered() {
        assert!(
            crate::omnitool_registry::MODULES
                .iter()
                .any(|m| m.id == "toggle_sneak")
        );
    }
}
