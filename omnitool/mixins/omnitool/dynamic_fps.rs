// Dynamic FPS (docs/spec/omnitool/session-misc.md, section 5): the pure
// decisions (window state, frame limit, volume, frame skip) and the module's
// settings. Horizon has no hook that changes the frame limit, the master
// volume or skips a frame (`native_frame` is read-only, `max_fps` would also
// be the saved option), so nothing applies the results in the game yet: the
// module is registered with its settings and `state()` is kept up to date by
// `tick`, the effect is a stub. Added to the game as
// `game::omnitool_dynamic_fps`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo, WindowState};
use super::omnitool_setting::{Kind, NONE, SettingDef};
use std::cell::Cell;

/// Java's `UNCHANGED`: a frame limit of 61 or more leaves the limit alone.
#[allow(dead_code)]
pub const UNCHANGED: i32 = 61;

const fn fps(id: &'static str, name: &'static str, default: f64) -> SettingDef {
    SettingDef::new(
        id,
        name,
        Kind::Number {
            default,
            min: 0.0,
            max: 61.0,
            step: 1.0,
        },
    )
}

const fn volume(id: &'static str, name: &'static str, default: f64) -> SettingDef {
    SettingDef::new(
        id,
        name,
        Kind::Number {
            default,
            min: 0.0,
            max: 100.0,
            step: 5.0,
        },
    )
}

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "dynamic_fps",
    name: "Dynamic FPS",
    category: Category::Misc,
    description: "Lowers FPS and volume when the game is in the background, minimized or you are AFK.",
    default_on: false,
    cheaty: false,
    beta: true,
    fair_variant: false,
    replaces: "Dynamic FPS",
    settings: &[
        fps("focused_fps", "Focused: frame limit", 61.0),
        volume("focused_volume", "Focused: volume %", 100.0),
        fps("hovered_fps", "Hovered (unfocused): frame limit", 60.0),
        volume("hovered_volume", "Hovered (unfocused): volume %", 100.0),
        fps("unfocused_fps", "Unfocused: frame limit", 15.0),
        volume("unfocused_volume", "Unfocused: volume %", 25.0),
        fps("invisible_fps", "Minimized: frame limit", 1.0),
        volume("invisible_volume", "Minimized: volume %", 0.0),
        fps("afk_fps", "AFK: frame limit", 10.0),
        volume("afk_volume", "AFK: volume %", 100.0),
        SettingDef::new(
            "afk_minutes",
            "AFK after minutes without input (0 = off)",
            Kind::Number {
                default: 10.0,
                min: 0.0,
                max: 60.0,
                step: 1.0,
            },
        ),
        SettingDef::new(
            "pause_invisible",
            "Don't render while minimized",
            Kind::Bool(true),
        ),
    ],
    command: None,
    key: NONE,
    hold: false,
    hud: None,
    tick: Some(tick),
    on_key: Some(key_event),
    hud_right: None,
    on_action: None,
};

/// Java's `State`, in its order (the settings use the lower-case name).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Focused,
    Hovered,
    Unfocused,
    Invisible,
    Afk,
}

impl State {
    #[allow(dead_code)]
    fn prefix(self) -> &'static str {
        match self {
            State::Focused => "focused",
            State::Hovered => "hovered",
            State::Unfocused => "unfocused",
            State::Invisible => "invisible",
            State::Afk => "afk",
        }
    }
}

/// Java's `detect()`: minimized beats AFK beats focus beats hover.
pub fn detect(w: WindowState, afk_minutes: i64, idle_ms: i64) -> State {
    if w.invisible {
        State::Invisible
    } else if afk_minutes > 0 && idle_ms > afk_minutes * 60_000 {
        State::Afk
    } else if w.focused {
        State::Focused
    } else if w.hovered {
        State::Hovered
    } else {
        State::Unfocused
    }
}

/// Java's `frameLimit` result for a game limit `original` (260 and more =
/// unlimited) and the state's `limit` setting.
#[allow(dead_code)]
pub fn frame_limit(original: i32, limit: i32) -> i32 {
    if limit < UNCHANGED {
        original.min(limit.max(1))
    } else {
        original
    }
}

/// Java's `skipRendering`: never the focused game, minimized by
/// `pause_invisible`, else a frame limit of 0.
#[allow(dead_code)]
pub fn skip_rendering(state: State, pause_invisible: bool, limit: i32) -> bool {
    if state == State::Invisible && pause_invisible {
        return true;
    }
    state != State::Focused && limit <= 0
}

thread_local! {
    /// Java's `state`, from the last tick.
    static STATE: Cell<State> = const { Cell::new(State::Focused) };
    /// Java's `lastInput` (ms; 0 until the first input or tick).
    static LAST_INPUT: Cell<u64> = const { Cell::new(0) };
}

/// Java's `currentState()`.
#[allow(dead_code)]
pub fn state() -> State {
    STATE.with(Cell::get)
}

