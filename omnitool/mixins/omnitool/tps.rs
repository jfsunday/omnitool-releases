// Server TPS (docs/spec/omnitool/hud.md, section 8): TpsTracker and the
// ServerTps rules as pure logic, checked against tests/fixtures/tps.json,
// plus the module. The tracker is fed by the `set_time` injection
// (hook_set_time.rs, Game::handle_packet, untested until T1) and shared by
// the module's tick and the Omni Overlay entries `tps` and `lag`. Added to
// the game as `game::omnitool_tps`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_overlay_entries::TpsInfo;
use super::omnitool_setting::{Kind, SettingDef};
use std::cell::{Cell, RefCell};
use std::time::Instant;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "server_tps",
    name: "Server TPS",
    category: Category::Hud,
    description: "Estimates the server TPS from world time updates and warns when the server lags. Shown in the Omni Overlay.",
    default_on: true,
    cheaty: false,
    beta: true,
    fair_variant: false,
    replaces: "",
    settings: &[
        SettingDef::new(
            "threshold",
            "Warn below TPS",
            Kind::Number {
                default: 15.0,
                min: 1.0,
                max: 20.0,
                step: 0.5,
            },
        ),
        SettingDef::new("action_bar", "Warning in the action bar", Kind::Bool(false)),
    ],
    command: None,
    key: super::omnitool_setting::NONE,
    hold: false,
    hud: None,
    tick: Some(tick),
    on_key: None,
    hud_right: None,
    on_action: None,
};

/// Ticks between action-bar repeats while the server lags.
const WARN_EVERY: u32 = 40;

thread_local! {
    /// Java's static `TRACKER`: collects whether or not the module is on.
    static TRACKER: RefCell<TpsTracker> = RefCell::new(TpsTracker::default());
    static START: Instant = Instant::now();
    static WARNED: Cell<(bool, u32)> = const { Cell::new((false, 0)) };
}

/// `System.nanoTime()`: nanoseconds since the first call.
pub fn nano_time() -> i64 {
    START.with(|s| s.elapsed().as_nanos() as i64)
}

/// `ServerTps.onTimePacket`: a world time packet from `source`.
pub fn on_time_packet(source: &str, game_time: i64) {
    TRACKER.with_borrow_mut(|t| t.sample(source, game_time, nano_time()));
}

fn tps_now() -> (f64, f64) {
    let now = nano_time();
    TRACKER.with_borrow(|t| {
        let silent = t.silent_seconds(now);
        (server_tps(t.tps(), silent), silent)
    })
}

fn threshold(host: &dyn Host) -> f64 {
    core::value(host, &MODULE, "threshold").num()
}

/// `ServerTps.warning()`, translated.
pub fn warning_text(host: &dyn Host) -> Option<String> {
    let (tps, silent) = tps_now();
    let w = warning(tps, silent, threshold(host))?;
    let base = if silent > SILENT_SECONDS {
        "Server not responding"
    } else {
        "Server lagging"
    };
    let suffix = &w[base.len()..];
    Some(format!("{}{suffix}", core::tr(host, base)))
}

/// The Omni Overlay's `tps` data; None while the module is off.
pub fn info(host: &dyn Host) -> Option<TpsInfo> {
    if !core::active(host, &MODULE) {
        return None;
    }
    let (tps, _) = tps_now();
    Some(TpsInfo {
        tps,
        lagging: lagging(tps, threshold(host)),
    })
}

/// The Omni Overlay's `lag` data; None while off or not lagging.
pub fn lag_warning(host: &dyn Host) -> Option<String> {
    core::active(host, &MODULE).then(|| warning_text(host))?
}

/// `ServerTps.tick`: forgets a gone connection (`current` = its identity,
/// None = disconnected) and repeats the action-bar warning.
pub fn keep_only(current: Option<&str>) {
    TRACKER.with_borrow_mut(|t| t.keep_only(current));
}

fn tick(host: &mut dyn Host) {
    if !core::active(host, &MODULE) || !core::value(host, &MODULE, "action_bar").on() {
        return;
    }
    let w = warning_text(host);
    let (warned, ticks) = WARNED.get();
    let mut ticks = ticks;
    if let Some(text) = &w {
        let again = if warned {
            ticks += 1;
            ticks >= WARN_EVERY
        } else {
            true
        };
        if again {
            host.action_bar(text);
            ticks = 0;
        }
    }
    WARNED.set((w.is_some(), ticks));
}

/// Samples kept (Java `WINDOW`).
const WINDOW: usize = 6;
/// Seconds without a packet before the server counts as frozen.
pub const SILENT_SECONDS: f64 = 3.0;

/// Java's `TpsTracker`; `source` is the connection's identity.
#[derive(Default)]
pub struct TpsTracker {
    game_time: [i64; WINDOW],
    nanos: [i64; WINDOW],
    count: usize,
    head: usize,
    source: Option<String>,
}

