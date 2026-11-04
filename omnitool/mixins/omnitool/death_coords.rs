// Death Coordinates (docs/spec/omnitool/waypoints.md, section 2): on death a
// chat line with the coordinates, an entry in deaths.json (last 100) and
// optionally a "Death" waypoint; `/ot deaths` lists the last ones. Chat has
// no click events in Horizon yet, so the coordinates are plain text;
// Omnitool's Chat Log does not exist in Rust yet, so its `death_coords` line
// is left out. Added to the game as `game::omnitool_death_coords`; also a
// module of this crate.

use super::omnitool_config as config;
use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_json::{Json, Obj};
use super::omnitool_safe::Server;
use super::omnitool_setting::{Kind, SettingDef};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "death_coords",
    name: "Death Coordinates",
    category: Category::Misc,
    description: "Shows and saves where you died (copyable coordinates, /ot deaths lists the last deaths).",
    default_on: true,
    cheaty: false,
    beta: true,
    fair_variant: false,
    replaces: "",
    settings: &[SettingDef::new(
        "waypoint",
        "Create waypoint on death",
        Kind::Bool(false),
    )],
    command: None,
    key: super::omnitool_setting::NONE,
    hold: false,
    hud: None,
    tick: Some(tick),
    on_key: None,
    hud_right: None,
    on_action: None,
};

/// How many deaths deaths.json keeps.
pub const KEEP: usize = 100;

/// One saved death; the text fields are `None` when a hand-edited entry
/// lacks them (Java prints `?`).
#[derive(Clone, Debug, PartialEq)]
pub struct Death {
    pub time: Option<String>,
    pub server: Option<String>,
    pub dimension: Option<String>,
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

thread_local! {
    static WAS_DEAD: Cell<bool> = const { Cell::new(false) };
    /// deaths.json's root as loaded; None = not loaded or never to be
    /// overwritten (see `root`).
    static DATA: RefCell<Option<(PathBuf, Option<Obj>)>> = const { RefCell::new(None) };
}

/// `Died at` timestamp `yyyy-MM-dd'T'HH:mm:ss.SSSXXX`. std has no time zones,
/// so the offset is always `+00:00` (Java: local offset).
pub fn timestamp(now_ms: u64) -> String {
    let secs = (now_ms / 1000) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}+00:00",
        rem / 3600,
        rem / 60 % 60,
        rem % 60,
        now_ms % 1000
    )
}

/// `ChatLog.serverKey` as the death entry stores it.
pub fn server_key(server: &Server, world_name: &str) -> String {
    match server {
        Server::Local => format!("singleplayer:{world_name}"),
        Server::Remote(ip) => ip.trim().to_lowercase(),
        Server::Nowhere | Server::Unknown => "unknown".to_owned(),
    }
}

/// The entry for a death (spec 2.4).
pub fn entry(time: &str, server: &str, dim: &str, x: i32, y: i32, z: i32) -> Json {
    let mut o = Obj::new();
    o.insert("time", Json::str(time));
    o.insert("server", Json::str(server));
    o.insert("dimension", Json::str(dim));
    o.insert("x", Json::Int(x.into()));
    o.insert("y", Json::Int(y.into()));
    o.insert("z", Json::Int(z.into()));
    Json::Obj(o)
}

/// Appends `e` and drops the oldest entries beyond `KEEP`.
pub fn push(deaths: &mut Vec<Json>, e: Json) {
    deaths.push(e);
    while deaths.len() > KEEP {
        deaths.remove(0);
    }
}

/// The printable deaths, newest first, at most `n`; entries that are no
/// objects or lack numeric coordinates are skipped (spec 2.6).
pub fn parse_last(deaths: &[Json], n: usize) -> Vec<Death> {
    let text = |o: &Obj, k: &str| match o.get(k) {
        Some(Json::Str(s)) => Some(s.clone()),
        _ => None,
    };
    deaths
        .iter()
        .rev()
        .filter_map(|j| {
            let o = j.as_obj()?;
            let num = |k: &str| match o.get(k) {
                Some(v @ (Json::Int(_) | Json::Num(_))) => v.as_f64().map(|f| f as i64),
                _ => None,
            };
            Some(Death {
                time: text(o, "time"),
                server: text(o, "server"),
                dimension: text(o, "dimension"),
                x: num("x")?,
                y: num("y")?,
                z: num("z")?,
            })
        })
        .take(n)
        .collect()
}

/// `time` shortened to `yyyy-MM-dd HH:mm` when long enough.
pub fn short_time(t: &str) -> String {
    let c: Vec<char> = t.chars().collect();
    if c.len() >= 16 {
        let s = |a: usize, b: usize| c[a..b].iter().collect::<String>();
        format!("{} {}", s(0, 10), s(11, 16))
    } else {
        t.to_owned()
    }
}

