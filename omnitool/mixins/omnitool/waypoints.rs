// Waypoints (docs/spec/omnitool/waypoints.md, section 1): per world and
// dimension, a HUD list of the nearest ones with a direction arrow, an
// automatic "Death" waypoint, and waypoints.json in Java's format. Beams and
// boxes in the world need a Gizmos hook (see `beams`); there is no GUI tab,
// so `/ot waypoint add|list|remove` stands in for it. Added to the game as
// `game::omnitool_waypoints`; also a module of this crate.

use super::omnitool_config as config;
use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_hud::{Canvas, HudBox, HudCorner, HudOp};
use super::omnitool_json::{Json, Obj};
use super::omnitool_safe::Server;
use super::omnitool_setting::{Kind, SettingDef};
use std::cell::RefCell;
use std::path::PathBuf;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "waypoints",
    name: "Waypoints",
    category: Category::World,
    description: "Waypoints with beams and HUD list. Manage them in the Waypoints tab.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "",
    settings: &[
        SettingDef::new("beams", "Beams in the world", Kind::Bool(true)),
        SettingDef::new("death", "Automatic death waypoint", Kind::Bool(true)),
        SettingDef::new(
            "max_hud",
            "Entries in HUD list (0 = off)",
            Kind::Number {
                default: 5.0,
                min: 0.0,
                max: 15.0,
                step: 1.0,
            },
        ),
        SettingDef::new("color", "Default color", Kind::Color(0xFF40FFC0u32 as i32)),
        SettingDef::new(
            "text_color",
            "HUD text color",
            Kind::Color(0xFFFFFFFFu32 as i32),
        ),
        SettingDef::new(
            "quick_key",
            "Quick waypoint key",
            Kind::Key(super::omnitool_setting::NONE),
        ),
        SettingDef::new("labels", "Labels in the world", Kind::Bool(true)),
        SettingDef::new(
            "max_distance",
            "Label max distance (0 = unlimited)",
            Kind::Number {
                default: 0.0,
                min: 0.0,
                max: 10000.0,
                step: 100.0,
            },
        ),
        SettingDef::new("tp_command", "Teleport command", Kind::Text(TP_COMMAND)),
        SettingDef::new(
            "tp_command_dim",
            "Teleport command (other dimension)",
            Kind::Text(TP_COMMAND_DIM),
        ),
        SettingDef::new(
            "tp_cross_dim",
            "Teleport across dimensions",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "tp_partial_y",
            "Teleport 0.5 blocks higher",
            Kind::Bool(true),
        ),
    ],
    command: None,
    key: super::omnitool_setting::NONE,
    hold: false,
    hud: Some(hud),
    tick: Some(tick),
    on_key: Some(on_key),
    hud_right: None,
    on_action: None,
};

pub const TP_COMMAND: &str = "tp @s {x} {y} {z}";
pub const TP_COMMAND_DIM: &str = "execute as @s in {d} run tp @s {x} {y} {z}";

/// The colours Quick Waypoint cycles through (spec 1a.2): the chat colours
/// without black.
pub const QUICK_PALETTE: [u32; 15] = [
    0xFF0000AA, 0xFF00AA00, 0xFF00AAAA, 0xFFAA0000, 0xFFAA00AA, 0xFFFFAA00, 0xFFAAAAAA, 0xFF555555,
    0xFF5555FF, 0xFF55FF55, 0xFF55FFFF, 0xFFFF5555, 0xFFFF55FF, 0xFFFFFF55, 0xFFFFFFFF,
];

/// The colour of the automatic death waypoint (fixed, not the setting).
pub const DEATH_COLOR: i32 = 0xFFFF4040u32 as i32;
pub const DEATH_NAME: &str = "Death";

/// One waypoint. `name`/`dim` are `None` when a hand-written entry lacks
/// them (Java: null fields).
#[derive(Clone, Debug, PartialEq)]
pub struct Waypoint {
    pub name: Option<String>,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub dim: Option<String>,
    pub color: i32,
}

impl Waypoint {
    pub fn new(name: &str, x: i32, y: i32, z: i32, dim: &str, color: i32) -> Waypoint {
        Waypoint {
            name: Some(name.to_owned()),
            x,
            y,
            z,
            dim: Some(dim.to_owned()),
            color,
        }
    }
    pub fn name(&self) -> &str {
        self.name.as_deref().unwrap_or("")
    }
    fn in_dim(&self, dim: &str) -> bool {
        self.dim.as_deref() == Some(dim)
    }
    fn to_json(&self) -> Json {
        let mut o = Obj::new();
        if let Some(n) = &self.name {
            o.insert("name", Json::str(n));
        }
        o.insert("x", Json::Int(self.x.into()));
        o.insert("y", Json::Int(self.y.into()));
        o.insert("z", Json::Int(self.z.into()));
        if let Some(d) = &self.dim {
            o.insert("dim", Json::str(d));
        }
        o.insert("color", Json::Int(self.color.into()));
        Json::Obj(o)
    }
    /// None if the entry has the wrong shape (Java: Gson throws).
    fn from_json(j: &Json) -> Option<Waypoint> {
        let o = j.as_obj()?;
        let text = |k: &str| match o.get(k) {
            None | Some(Json::Null) => Some(None),
            Some(Json::Str(s)) => Some(Some(s.clone())),
            Some(_) => None,
        };
        let int = |k: &str| match o.get(k) {
            None | Some(Json::Null) => Some(0),
            Some(v @ (Json::Int(_) | Json::Num(_))) => v.as_int(),
            Some(_) => None,
        };
        Some(Waypoint {
            name: text("name")?,
            x: int("x")?,
            y: int("y")?,
            z: int("z")?,
            dim: text("dim")?,
            color: int("color")?,
        })
    }
}

