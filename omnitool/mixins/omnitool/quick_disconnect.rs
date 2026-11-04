// Quick Disconnect (docs/spec/omnitool/session-misc.md, section 2): one key
// leaves the world or server, by default after pressing it twice within 1.5
// seconds. Java polls the key every tick; here the rising edge is the key
// event (`native_key`), with the same conditions (module active, no screen,
// key bound). Added to the game as `game::omnitool_quick_disconnect`; also a
// module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, NONE, SettingDef};
use std::cell::Cell;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "quick_disconnect",
    name: "Quick Disconnect",
    category: Category::Misc,
    description: "Leave the world/server with one key (set it below).",
    default_on: true,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "BindQuit",
    settings: &[
        SettingDef::new("quit_key", "Disconnect key", Kind::Key(NONE)),
        SettingDef::new("confirm", "Press twice to confirm", Kind::Bool(true)),
    ],
    command: None,
    key: NONE,
    hold: false,
    hud: None,
    tick: None,
    on_key: Some(key_event),
    hud_right: None,
    on_action: None,
};

/// How long the first press arms the second one (ms).
pub const CONFIRM_WINDOW_MS: u64 = 1500;
/// Java's action bar hint (no German line in `de.json`).
pub const HINT: &str = "Press again to disconnect";

thread_local! {
    /// Java's `armedUntil` (wall clock ms; 0 = not armed).
    static ARMED_UNTIL: Cell<u64> = const { Cell::new(0) };
}

/// What a press does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// First press: show the hint, wait for the second.
    Arm,
    /// Leave the world now.
    Disconnect,
}

/// Java's `press()`: the outcome and the new `armedUntil`.
pub fn press(confirm: bool, now: u64, armed_until: u64) -> (Press, u64) {
    if confirm && now > armed_until {
        (Press::Arm, now + CONFIRM_WINDOW_MS)
    } else {
        (Press::Disconnect, 0)
    }
}

/// Java's `press()` on the game.
pub fn press_now(host: &mut dyn Host) {
    let confirm = core::value(host, &MODULE, "confirm").on();
    let (what, armed) = press(confirm, host.now_ms(), ARMED_UNTIL.with(Cell::get));
    ARMED_UNTIL.with(|a| a.set(armed));
    match what {
        Press::Arm => {
            let text = core::tr(host, HINT);
            host.action_bar(&text);
        }
        Press::Disconnect => host.disconnect(),
    }
}

fn key_event(host: &mut dyn Host, key: &str, pressed: bool) {
    if !pressed || !core::active(host, &MODULE) || host.screen_open() {
        return;
    }
    let code = core::value(host, &MODULE, "quit_key").key();
    if code != NONE && core::vanilla_key_name(code).as_deref() == Some(key) {
        press_now(host);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};

    #[test]
    fn first_press_arms_second_disconnects() {
        let (p, armed) = press(true, 10_000, 0);
        assert_eq!((p, armed), (Press::Arm, 11_500));
        assert_eq!(press(true, 11_500, armed), (Press::Disconnect, 0));
    }

    #[test]
    fn late_second_press_arms_again() {
        let (_, armed) = press(true, 10_000, 0);
        assert_eq!(press(true, 11_501, armed), (Press::Arm, 13_001));
    }

    #[test]
    fn without_confirm_disconnects_at_once() {
        assert_eq!(press(false, 10_000, 0), (Press::Disconnect, 0));
        // confirm switched off while armed
        assert_eq!(press(false, 10_100, 11_500), (Press::Disconnect, 0));
    }

    fn bound() -> Fake {
        restart();
        ARMED_UNTIL.with(|a| a.set(0));
        let mut h = Fake::default();
        // key.keyboard.k = GLFW 75
        core::with(&mut h, |s, _| {
            s.set_setting(&MODULE, "quit_key", "75").unwrap();
        });
        h
    }

    #[test]
    fn key_press_flow() {
        let mut h = bound();
        h.now = 5_000;
        key_event(&mut h, "key.keyboard.k", true);
        assert_eq!(h.bar, [HINT]);
        assert_eq!(h.disconnects, 0);
        h.now = 6_000;
        key_event(&mut h, "key.keyboard.k", false); // release does nothing
        key_event(&mut h, "key.keyboard.j", true); // other key
        key_event(&mut h, "key.keyboard.k", true);
        assert_eq!(h.disconnects, 1);
    }

    #[test]
    fn ignored_in_screen_and_when_off() {
        let mut h = bound();
        h.screen = true;
        key_event(&mut h, "key.keyboard.k", true);
        assert!(h.bar.is_empty());
        h.screen = false;
        core::set_enabled(&mut h, &MODULE, false);
        key_event(&mut h, "key.keyboard.k", true);
        assert!(h.bar.is_empty() && h.disconnects == 0);
    }

    #[test]
    fn unbound_key_does_nothing() {
        restart();
        let mut h = Fake::default();
        key_event(&mut h, "key.keyboard.k", true);
        assert!(h.bar.is_empty());
    }

    #[test]
    fn registered() {
        assert!(
            crate::omnitool_registry::MODULES
                .iter()
                .any(|m| m.id == "quick_disconnect")
        );
    }
}