/// One line of `/ot deaths`.
pub fn line(d: &Death) -> String {
    format!(
        " §8{} §a{} {} {} §7{}  {}",
        short_time(d.time.as_deref().unwrap_or("?")),
        d.x,
        d.y,
        d.z,
        d.dimension.as_deref().unwrap_or("?"),
        d.server.as_deref().unwrap_or("?")
    )
}

/// The loaded root of deaths.json, or None when the file is unreadable and
/// must not be overwritten ("never overwrite the file").
fn with_root<R>(host: &dyn Host, f: impl FnOnce(Option<&mut Obj>) -> R) -> Option<R> {
    let file = host.data_dir()?.join("deaths.json");
    DATA.with(|d| {
        let mut d = d.borrow_mut();
        if d.as_ref().is_none_or(|(p, _)| *p != file) {
            let root = match config::read(&file) {
                Some(o) => Some(o),
                None if file.exists() => None,
                None => Some(Obj::new()),
            };
            *d = Some((file, root));
        }
        let (_, root) = d.as_mut()?;
        Some(f(root.as_mut()))
    })
}

fn deaths_of(root: &mut Obj) -> &mut Vec<Json> {
    if !matches!(root.get("deaths"), Some(Json::Arr(_))) {
        root.insert("deaths", Json::Arr(Vec::new()));
    }
    match root.get_mut("deaths") {
        Some(Json::Arr(a)) => a,
        _ => unreachable!("just inserted"),
    }
}

fn tick(host: &mut dyn Host) {
    let Some(w) = host.world() else { return };
    let was = WAS_DEAD.with(|d| d.replace(w.dead));
    if !was
        && w.dead
        && core::active(host, &MODULE)
        && let Some(p) = host.player()
    {
        died(
            host,
            p.x.floor() as i32,
            p.y.floor() as i32,
            p.z.floor() as i32,
            &p.dimension,
        );
    }
}

/// Spec 2.4: file, chat, waypoint (the Chat Log line is left out).
fn died(host: &mut dyn Host, x: i32, y: i32, z: i32, dim: &str) {
    let name = host.world().map(|w| w.world_name).unwrap_or_default();
    let e = entry(
        &timestamp(host.now_ms()),
        &server_key(&host.server(), &name),
        dim,
        x,
        y,
        z,
    );
    let Some(file) = host.data_dir().map(|d| d.join("deaths.json")) else {
        announce(host, x, y, z, dim);
        return;
    };
    with_root(host, |root| {
        if let Some(root) = root {
            push(deaths_of(root), e);
            config::write(&file, root);
        }
    });
    announce(host, x, y, z, dim);
    if core::value(host, &MODULE, "waypoint").on() {
        super::omnitool_waypoints::add_death(host, x, y, z, dim);
    }
}

fn announce(host: &mut dyn Host, x: i32, y: i32, z: i32, dim: &str) {
    let you_died = core::tr(host, "You died at");
    host.chat(&format!(
        "§6[Omnitool] §f{you_died} §a{x} {y} {z} §7({dim})"
    ));
}

/// `/ot deaths`: the last `n` deaths, newest first.
pub fn print_last(host: &mut dyn Host, n: usize) {
    let list = with_root(host, |root| match root {
        Some(r) => parse_last(
            match r.get("deaths") {
                Some(Json::Arr(a)) => a,
                _ => &[],
            },
            n,
        ),
        None => Vec::new(),
    })
    .unwrap_or_default();
    if list.is_empty() {
        let t = core::tr(host, "No deaths saved yet");
        host.chat(&format!("§6[Omnitool] {t}"));
        return;
    }
    let head = core::tr(host, "Last deaths:");
    host.chat(&format!("§6[Omnitool] {head}"));
    for d in &list {
        host.chat(&line(d));
    }
}