// ------------------------------------------------------------ pure logic

/// Java's arrow for a waypoint at block `(wx, wz)` seen by a player at
/// `(px, pz)` looking along `yaw` (spec 1.12).
pub fn arrow(wx: i32, wz: i32, px: f64, pz: f64, yaw: f64) -> &'static str {
    let angle = (f64::from(wz) + 0.5 - pz)
        .atan2(f64::from(wx) + 0.5 - px)
        .to_degrees()
        - 90.0;
    let rel = ((angle - yaw) % 360.0 + 540.0) % 360.0 - 180.0;
    if rel.abs() < 22.5 {
        "^"
    } else if rel.abs() > 157.5 {
        "v"
    } else if rel > 0.0 {
        if rel < 67.5 {
            "/>"
        } else if rel < 112.5 {
            ">"
        } else {
            "\\>"
        }
    } else if rel > -67.5 {
        "<\\"
    } else if rel > -112.5 {
        "<"
    } else {
        "</"
    }
}

/// The HUD list (spec 1.12): the `max` nearest waypoints of `dim` by
/// horizontal distance (stable sort), `<arrow> <name>  <metres>m`.
pub fn hud_lines(
    list: &[Waypoint],
    player: (f64, f64),
    yaw: f64,
    dim: &str,
    max: usize,
    preview: bool,
) -> Vec<String> {
    if max == 0 {
        return Vec::new();
    }
    let dist = |w: &Waypoint| {
        let (dx, dz) = (
            f64::from(w.x) + 0.5 - player.0,
            f64::from(w.z) + 0.5 - player.1,
        );
        (dx * dx + dz * dz).sqrt()
    };
    let mut near: Vec<(&Waypoint, f64)> = list
        .iter()
        .filter(|w| w.in_dim(dim))
        .map(|w| (w, dist(w)))
        .collect();
    near.sort_by(|a, b| a.1.total_cmp(&b.1));
    let lines: Vec<String> = near
        .iter()
        .take(max)
        .map(|(w, d)| {
            format!(
                "{} {}  {}m",
                arrow(w.x, w.z, player.0, player.1, yaw),
                w.name(),
                *d as i64
            )
        })
        .collect();
    if lines.is_empty() && preview {
        return vec!["^ Waypoint  42m".to_owned()];
    }
    lines
}

/// What the world renderer has to draw for one waypoint (spec 1.11): a
/// vertical beam and a unit-cube outline, both in `color`, line width 3.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub struct Beam {
    pub from: [f64; 3],
    pub to: [f64; 3],
    pub cube_min: [f64; 3],
    pub cube_max: [f64; 3],
    pub color: i32,
}

/// The beams of the waypoints in `dim` (nothing if `beams` is off). Nothing
/// calls it until a Gizmos hook exists: in Horizon `Game::debug_gizmos`
/// (game/debug_keys.rs:759, drawn at game.rs:12374) builds a `Gizmos` with
/// `line` and `cuboid` (gizmos.rs:71, 80), which is where these go.
#[allow(dead_code)]
pub fn beams(list: &[Waypoint], dim: &str, enabled: bool) -> Vec<Beam> {
    if !enabled {
        return Vec::new();
    }
    list.iter()
        .filter(|w| w.in_dim(dim))
        .map(|w| {
            let (x, y, z) = (f64::from(w.x), f64::from(w.y), f64::from(w.z));
            Beam {
                from: [x + 0.5, y, z + 0.5],
                to: [x + 0.5, y + 256.0, z + 0.5],
                cube_min: [x, y, z],
                cube_max: [x + 1.0, y + 1.0, z + 1.0],
                color: w.color,
            }
        })
        .collect()
}

/// `Waypoints.worldKey`: `server:<ip>` for a remote server, else
/// `world:<level name>` (`unknown` without one). A server the host can't name
/// counts as `server:unknown`.
pub fn world_key(server: &Server, world_name: &str) -> String {
    match server {
        Server::Remote(ip) => format!("server:{}", ip.to_lowercase()),
        Server::Unknown => "server:unknown".to_owned(),
        Server::Local | Server::Nowhere => format!(
            "world:{}",
            if world_name.is_empty() {
                "unknown"
            } else {
                world_name
            }
        ),
    }
}

/// Java's `Waypoints.dimension()` cleaned up: the id out of
/// `ResourceKey[minecraft:dimension / <id>]`, or the string as is. Java cuts
/// at the last `/` and so turns `mymod:cave/deep` into `deep`; we keep the
/// id (a deliberate deviation, see the fixture's `rust_out`).
#[cfg(test)]
pub fn dimension_id(s: &str) -> String {
    let inner = match s.split_once(" / ") {
        Some((_, id)) => id,
        None => s,
    };
    inner.trim_end_matches(']').trim().to_owned()
}

/// `Waypoint <n>` for an unnamed one, n = all waypoints of the world + 1.
pub fn default_name(existing: usize) -> String {
    format!("Waypoint {}", existing + 1)
}

/// `addDeath`: replaces every waypoint named exactly `Death`.
pub fn with_death(list: &mut Vec<Waypoint>, x: i32, y: i32, z: i32, dim: &str) {
    list.retain(|w| w.name.as_deref() != Some(DEATH_NAME));
    list.push(Waypoint::new(DEATH_NAME, x, y, z, dim, DEATH_COLOR));
}

