// Session Stats (docs/spec/omnitool/hud.md, section 7): playtime, distance,
// deaths, blocks mined and mobs killed of this session, shown as Omni
// Overlay entries. Playtime, distance and deaths follow the spec's tick
// rules. Blocks mined and mobs killed come from the game's own statistics
// (`mined`, `killed`; single player only, Horizon counts nothing on
// servers) instead of Java's mixins, so they differ in edge cases (a mined
// block counts only when it dropped, projectile kills count). Blocks placed
// needs a mixin and stays out. The `Reset stats` button is the Settings page's.
// Added to the game as `game::omnitool_session_stats`; also a module of
// this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo, SessionSample};
use super::omnitool_overlay_entries::SessionInfo;
use super::omnitool_setting::{Kind, SettingDef};
use std::cell::RefCell;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "session_stats",
    name: "Session Stats",
    category: Category::Hud,
    description: "Playtime, blocks mined/placed, distance, deaths and kills of this session. Shown in the Omni Overlay.",
    default_on: true,
    cheaty: false,
    beta: true,
    fair_variant: false,
    replaces: "",
    settings: &[
        SettingDef::new(
            "reset_join",
            "Reset when joining a world/server",
            Kind::Bool(true),
        ),
        SettingDef::new("reset", "Reset stats", Kind::Action),
    ],
    command: None,
    key: super::omnitool_setting::NONE,
    hold: false,
    hud: None,
    tick: Some(tick),
    on_key: None,
    hud_right: None,
    on_action: Some(action),
};

/// A client freeze counts at most this long per tick.
const MAX_STEP_NANOS: i64 = 1_000_000_000;
/// Moves of this length or more are teleports.
const TELEPORT: f64 = 20.0;

/// Java's `SessionStats` state.
#[derive(Default)]
pub struct Session {
    play_nanos: i64,
    mined: i64,
    placed: i64,
    deaths: i64,
    kills: i64,
    walked: f64,
    flown: f64,
    join: Option<u64>,
    last_nanos: i64,
    last_pos: Option<[f64; 3]>,
    last_dim: String,
    dead: bool,
    /// The game's totals at the last tick, to count what came after.
    base_mined: i64,
    base_kills: i64,
}

impl Session {
    pub fn clear(&mut self) {
        self.play_nanos = 0;
        self.mined = 0;
        self.placed = 0;
        self.deaths = 0;
        self.kills = 0;
        self.walked = 0.0;
        self.flown = 0.0;
    }

    /// One client tick (spec 7.3). `pos`/`dim` are the player's.
    pub fn step(
        &mut self,
        now: i64,
        active: bool,
        reset_join: bool,
        s: &SessionSample,
        pos: [f64; 3],
        dim: &str,
    ) {
        if self.join != Some(s.join) {
            self.join = Some(s.join);
            if reset_join {
                self.clear();
            }
            self.last_pos = None;
            self.last_nanos = now;
        }
        if !active {
            self.last_nanos = now;
            self.last_pos = None;
            self.base_mined = s.mined_total;
            self.base_kills = s.kills_total;
            return;
        }
        self.play_nanos += (now - self.last_nanos).min(MAX_STEP_NANOS);
        self.last_nanos = now;
        // A respawn or another dimension is a new player: no jump counts.
        let fresh = self.last_pos.is_none() || self.last_dim != dim || (self.dead && !s.dead);
        if fresh {
            self.last_dim = dim.to_owned();
            self.dead = s.dead;
            self.base_mined = s.mined_total;
            self.base_kills = s.kills_total;
        } else if let Some(last) = self.last_pos
            && !s.passenger
            && !s.spectator
        {
            let d = [pos[0] - last[0], pos[1] - last[1], pos[2] - last[2]];
            let all = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if all < TELEPORT {
                if s.flying || s.fall_flying {
                    self.flown += all;
                } else {
                    self.walked += (d[0] * d[0] + d[2] * d[2]).sqrt();
                }
            }
        }
        self.last_pos = Some(pos);
        if s.dead && !self.dead {
            self.deaths += 1;
        }
        self.dead = s.dead;
        self.mined += (s.mined_total - self.base_mined).max(0);
        self.kills += (s.kills_total - self.base_kills).max(0);
        self.base_mined = s.mined_total;
        self.base_kills = s.kills_total;
    }

    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            play_seconds: self.play_nanos / 1_000_000_000,
            mined: self.mined,
            placed: self.placed,
            walked: self.walked,
            flown: self.flown,
            deaths: self.deaths,
            kills: self.kills,
        }
    }
}