/// For tests: forget the loaded file and the death state.
#[cfg(test)]
pub fn reset() {
    DATA.with(|d| *d.borrow_mut() = None);
    WAS_DEAD.with(|d| d.set(false));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_conditions::World;
    use crate::omnitool_core::Player;
    use crate::omnitool_core::tests::{Fake, restart};

    fn fixtures() -> serde_json::Value {
        let path = format!(
            "{}/../tests/fixtures/waypoints.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn world(dead: bool) -> World {
        World {
            health: 20.0,
            hunger: 20,
            y: 64.0,
            holding: None,
            inventory: vec![],
            inventory_full: false,
            dimension: "minecraft:overworld".into(),
            server: Server::Local,
            world_name: "W".into(),
            sneaking: false,
            dead,
            day_time: 0,
            screen_open: false,
        }
    }

    fn host(name: &str) -> (Fake, PathBuf) {
        let dir = std::env::temp_dir().join(format!("ot-dc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let h = Fake {
            dir: Some(dir.clone()),
            player: Some(Player {
                x: 1.5,
                y: 2.0,
                z: 3.9,
                yaw: 0.0,
                pitch: 0.0,
                dimension: "minecraft:overworld".into(),
            }),
            world: Some(world(false)),
            now: 1_793_363_696_789,
            ..Fake::default()
        };
        (h, dir)
    }

    #[test]
    fn registered_and_defaults() {
        const { assert!(MODULE.default_on && MODULE.beta) };
        assert!(
            crate::omnitool_registry::MODULES
                .iter()
                .any(|m| m.id == "death_coords")
        );
    }

    #[test]
    fn timestamp_format() {
        assert_eq!(
            timestamp(1_793_363_696_789),
            "2026-10-30T12:34:56.789+00:00"
        );
        assert_eq!(timestamp(0), "1970-01-01T00:00:00.000+00:00");
    }

    #[test]
    fn server_keys() {
        assert_eq!(server_key(&Server::Local, "W"), "singleplayer:W");
        assert_eq!(
            server_key(&Server::Remote(" Mc.X.com ".into()), ""),
            "mc.x.com"
        );
        assert_eq!(server_key(&Server::Unknown, ""), "unknown");
    }

    #[test]
    fn fixture_death_list_print() {
        let f = &fixtures()["death_list_print"];
        let deaths: Vec<Json> = f["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| Json::parse(&e.to_string()).unwrap())
            .collect();
        let got: Vec<String> = parse_last(&deaths, f["n"].as_u64().unwrap() as usize)
            .iter()
            .map(line)
            .collect();
        // the fixture has Java's plain text; ours adds colour codes
        let plain: Vec<String> = got
            .iter()
            .map(|l| {
                let mut out = String::new();
                let mut it = l.chars();
                while let Some(c) = it.next() {
                    if c == '§' {
                        it.next();
                    } else {
                        out.push(c);
                    }
                }
                out
            })
            .collect();
        let want: Vec<&str> = f["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        assert_eq!(plain, want);
    }

    #[test]
    fn fixture_death_keep() {
        let f = &fixtures()["death_keep"];
        let keep = f["keep"].as_u64().unwrap() as usize;
        assert_eq!(keep, KEEP);
        let mut d: Vec<Json> = (0..f["existing"].as_i64().unwrap())
            .map(|i| entry("t", "s", "d", i as i32, 0, 0))
            .collect();
        push(&mut d, entry("t", "s", "d", 999, 0, 0));
        assert_eq!(d.len() as u64, f["after_add"].as_u64().unwrap());
        assert_eq!(d[0].as_obj().unwrap().get("x"), Some(&Json::Int(1)));
        assert_eq!(
            d.last().unwrap().as_obj().unwrap().get("x"),
            Some(&Json::Int(999))
        );
    }

    #[test]
    fn short_time_cases() {
        assert_eq!(
            short_time("2026-10-30T12:34:56.789+01:00"),
            "2026-10-30 12:34"
        );
        assert_eq!(short_time("short"), "short");
    }

    #[test]
    fn death_is_saved_announced_and_waypointed() {
        restart();
        reset();
        super::super::omnitool_waypoints::reset();
        let (mut h, dir) = host("died");
        h.world = Some(world(false));
        tick(&mut h);
        h.world = Some(world(true));
        assert!(core::command(&mut h, "ot set death_coords waypoint true"));
        h.lines.clear();
        tick(&mut h);
        tick(&mut h);
        let text = std::fs::read_to_string(dir.join("deaths.json")).unwrap();
        let j = Json::parse(&text).unwrap();
        let Some(Json::Arr(a)) = j.as_obj().unwrap().get("deaths") else {
            panic!()
        };
        assert_eq!(a.len(), 1);
        let e = a[0].as_obj().unwrap();
        assert_eq!(e.get("server"), Some(&Json::str("singleplayer:W")));
        assert_eq!(e.get("x"), Some(&Json::Int(1)));
        assert_eq!(e.get("z"), Some(&Json::Int(3)));
        assert_eq!(
            h.lines,
            ["§6[Omnitool] §fYou died at §a1 2 3 §7(minecraft:overworld)"]
        );
        assert_eq!(h.clip, None);
        let wp = std::fs::read_to_string(dir.join("waypoints.json")).unwrap();
        assert!(wp.contains("\"Death\""));
        h.lines.clear();
        print_last(&mut h, 10);
        assert_eq!(h.lines.len(), 2);
        assert_eq!(h.lines[0], "§6[Omnitool] Last deaths:");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn empty_list_and_broken_file() {
        restart();
        reset();
        let (mut h, dir) = host("empty");
        print_last(&mut h, 10);
        assert_eq!(h.lines, ["§6[Omnitool] No deaths saved yet"]);
        // a broken file is kept as .broken-*; chat still works and a fresh
        // file is started
        reset();
        std::fs::write(dir.join("deaths.json"), "{broken").unwrap();
        died(&mut h, 1, 2, 3, "minecraft:overworld");
        assert!(h.lines.last().unwrap().contains("You died at"));
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().any(|n| n.starts_with("deaths.json.broken-")));
        assert!(
            std::fs::read_to_string(dir.join("deaths.json"))
                .unwrap()
                .contains("\"x\": 1")
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