// ------------------------------------------------------------ Xaero extras

/// Quick Waypoint's colour for the world's `count`-th waypoint.
pub fn quick_color(count: usize) -> i32 {
    QUICK_PALETTE[count % QUICK_PALETTE.len()] as i32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpError {
    OtherDimension,
    NoCommand,
}

fn fill_template(tpl: &str, x: &str, y: &str, z: &str, d: &str) -> String {
    tpl.replace("{x}", x)
        .replace("{y}", y)
        .replace("{z}", z)
        .replace("{d}", d)
}

/// The command (without `/`) that teleports to `w` (spec 1a.3).
pub fn teleport_command(
    tpl: &str,
    tpl_dim: &str,
    cross_dim: bool,
    partial_y: bool,
    w: &Waypoint,
    player_dim: &str,
) -> Result<String, TpError> {
    let other = w.dim.as_deref().is_some_and(|d| d != player_dim);
    let tpl = if other {
        if !cross_dim {
            return Err(TpError::OtherDimension);
        }
        tpl_dim
    } else {
        tpl
    };
    let tpl = tpl.trim().trim_start_matches('/').trim();
    if tpl.is_empty() {
        return Err(TpError::NoCommand);
    }
    let y = if partial_y {
        format!("{:.1}", f64::from(w.y) + 0.5)
    } else {
        w.y.to_string()
    };
    Ok(fill_template(
        tpl,
        &w.x.to_string(),
        &y,
        &w.z.to_string(),
        w.dim.as_deref().unwrap_or(player_dim),
    ))
}

/// Where a world point lands on the screen (spec 1a.4): `eye` and the
/// view's `yaw`/`pitch` (degrees, 0 yaw = south), vertical `fov` in degrees,
/// GUI size. None behind the camera or off the screen.
pub fn project(
    eye: [f64; 3],
    yaw: f64,
    pitch: f64,
    fov: f64,
    size: (f64, f64),
    target: [f64; 3],
) -> Option<(f32, f32)> {
    let (sy, cy) = yaw.to_radians().sin_cos();
    let (sp, cp) = pitch.to_radians().sin_cos();
    let fwd = [-sy * cp, -sp, cy * cp];
    let right = [-cy, 0.0, -sy];
    // up = right x forward
    let up = [
        right[1] * fwd[2] - right[2] * fwd[1],
        right[2] * fwd[0] - right[0] * fwd[2],
        right[0] * fwd[1] - right[1] * fwd[0],
    ];
    let c = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
    let dot = |a: [f64; 3]| a[0] * c[0] + a[1] * c[1] + a[2] * c[2];
    let (cz, cx, cu) = (dot(fwd), dot(right), dot(up));
    if cz <= 0.05 {
        return None;
    }
    let f = 1.0 / (fov.to_radians() / 2.0).tan();
    let (w, h) = size;
    let sx = w / 2.0 * (1.0 + f * cx / (cz * (w / h)));
    let sy = h / 2.0 * (1.0 - f * cu / cz);
    if !(0.0..=w).contains(&sx) || !(0.0..=h).contains(&sy) {
        return None;
    }
    Some((sx as f32, sy as f32))
}

/// `123m`, from 10 km on `12.5km`.
pub fn format_distance(d: f64) -> String {
    if d < 10000.0 {
        format!("{}m", d as i64)
    } else {
        format!("{:.1}km", d / 1000.0)
    }
}

const MAX_LABELS: usize = 64;
const EYE_HEIGHT: f64 = 1.62;

/// What to draw for the waypoints in view, at absolute GUI positions.
pub fn label_ops(
    list: &[Waypoint],
    p: &core::Player,
    view: core::View,
    max_distance: f64,
    canvas_width: impl Fn(&str) -> f32,
) -> Vec<HudOp> {
    let eye = [p.x, p.y + EYE_HEIGHT, p.z];
    let mut seen: Vec<(&Waypoint, f64, (f32, f32))> = list
        .iter()
        .filter(|w| w.in_dim(&p.dimension))
        .filter_map(|w| {
            let t = [
                f64::from(w.x) + 0.5,
                f64::from(w.y) + 0.5,
                f64::from(w.z) + 0.5,
            ];
            let d = ((t[0] - eye[0]).powi(2) + (t[1] - eye[1]).powi(2) + (t[2] - eye[2]).powi(2))
                .sqrt();
            if max_distance > 0.0 && d > max_distance {
                return None;
            }
            let at = project(
                eye,
                f64::from(p.yaw),
                f64::from(p.pitch),
                f64::from(view.fov),
                (f64::from(view.gui_w), f64::from(view.gui_h)),
                t,
            )?;
            Some((w, d, at))
        })
        .collect();
    seen.sort_by(|a, b| a.1.total_cmp(&b.1));
    seen.truncate(MAX_LABELS);
    let mut ops = Vec::new();
    for (w, d, (x, y)) in seen {
        let rgb = w.color as u32 & 0x00FF_FFFF;
        ops.push(HudOp::Fill {
            rect: [x - 2.0, y - 2.0, 5.0, 5.0],
            argb: 0xFF00_0000 | rgb,
        });
        let text = format!("{}  {}", w.name(), format_distance(d));
        let tw = canvas_width(&text);
        ops.push(HudOp::Text {
            text,
            at: [x - tw / 2.0, y - 13.0],
            rgb,
            shadow: true,
        });
    }
    ops
}

/// The labels of the waypoints in view this frame (empty when off).
pub fn world_ops(host: &dyn Host) -> Vec<HudOp> {
    if !core::active(host, &MODULE) || !core::value(host, &MODULE, "labels").on() {
        return Vec::new();
    }
    let (Some(p), Some(view)) = (host.player(), host.view()) else {
        return Vec::new();
    };
    let max = core::value(host, &MODULE, "max_distance").num();
    label_ops(&all(host), &p, view, max, |t| host.text_width(t))
}

fn text_setting(host: &dyn Host, id: &str, default: &str) -> String {
    match core::value(host, &MODULE, id) {
        super::omnitool_setting::Value::Text(t) => t,
        _ => default.to_owned(),
    }
}

/// Quick Waypoint's key (spec 1a.2).
fn on_key(host: &mut dyn Host, key: &str, pressed: bool) {
    let super::omnitool_setting::Value::Key(code) = core::value(host, &MODULE, "quick_key") else {
        return;
    };
    if !pressed
        || code == super::omnitool_setting::NONE
        || core::vanilla_key_name(code).as_deref() != Some(key)
        || !core::active(host, &MODULE)
    {
        return;
    }
    quick(host);
}

/// A waypoint at the player's feet in the next palette colour.
pub fn quick(host: &mut dyn Host) {
    let Some(p) = host.player() else { return };
    let (x, y, z) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
    let name = edit(host, |l| {
        let name = default_name(l.len());
        l.push(Waypoint::new(
            &name,
            x,
            y,
            z,
            &p.dimension,
            quick_color(l.len()),
        ));
        (name, true)
    });
    if let Some(name) = name {
        host.action_bar(&format!("Omnitool: waypoint '{name}' added"));
    }
}

/// `/ot waypoint tp`: teleports with the configured command.
fn teleport(host: &mut dyn Host, what: &str) {
    if host.player().is_none() {
        return;
    }
    let found = edit(host, |l| {
        let at = what
            .parse::<usize>()
            .ok()
            .and_then(|n| n.checked_sub(1))
            .filter(|&i| i < l.len())
            .or_else(|| l.iter().position(|w| w.name() == what));
        (at.map(|i| l[i].clone()), false)
    })
    .flatten();
    let Some(w) = found else {
        return core::fail(host, &format!("No waypoint '{what}'"));
    };
    teleport_to(host, &w);
}

/// Teleports to `w` with the configured command (also the world map's
/// menu); true when the command was sent.
pub fn teleport_to(host: &mut dyn Host, w: &Waypoint) -> bool {
    let Some(p) = host.player() else {
        return false;
    };
    let cmd = teleport_command(
        &text_setting(host, "tp_command", TP_COMMAND),
        &text_setting(host, "tp_command_dim", TP_COMMAND_DIM),
        core::value(host, &MODULE, "tp_cross_dim").on(),
        core::value(host, &MODULE, "tp_partial_y").on(),
        w,
        &p.dimension,
    );
    match cmd {
        Ok(c) => {
            host.send_command(&c);
            core::ok(host, &format!("Teleporting to '{}'", w.name()));
            true
        }
        Err(TpError::OtherDimension) => {
            core::fail(host, "Waypoint is in another dimension");
            false
        }
        Err(TpError::NoCommand) => {
            core::fail(host, "No teleport command set");
            false
        }
    }
}

// ------------------------------------------------------------ the file

/// waypoints.json held in memory (Java loads it lazily on first use).
struct Store {
    file: PathBuf,
    /// World key -> its waypoints, in file order. Worlds of a wrong shape
    /// were dropped on load (a copy of the file is kept).
    worlds: Vec<(String, Vec<Waypoint>)>,
}

thread_local! {
    static STORE: RefCell<Option<Store>> = const { RefCell::new(None) };
    static WAS_DEAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn load(file: PathBuf) -> Store {
    let mut worlds = Vec::new();
    if let Some(root) = config::read(&file) {
        let parsed: Option<Vec<(String, Vec<Waypoint>)>> = root
            .iter()
            .map(|(k, v)| match v {
                Json::Arr(a) => a
                    .iter()
                    .map(Waypoint::from_json)
                    .collect::<Option<Vec<_>>>()
                    .map(|l| (k.to_owned(), l)),
                _ => None,
            })
            .collect();
        match parsed {
            Some(w) => worlds = w,
            None => {
                let kept = config::set_aside(&file, false);
                config::log(&format!("Broken waypoints.json, kept a copy as {kept:?}"));
            }
        }
    }
    Store { file, worlds }
}

impl Store {
    fn list(&mut self, key: &str) -> &mut Vec<Waypoint> {
        let i = match self.worlds.iter().position(|(k, _)| k == key) {
            Some(i) => i,
            None => {
                self.worlds.push((key.to_owned(), Vec::new()));
                self.worlds.len() - 1
            }
        };
        &mut self.worlds[i].1
    }
    fn save(&self) {
        let mut root = Obj::new();
        for (k, l) in &self.worlds {
            root.insert(k, Json::Arr(l.iter().map(Waypoint::to_json).collect()));
        }
        config::write(&self.file, &root);
    }
}

/// Runs `f` on the world's list and saves if `f` says it changed it.
/// None without a data folder.
fn edit<R>(host: &dyn Host, f: impl FnOnce(&mut Vec<Waypoint>) -> (R, bool)) -> Option<R> {
    let file = host.data_dir()?.join("waypoints.json");
    let world = host.world().map(|w| w.world_name).unwrap_or_default();
    let key = world_key(&host.server(), &world);
    STORE.with(|s| {
        let mut s = s.borrow_mut();
        if s.as_ref().is_none_or(|st| st.file != file) {
            *s = Some(load(file));
        }
        let st = s.as_mut()?;
        let (r, changed) = f(st.list(&key));
        if changed {
            st.save();
        }
        Some(r)
    })
}

/// The waypoints of the current world, all dimensions.
pub fn all(host: &dyn Host) -> Vec<Waypoint> {
    edit(host, |l| (l.clone(), false)).unwrap_or_default()
}

/// ClickGui: deletes waypoint `i` of the world's list.
pub fn remove_at(host: &dyn Host, i: usize) {
    edit(host, |l| {
        if i < l.len() {
            l.remove(i);
        }
        ((), true)
    });
}

/// ClickGui: renames waypoint `i` (`wp.changed()`).
pub fn rename(host: &dyn Host, i: usize, name: &str) {
    edit(host, |l| {
        if let Some(w) = l.get_mut(i) {
            w.name = Some(name.to_owned());
        }
        ((), true)
    });
}

/// `Waypoints.addHere`: at the player's feet. Needs no active module.
pub fn add_here(host: &mut dyn Host, name: Option<&str>) {
    let Some(p) = host.player() else { return };
    let (x, y, z) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
    add_at(host, name, x, y, z, &p.dimension);
}

/// A waypoint at a block (the world map's "Add waypoint here").
pub fn add_at(host: &mut dyn Host, name: Option<&str>, x: i32, y: i32, z: i32, dim: &str) {
    let color = match core::value(host, &MODULE, "color") {
        super::omnitool_setting::Value::Color(c) => c,
        _ => 0xFF40FFC0u32 as i32,
    };
    let name = edit(host, |l| {
        let name = name.map_or_else(|| default_name(l.len()), str::to_owned);
        l.push(Waypoint::new(&name, x, y, z, dim, color));
        (name, true)
    });
    if let Some(name) = name {
        host.action_bar(&format!("Omnitool: waypoint '{name}' added"));
    }
}

/// `Waypoints.addDeath` (also used by Death Coordinates).
pub fn add_death(host: &dyn Host, x: i32, y: i32, z: i32, dim: &str) {
    edit(host, |l| {
        with_death(l, x, y, z, dim);
        ((), true)
    });
}

// ------------------------------------------------------------ hooks

fn tick(host: &mut dyn Host) {
    let Some(w) = host.world() else { return };
    let dead = w.dead;
    // Java keeps `wasDead` when the world is left and does not reset it; so
    // do we (it only matters when joining a world already dead).
    let was = WAS_DEAD.with(|d| d.replace(dead));
    if !was
        && dead
        && core::active(host, &MODULE)
        && core::value(host, &MODULE, "death").on()
        && let Some(p) = host.player()
    {
        add_death(
            host,
            p.x.floor() as i32,
            p.y.floor() as i32,
            p.z.floor() as i32,
            &p.dimension,
        );
    }
}

fn hud(host: &mut dyn Host) -> Option<HudBox> {
    let p = host.player()?;
    let max = core::value(host, &MODULE, "max_hud").num().round() as usize;
    let color = match core::value(host, &MODULE, "text_color") {
        super::omnitool_setting::Value::Color(c) => c as u32 & 0x00FF_FFFF,
        _ => 0xFFFFFF,
    };
    let lines = hud_lines(
        &all(host),
        (p.x, p.z),
        f64::from(p.yaw),
        &p.dimension,
        max,
        false,
    );
    if lines.is_empty() {
        return None;
    }
    let lines: Vec<(String, u32)> = lines.into_iter().map(|l| (l, color)).collect();
    Some(Canvas::new(host).lines(HudCorner::TopLeft, &lines, true))
}

// ------------------------------------------------------------ /ot waypoint

/// `/ot waypoint add [name] | list | remove <number|name>`: the native mod's
/// stand-in for the Waypoints tab (Java has no command for it).
pub fn ot(host: &mut dyn Host, args: &str) {
    let args = args.trim();
    let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
    match sub {
        "add" => {
            let name = rest.trim();
            add_here(host, (!name.is_empty()).then_some(name));
        }
        "list" => {
            let list = all(host);
            if list.is_empty() {
                core::ok(host, "No waypoints in this world");
            }
            for (i, w) in list.iter().enumerate() {
                host.chat(&format!(
                    "§7{}. §f{} §7{} {} {}  {}",
                    i + 1,
                    w.name(),
                    w.x,
                    w.y,
                    w.z,
                    w.dim.as_deref().unwrap_or("?")
                ));
            }
        }
        "remove" if !rest.trim().is_empty() => remove(host, rest.trim()),
        "tp" if !rest.trim().is_empty() => teleport(host, rest.trim()),
        _ => core::fail(
            host,
            "Usage: /ot waypoint add [name] | list | remove <number|name> | tp <number|name>",
        ),
    }
}

fn remove(host: &mut dyn Host, what: &str) {
    let removed = edit(host, |l| {
        let at = what
            .parse::<usize>()
            .ok()
            .and_then(|n| n.checked_sub(1))
            .filter(|&i| i < l.len())
            .or_else(|| l.iter().position(|w| w.name() == what));
        match at {
            Some(i) => (Some(l.remove(i)), true),
            None => (None, false),
        }
    })
    .flatten();
    match removed {
        Some(w) => core::ok(host, &format!("Removed waypoint '{}'", w.name())),
        None => core::fail(host, &format!("No waypoint '{what}'")),
    }
}

/// For tests: forget the loaded file and the death state.
#[cfg(test)]
pub fn reset() {
    STORE.with(|s| *s.borrow_mut() = None);
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

    fn wps(v: &serde_json::Value) -> Vec<Waypoint> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|w| {
                Waypoint::new(
                    w["name"].as_str().unwrap(),
                    w["x"].as_i64().unwrap() as i32,
                    w["y"].as_i64().unwrap() as i32,
                    w["z"].as_i64().unwrap() as i32,
                    w["dim"].as_str().unwrap(),
                    w["color"].as_i64().unwrap_or(0) as i32,
                )
            })
            .collect()
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ot-wp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn host(dir: &std::path::Path) -> Fake {
        Fake {
            dir: Some(dir.to_path_buf()),
            player: Some(Player {
                x: 10.7,
                y: 64.0,
                z: -3.2,
                yaw: 0.0,
                pitch: 0.0,
                dimension: "minecraft:overworld".into(),
            }),
            world: Some(world(false)),
            ..Fake::default()
        }
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
            world_name: "My World".into(),
            sneaking: false,
            dead,
            day_time: 0,
            screen_open: false,
        }
    }

    #[test]
    fn registered() {
        assert!(
            crate::omnitool_registry::MODULES
                .iter()
                .any(|m| m.id == "waypoints")
        );
    }

    #[test]
    fn fixture_arrow() {
        for c in fixtures()["arrow"].as_array().unwrap() {
            let i = &c["in"];
            let got = arrow(
                i["wp"][0].as_i64().unwrap() as i32,
                i["wp"][1].as_i64().unwrap() as i32,
                i["player"][0].as_f64().unwrap(),
                i["player"][1].as_f64().unwrap(),
                i["yaw"].as_f64().unwrap(),
            );
            assert_eq!(got, c["out"].as_str().unwrap(), "{c}");
        }
    }

    #[test]
    fn arrow_edges() {
        // wp straight ahead is ~2.9 degrees off the +Z axis; just inside and
        // just outside the 22.5 degree cone (spec 1.12)
        assert_eq!(arrow(0, 10, 0.5, 0.5, 0.0), "^");
        assert_eq!(arrow(0, 10, 0.5, 0.5, -25.0), "/>");
    }

    #[test]
    fn fixture_dimension() {
        for c in fixtures()["dimension"].as_array().unwrap() {
            // Java cuts at the last "/"; we keep the id (`rust_out`).
            let want = c.get("rust_out").unwrap_or(&c["out"]);
            assert_eq!(
                dimension_id(c["in"].as_str().unwrap()),
                want.as_str().unwrap()
            );
        }
    }

    #[test]
    fn fixture_hud_lines() {
        for c in fixtures()["hud_lines"].as_array().unwrap() {
            let i = &c["in"];
            let got = hud_lines(
                &wps(&i["waypoints"]),
                (
                    i["player"][0].as_f64().unwrap(),
                    i["player"][1].as_f64().unwrap(),
                ),
                i["yaw"].as_f64().unwrap(),
                i["dimension"].as_str().unwrap(),
                i["max_hud"].as_u64().unwrap() as usize,
                i["preview"].as_bool().unwrap(),
            );
            let want: Vec<&str> = c["out"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap())
                .collect();
            assert_eq!(got, want, "{c}");
        }
    }

    #[test]
    fn max_hud_zero_is_empty_even_in_preview() {
        assert!(hud_lines(&[], (0.0, 0.0), 0.0, "d", 0, true).is_empty());
    }

    #[test]
    fn fixture_colors() {
        for c in fixtures()["colors"].as_array().unwrap() {
            let argb = u32::from_str_radix(c["argb"].as_str().unwrap(), 16).unwrap();
            assert_eq!(argb as i32 as i64, c["json_int"].as_i64().unwrap());
        }
        assert_eq!(DEATH_COLOR, -49088);
    }

    #[test]
    fn fixture_default_names() {
        for c in fixtures()["default_names"].as_array().unwrap() {
            let given = c["name_arg"].as_str();
            let name = given.map_or_else(
                || default_name(c["existing_count"].as_u64().unwrap() as usize),
                str::to_owned,
            );
            assert_eq!(name, c["name"].as_str().unwrap());
        }
    }

    #[test]
    fn fixture_world_key() {
        for c in fixtures()["world_key"].as_array().unwrap() {
            let server = match (c["server_ip"].as_str(), c["local"].as_bool().unwrap()) {
                (Some(ip), false) => Server::Remote(ip.into()),
                (_, true) => Server::Local,
                (None, false) => Server::Nowhere,
            };
            assert_eq!(
                world_key(&server, c["world"].as_str().unwrap()),
                c["key"].as_str().unwrap()
            );
        }
        assert_eq!(world_key(&Server::Local, ""), "world:unknown");
    }

    #[test]
    fn fixture_death_waypoint() {
        let f = &fixtures()["death_waypoint"];
        let mut list = wps(&f["before"]);
        let d = &f["died_at"];
        with_death(
            &mut list,
            d["x"].as_i64().unwrap() as i32,
            d["y"].as_i64().unwrap() as i32,
            d["z"].as_i64().unwrap() as i32,
            d["dim"].as_str().unwrap(),
        );
        assert_eq!(list, wps(&f["after"]));
    }

    #[test]
    fn beams_are_per_dimension() {
        let l = vec![
            Waypoint::new("a", 1, 2, 3, "minecraft:overworld", -1),
            Waypoint::new("b", 1, 2, 3, "minecraft:the_nether", -1),
        ];
        let b = beams(&l, "minecraft:overworld", true);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].from, [1.5, 2.0, 3.5]);
        assert_eq!(b[0].to, [1.5, 258.0, 3.5]);
        assert_eq!(b[0].cube_max, [2.0, 3.0, 4.0]);
        assert!(beams(&l, "minecraft:overworld", false).is_empty());
    }

    #[test]
    fn file_round_trip_and_command() {
        restart();
        reset();
        let dir = temp("rt");
        let mut h = host(&dir);
        add_here(&mut h, None);
        add_here(&mut h, Some("Home"));
        assert_eq!(
            h.bar,
            [
                "Omnitool: waypoint 'Waypoint 1' added",
                "Omnitool: waypoint 'Home' added"
            ]
        );
        let text = std::fs::read_to_string(dir.join("waypoints.json")).unwrap();
        let json = Json::parse(&text).unwrap();
        let list = json.as_obj().unwrap().get("world:My World").unwrap();
        let Json::Arr(a) = list else { panic!() };
        assert_eq!(a.len(), 2);
        let w = a[1].as_obj().unwrap();
        assert_eq!(w.get("name"), Some(&Json::str("Home")));
        assert_eq!(w.get("x"), Some(&Json::Int(10)));
        assert_eq!(w.get("z"), Some(&Json::Int(-4)));
        assert_eq!(w.get("color"), Some(&Json::Int(-12517440)));
        // a fresh start reads it back
        reset();
        assert_eq!(all(&h).len(), 2);
        ot(&mut h, "remove Home");
        ot(&mut h, "remove 9");
        assert_eq!(all(&h).len(), 1);
        assert!(h.lines.last().unwrap().contains("No waypoint '9'"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn broken_shape_is_kept_aside_and_not_fatal() {
        restart();
        reset();
        let dir = temp("broken");
        std::fs::write(dir.join("waypoints.json"), r#"{"world:My World": 5}"#).unwrap();
        let h = host(&dir);
        assert!(all(&h).is_empty());
        let kept = std::fs::read_dir(&dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains(".broken-")
            })
            .count();
        assert_eq!(kept, 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_dim_is_skipped_not_fatal() {
        restart();
        reset();
        let dir = temp("nodim");
        std::fs::write(
            dir.join("waypoints.json"),
            r#"{"world:My World": [{"name": "x", "x": 1, "y": 2, "z": 3, "color": -1}]}"#,
        )
        .unwrap();
        let mut h = host(&dir);
        let l = all(&h);
        assert_eq!(l[0].dim, None);
        assert!(hud_lines(&l, (0.0, 0.0), 0.0, "minecraft:overworld", 5, false).is_empty());
        ot(&mut h, "list");
        assert!(h.lines.last().unwrap().ends_with("  ?"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_worlds_survive_a_save() {
        restart();
        reset();
        let dir = temp("keep");
        std::fs::write(
            dir.join("waypoints.json"),
            r#"{"server:a": [{"name": "A", "x": 1, "y": 2, "z": 3, "dim": "d", "color": -1}]}"#,
        )
        .unwrap();
        let mut h = host(&dir);
        add_here(&mut h, Some("n"));
        let text = std::fs::read_to_string(dir.join("waypoints.json")).unwrap();
        assert!(text.contains("server:a") && text.contains("world:My World"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn death_transition_adds_one_waypoint() {
        restart();
        reset();
        let dir = temp("death");
        let mut h = host(&dir);
        core::set_enabled(&mut h, &MODULE, true);
        tick(&mut h);
        assert!(all(&h).is_empty());
        h.world = Some(world(true));
        tick(&mut h);
        tick(&mut h); // still dead: no second entry
        let l = all(&h);
        assert_eq!(l.len(), 1);
        assert_eq!(
            (l[0].name(), l[0].x, l[0].z, l[0].color),
            ("Death", 10, -4, DEATH_COLOR)
        );
        // dies again after respawn: replaced, not added
        h.world = Some(world(false));
        tick(&mut h);
        h.player.as_mut().unwrap().x = 100.0;
        h.world = Some(world(true));
        tick(&mut h);
        let l = all(&h);
        assert_eq!((l.len(), l[0].x), (1, 100));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn death_needs_active_module_and_setting() {
        restart();
        reset();
        let dir = temp("death-off");
        let mut h = host(&dir);
        h.world = Some(world(true));
        tick(&mut h); // module off
        assert!(all(&h).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn hud_box_uses_text_colour() {
        restart();
        reset();
        let dir = temp("hud");
        let mut h = host(&dir);
        core::set_enabled(&mut h, &MODULE, true);
        add_here(&mut h, Some("Here"));
        let b = hud(&mut h).expect("a box");
        assert!(b.h > 10.0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn fixture_projection() {
        for c in fixtures()["projection"].as_array().unwrap() {
            let i = &c["in"];
            let v = |k: &str| i[k].as_f64().unwrap();
            let a = |k: &str| {
                let a = i[k].as_array().unwrap();
                [
                    a[0].as_f64().unwrap(),
                    a[1].as_f64().unwrap(),
                    a[2].as_f64().unwrap(),
                ]
            };
            let got = project(
                a("eye"),
                v("yaw"),
                v("pitch"),
                v("fov"),
                (v("w"), v("h")),
                a("target"),
            );
            match c["out"].as_array() {
                None => assert_eq!(got, None, "{c}"),
                Some(o) => {
                    let (x, y) = got.unwrap_or_else(|| panic!("{c}"));
                    assert!((f64::from(x) - o[0].as_f64().unwrap()).abs() < 0.01, "{c}");
                    assert!((f64::from(y) - o[1].as_f64().unwrap()).abs() < 0.01, "{c}");
                }
            }
        }
    }

    #[test]
    fn fixture_teleport() {
        for c in fixtures()["teleport"].as_array().unwrap() {
            let i = &c["in"];
            let w = Waypoint::new(
                "w",
                i["wp"][0].as_i64().unwrap() as i32,
                i["wp"][1].as_i64().unwrap() as i32,
                i["wp"][2].as_i64().unwrap() as i32,
                i["wp_dim"].as_str().unwrap(),
                0,
            );
            let got = teleport_command(
                i["tpl"].as_str().unwrap(),
                i["tpl_dim"].as_str().unwrap(),
                i["cross_dim"].as_bool().unwrap(),
                i["partial_y"].as_bool().unwrap(),
                &w,
                i["player_dim"].as_str().unwrap(),
            );
            match c["out"].as_str() {
                Some(s) => assert_eq!(got, Ok(s.to_owned()), "{c}"),
                None => assert_eq!(
                    got,
                    Err(if c["error"] == "dimension" {
                        TpError::OtherDimension
                    } else {
                        TpError::NoCommand
                    }),
                    "{c}"
                ),
            }
        }
    }

    #[test]
    fn fixture_quick_colors() {
        for c in fixtures()["quick"].as_array().unwrap() {
            let want = u32::from_str_radix(c["argb"].as_str().unwrap(), 16).unwrap();
            assert_eq!(
                quick_color(c["count"].as_u64().unwrap() as usize) as u32,
                want
            );
        }
    }

    #[test]
    fn distances() {
        assert_eq!(format_distance(123.9), "123m");
        assert_eq!(format_distance(12500.0), "12.5km");
    }

    fn view() -> core::View {
        core::View {
            gui_w: 400.0,
            gui_h: 300.0,
            fov: 70.0,
        }
    }

    #[test]
    fn labels_for_waypoints_in_view() {
        restart();
        reset();
        let dir = temp("labels");
        let mut h = host(&dir);
        h.view = Some(view());
        core::set_enabled(&mut h, &MODULE, true);
        // player at (10.7, 64, -3.2) looking south: a waypoint straight ahead
        add_here(&mut h, Some("Ahead"));
        edit(&h, |l| {
            l[0].x = 10;
            l[0].z = 46;
            ((), true)
        });
        // one behind and one in the Nether never show
        edit(&h, |l| {
            l.push(Waypoint::new(
                "Behind",
                10,
                64,
                -60,
                "minecraft:overworld",
                -1,
            ));
            l.push(Waypoint::new(
                "Nether",
                10,
                64,
                40,
                "minecraft:the_nether",
                -1,
            ));
            ((), true)
        });
        let ops = world_ops(&h);
        assert_eq!(ops.len(), 2, "{ops:?}");
        let HudOp::Text { text, .. } = &ops[1] else {
            panic!()
        };
        assert_eq!(text, "Ahead  49m");
        // too far for the limit
        assert!(label_ops(&all(&h), &h.player.clone().unwrap(), view(), 10.0, |_| 0.0).is_empty());
        // off by setting
        h.view = None;
        assert!(world_ops(&h).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn quick_key_adds_a_coloured_waypoint() {
        restart();
        reset();
        let dir = temp("quick");
        let mut h = host(&dir);
        core::set_enabled(&mut h, &MODULE, true);
        // unbound: nothing
        on_key(&mut h, "key.keyboard.b", true);
        assert!(all(&h).is_empty());
        core::with(&mut h, |st, _| st.set_setting(&MODULE, "quick_key", "66")).unwrap();
        on_key(&mut h, "key.keyboard.b", false);
        on_key(&mut h, "key.keyboard.x", true);
        assert!(all(&h).is_empty());
        on_key(&mut h, "key.keyboard.b", true);
        on_key(&mut h, "key.keyboard.b", true);
        let l = all(&h);
        assert_eq!(l.len(), 2);
        assert_eq!((l[0].name(), l[1].name()), ("Waypoint 1", "Waypoint 2"));
        assert_eq!((l[0].color, l[1].color), (quick_color(0), quick_color(1)));
        assert_eq!(h.bar.len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tp_command_sends_without_slash() {
        restart();
        reset();
        let dir = temp("tp");
        let mut h = host(&dir);
        add_here(&mut h, Some("Home"));
        edit(&h, |l| {
            l.push(Waypoint::new("Far", 5, 70, 6, "minecraft:the_nether", -1));
            ((), true)
        });
        ot(&mut h, "tp Home");
        ot(&mut h, "tp 2");
        ot(&mut h, "tp nope");
        assert_eq!(
            h.sent,
            [
                "/tp @s 10 64.5 -4",
                "/execute as @s in minecraft:the_nether run tp @s 5 70.5 6"
            ]
        );
        assert!(h.lines.last().unwrap().contains("No waypoint 'nope'"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