#[allow(dead_code)]
fn setting(host: &dyn Host, state: State, what: &str) -> i32 {
    core::value(host, &MODULE, &format!("{}_{what}", state.prefix())).num() as i32
}

/// The frame limit the current state asks for, given the game's `original`.
#[allow(dead_code)]
pub fn wanted_limit(host: &dyn Host, original: i32) -> i32 {
    frame_limit(original, setting(host, state(), "fps"))
}

/// The master volume factor the current state asks for.
#[allow(dead_code)]
pub fn wanted_volume(host: &dyn Host) -> f64 {
    f64::from(setting(host, state(), "volume")) / 100.0
}

/// Whether this frame is not drawn.
#[allow(dead_code)]
pub fn wanted_skip(host: &dyn Host) -> bool {
    let pause = core::value(host, &MODULE, "pause_invisible").on();
    skip_rendering(state(), pause, setting(host, state(), "fps"))
}

/// Java's `InputActivityMixin`: a key or button resets the AFK timer. Mouse
/// moves and wheel steps have no event in Horizon's `native_key`.
fn key_event(host: &mut dyn Host, _key: &str, _pressed: bool) {
    LAST_INPUT.with(|l| l.set(host.now_ms()));
}

fn tick(host: &mut dyn Host) {
    let now = host.now_ms();
    let idle = match LAST_INPUT.with(Cell::get) {
        0 => {
            LAST_INPUT.with(|l| l.set(now));
            0
        }
        last => now.saturating_sub(last) as i64,
    };
    let next = if core::active(host, &MODULE) {
        let minutes = core::value(host, &MODULE, "afk_minutes").num() as i64;
        detect(host.window_state(), minutes, idle)
    } else {
        State::Focused
    };
    STATE.with(|s| s.set(next));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};

    fn w(focused: bool, hovered: bool, invisible: bool) -> WindowState {
        WindowState {
            focused,
            hovered,
            invisible,
        }
    }

    #[test]
    fn detect_priority() {
        let min = 60_000;
        assert_eq!(detect(w(true, true, true), 10, 0), State::Invisible);
        assert_eq!(detect(w(true, true, false), 10, 10 * min + 1), State::Afk);
        assert_eq!(detect(w(true, true, false), 10, 10 * min), State::Focused);
        assert_eq!(detect(w(true, true, false), 0, 99 * min), State::Focused);
        assert_eq!(detect(w(false, true, false), 10, 0), State::Hovered);
        assert_eq!(detect(w(false, false, false), 10, 0), State::Unfocused);
        assert_eq!(detect(w(false, true, false), 10, 11 * min), State::Afk);
    }

    #[test]
    fn limits() {
        assert_eq!(frame_limit(260, 61), 260);
        assert_eq!(frame_limit(260, 15), 15);
        assert_eq!(frame_limit(30, 60), 30);
        assert_eq!(frame_limit(120, 0), 1);
        assert_eq!(frame_limit(0, 15), 0);
    }

    #[test]
    fn skipping() {
        assert!(skip_rendering(State::Invisible, true, 1));
        assert!(!skip_rendering(State::Invisible, false, 1));
        assert!(skip_rendering(State::Invisible, false, 0));
        assert!(skip_rendering(State::Unfocused, false, 0));
        assert!(!skip_rendering(State::Focused, true, 0));
    }

    #[test]
    fn defaults_and_tick() {
        restart();
        let mut h = Fake::default();
        assert_eq!(MODULE.settings.len(), 12);
        // off by default: always focused, no change
        tick(&mut h);
        assert_eq!(state(), State::Focused);
        core::set_enabled(&mut h, &MODULE, true);
        h.window = w(false, false, false);
        tick(&mut h);
        assert_eq!(state(), State::Unfocused);
        assert_eq!(wanted_limit(&h, 260), 15);
        assert_eq!(wanted_volume(&h), 0.25);
        assert!(!wanted_skip(&h));
        h.window = w(false, false, true);
        tick(&mut h);
        assert_eq!(wanted_volume(&h), 0.0);
        assert!(wanted_skip(&h));
    }

    #[test]
    fn afk_after_idle_and_input_resets() {
        restart();
        LAST_INPUT.with(|l| l.set(0));
        let mut h = Fake::default();
        core::set_enabled(&mut h, &MODULE, true);
        h.window = w(true, true, false);
        h.now = 1_000;
        tick(&mut h);
        h.now = 1_000 + 10 * 60_000 + 1;
        tick(&mut h);
        assert_eq!(state(), State::Afk);
        key_event(&mut h, "key.keyboard.w", true);
        h.now += 1;
        tick(&mut h);
        assert_eq!(state(), State::Focused);
    }

    #[test]
    fn registered() {
        assert!(
            crate::omnitool_registry::MODULES
                .iter()
                .any(|m| m.id == "dynamic_fps")
        );
    }
}