impl TpsTracker {
    pub fn sample(&mut self, source: &str, game_time: i64, nano_time: i64) {
        if self.source.as_deref() != Some(source) {
            self.reset();
        }
        self.source = Some(source.to_owned());
        if self.count > 0 && game_time < self.game_time[(self.head + WINDOW - 1) % WINDOW] {
            self.clear();
        }
        self.game_time[self.head] = game_time;
        self.nanos[self.head] = nano_time;
        self.head = (self.head + 1) % WINDOW;
        self.count = (self.count + 1).min(WINDOW);
    }

    /// Forgets everything when the connection is not `source` any more
    /// (`None` = disconnected).
    pub fn keep_only(&mut self, source: Option<&str>) {
        if self.source.is_some() && self.source.as_deref() != source {
            self.reset();
        }
    }

    pub fn reset(&mut self) {
        self.clear();
        self.source = None;
    }

    fn clear(&mut self) {
        self.count = 0;
        self.head = 0;
    }

    /// TPS over the window, 0..20; -1 = unknown.
    pub fn tps(&self) -> f64 {
        if self.count < 2 {
            return -1.0;
        }
        let newest = (self.head + WINDOW - 1) % WINDOW;
        let oldest = (self.head + WINDOW - self.count) % WINDOW;
        let seconds = (self.nanos[newest] - self.nanos[oldest]) as f64 / 1e9;
        if seconds <= 0.0 {
            return -1.0;
        }
        ((self.game_time[newest] - self.game_time[oldest]) as f64 / seconds).clamp(0.0, 20.0)
    }

    /// Seconds since the last packet; -1 = none yet.
    pub fn silent_seconds(&self, now_nanos: i64) -> f64 {
        if self.count == 0 {
            return -1.0;
        }
        (now_nanos - self.nanos[(self.head + WINDOW - 1) % WINDOW]) as f64 / 1e9
    }
}

/// `ServerTps.tps()`: 0 when the server has been silent too long.
pub fn server_tps(tracker_tps: f64, silent: f64) -> f64 {
    if silent > SILENT_SECONDS {
        0.0
    } else {
        tracker_tps
    }
}

/// `ServerTps.lagging()`.
pub fn lagging(tps: f64, threshold: f64) -> bool {
    tps >= 0.0 && tps < threshold
}

/// `ServerTps.warning()` (English; the caller translates the base text).
pub fn warning(tps: f64, silent: f64, threshold: f64) -> Option<String> {
    if !lagging(tps, threshold) {
        return None;
    }
    Some(if silent > SILENT_SECONDS {
        format!("Server not responding ({} s)", silent as i64)
    } else {
        format!("Server lagging ({tps:.1} TPS)")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixtures() -> Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/tps.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn tracker_matches_java() {
        for case in fixtures()["tps_tracker"].as_array().unwrap() {
            let mut t = TpsTracker::default();
            for (n, op) in case["ops"].as_array().unwrap().iter().enumerate() {
                let name = format!("{} op {n}", case["name"]);
                match op["op"].as_str().unwrap() {
                    "sample" => t.sample(
                        op["source"].as_str().unwrap(),
                        op["game_time"].as_i64().unwrap(),
                        op["nanos"].as_i64().unwrap(),
                    ),
                    "keep_only" => t.keep_only(op["source"].as_str()),
                    "reset" => t.reset(),
                    _ => {
                        if let Some(want) = op.get("tps") {
                            assert!((t.tps() - want.as_f64().unwrap()).abs() < 1e-9, "{name}");
                        }
                        if let Some(want) = op.get("silent") {
                            let at = op["silent_at"].as_i64().unwrap_or(0);
                            let got = t.silent_seconds(at);
                            assert!((got - want.as_f64().unwrap()).abs() < 1e-9, "{name}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn server_tps_matches_java() {
        for r in fixtures()["server_tps"].as_array().unwrap() {
            let got = server_tps(
                r["tracker_tps"].as_f64().unwrap(),
                r["silent"].as_f64().unwrap(),
            );
            assert!((got - r["out_tps"].as_f64().unwrap()).abs() < 1e-9, "{r}");
        }
    }

    #[test]
    fn lag_matches_java() {
        for r in fixtures()["lag"].as_array().unwrap() {
            let (tps, silent, th) = (
                r["tps"].as_f64().unwrap(),
                r["silent"].as_f64().unwrap(),
                r["threshold"].as_f64().unwrap(),
            );
            assert_eq!(lagging(tps, th), r["lagging"].as_bool().unwrap(), "{r}");
            assert_eq!(
                warning(tps, silent, th).as_deref(),
                r["warning"].as_str(),
                "{r}"
            );
        }
    }
}