thread_local! {
    static SESSION: RefCell<Session> = RefCell::new(Session::default());
}

fn tick(host: &mut dyn Host) {
    let (Some(sample), Some(p)) = (host.session_sample(), host.player()) else {
        return;
    };
    let active = core::active(host, &MODULE);
    let reset_join = core::value(host, &MODULE, "reset_join").on();
    let now = host.nano_time();
    SESSION.with_borrow_mut(|s| {
        s.step(
            now,
            active,
            reset_join,
            &sample,
            [p.x, p.y, p.z],
            &p.dimension,
        )
    });
}

/// `Reset stats` button: Java's `SessionStats.reset`.
fn action(_host: &mut dyn Host, setting: &str) {
    if setting == "reset" {
        SESSION.with_borrow_mut(Session::clear);
    }
}

/// The overlay's numbers; None while the module is off.
pub fn info(host: &dyn Host) -> Option<SessionInfo> {
    core::active(host, &MODULE).then(|| SESSION.with_borrow(Session::info))
}

#[cfg(test)]
pub fn test_add_deaths(n: i64) {
    SESSION.with_borrow_mut(|s| s.deaths += n);
}

#[cfg(test)]
pub fn test_deaths() -> i64 {
    SESSION.with_borrow(|s| s.info().deaths)
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: i64 = 1_000_000_000;

    fn run(s: &mut Session, now: i64, x: f64, smp: &SessionSample) {
        s.step(now, true, true, smp, [x, 64.0, 0.0], "minecraft:overworld");
    }

    #[test]
    fn walking_flying_and_teleports() {
        let mut s = Session::default();
        let mut smp = SessionSample::default();
        run(&mut s, 0, 0.0, &smp);
        run(&mut s, 50_000_000, 1.0, &smp);
        smp.flying = true;
        run(&mut s, 100_000_000, 3.0, &smp);
        run(&mut s, 150_000_000, 100.0, &smp);
        let i = s.info();
        assert_eq!((i.walked, i.flown), (1.0, 2.0));
    }

    #[test]
    fn passengers_do_not_count_and_freezes_are_capped() {
        let mut s = Session::default();
        let mut smp = SessionSample::default();
        run(&mut s, 0, 0.0, &smp);
        smp.passenger = true;
        run(&mut s, 10 * S, 5.0, &smp);
        let i = s.info();
        assert_eq!((i.walked, i.play_seconds), (0.0, 1));
    }

    #[test]
    fn deaths_count_once_and_respawn_does_not_jump() {
        let mut s = Session::default();
        let mut smp = SessionSample::default();
        run(&mut s, 0, 0.0, &smp);
        smp.dead = true;
        run(&mut s, 1, 0.0, &smp);
        run(&mut s, 2, 0.0, &smp);
        smp.dead = false;
        run(&mut s, 3, 10.0, &smp);
        let i = s.info();
        assert_eq!((i.deaths, i.walked), (1, 0.0));
    }

    #[test]
    fn game_totals_count_what_comes_after_the_baseline() {
        let mut s = Session::default();
        let mut smp = SessionSample {
            mined_total: 40,
            kills_total: 7,
            ..SessionSample::default()
        };
        run(&mut s, 0, 0.0, &smp);
        smp.mined_total = 43;
        smp.kills_total = 8;
        run(&mut s, 1, 0.0, &smp);
        let i = s.info();
        assert_eq!((i.mined, i.kills), (3, 1));
    }

    #[test]
    fn join_resets_only_when_asked() {
        let mut s = Session::default();
        let mut smp = SessionSample::default();
        run(&mut s, 0, 0.0, &smp);
        run(&mut s, 1, 5.0, &smp);
        smp.join = 1;
        s.step(
            2,
            true,
            false,
            &smp,
            [5.0, 64.0, 0.0],
            "minecraft:overworld",
        );
        assert_eq!(s.info().walked, 5.0);
        smp.join = 2;
        run(&mut s, 3, 5.0, &smp);
        assert_eq!(s.info().walked, 0.0);
    }

    #[test]
    fn inactive_counts_nothing() {
        let mut s = Session::default();
        let smp = SessionSample::default();
        s.step(0, false, true, &smp, [0.0; 3], "d");
        s.step(5 * S, false, true, &smp, [9.0, 0.0, 0.0], "d");
        assert_eq!(s.info(), SessionInfo::default());
    }
}
