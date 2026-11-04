// Omni Overlay (docs/spec/omnitool/omni-overlay.md): F3-like text columns
// from the layout's entries, the overlay key / F6 / F7 / layout key, speed,
// remembered beds, the rest statistic and the visibility of other modules'
// HUD elements. Deviations: Horizon places boxes by corner (left column top
// left, right column top right) instead of free x/y, and there is no layout
// editor screen yet (F6 does nothing). Added to the game as
// `game::omnitool_omni_overlay`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_hud::{HudBox, HudCorner, HudOp, LINE};
use super::omnitool_json::Json;
use super::omnitool_overlay_entries::{self as entries, Ctx, OverlayInfo};
use super::omnitool_overlay_layouts::{Bed, Layout, Layouts, Visibility};
use super::omnitool_safe::Server;
use super::omnitool_setting::{Kind, SettingDef};
use std::cell::RefCell;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

const KEY_F4: i32 = 293;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "omni_overlay",
    name: "Omni Overlay",
    category: Category::Hud,
    description: "F3-like overlay on every version. Overlay key toggles it, overlay key + F6 configures the entries.",
    default_on: true,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "",
    settings: &[
        SettingDef::new(
            "overlay_key",
            "Overlay key (+F6 = settings, +F7 = next layout)",
            Kind::Key(KEY_F4),
        ),
        SettingDef::new(
            "layout_key",
            "Next overlay layout key",
            Kind::Key(super::omnitool_setting::NONE),
        ),
        SettingDef::new("background", "Background", Kind::Bool(true)),
        SettingDef::new("hide_f3", "Hide while vanilla F3 is open", Kind::Bool(true)),
        SettingDef::new("color", "Text color", Kind::Color(0xFFFF_FFFFu32 as i32)),
        // Hidden in Java's settings UI; saved like the others.
        SettingDef::new("layout", "Overlay layout", Kind::Text("Default")),
    ],
    command: None,
    key: super::omnitool_setting::NONE,
    hold: false,
    hud: Some(hud_left),
    tick: Some(tick),
    on_key: Some(key_event),
    hud_right: Some(hud_right),
    on_action: None,
};

/// `STATS_INTERVAL`: ticks between statistic requests.
const STATS_INTERVAL: i64 = 1200;

struct State {
    open: bool,
    key_down: bool,
    combo_used: bool,
    ticks: i64,
    last_xz: Option<(f64, f64)>,
    speed: f64,
    in_world: bool,
    rest_value: i64,
    rest_at: i64,
    rest_wanted: bool,
    last_request: i64,
    layouts: Option<Layouts>,
    failures: HashMap<String, String>,
}

impl Default for State {
    fn default() -> State {
        State {
            open: false,
            key_down: false,
            combo_used: false,
            ticks: 0,
            last_xz: None,
            speed: 0.0,
            in_world: false,
            rest_value: -1,
            rest_at: 0,
            rest_wanted: false,
            last_request: i64::MIN / 2,
            layouts: None,
            failures: HashMap::new(),
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// `restTicks()`: -1 = unknown, else the estimate that keeps ticking between
/// answers. Asks for the statistic.
pub fn rest_ticks() -> i64 {
    state(|s| {
        s.rest_wanted = true;
        rest_estimate(s.rest_value, s.ticks, s.rest_at)
    })
}

pub fn rest_estimate(rest_value: i64, ticks: i64, rest_at: i64) -> i64 {
    if rest_value < 0 {
        -1
    } else {
        rest_value + (ticks - rest_at)
    }
}

// ------------------------------------------------------------ layouts

fn overlay_file(host: &dyn Host) -> Option<std::path::PathBuf> {
    host.data_dir().map(|d| d.join("overlay.json"))
}

/// Runs `f` on the loaded layouts (loaded on first use).
pub fn with_layouts<R>(host: &dyn Host, f: impl FnOnce(&mut Layouts) -> R) -> R {
    let beta = core::read(host, |s| s.beta);
    let file = overlay_file(host);
    state(|s| {
        let l = s
            .layouts
            .get_or_insert_with(|| Layouts::load(file.as_deref(), beta));
        f(l)
    })
}

fn layout_name(host: &dyn Host) -> String {
    core::value(host, &MODULE, "layout").text().to_owned()
}

/// Java's `Layouts.save()`.
pub fn save_layouts(host: &dyn Host) {
    let file = overlay_file(host);
    with_layouts(host, |l| l.save(file.as_deref()));
}

/// Java's `nextLayout`: selects the next layout, saves the config and tells
/// the player.
pub fn next_layout(host: &mut dyn Host) {
    let cur = layout_name(host);
    let next = with_layouts(host, |l| l.next(&cur));
    core::with(host, |s, _| {
        let _ = s.set_setting(&MODULE, "layout", &next);
        s.save();
    });
    let text = format!("{}: {next}", core::tr(host, "Overlay layout"));
    host.action_bar(&text);
}

fn open() -> bool {
    state(|s| s.open)
}

/// Java's `showsHud`: whether another module's HUD element draws now.
pub fn shows_hud(host: &dyn Host, element: &str) -> bool {
    let name = layout_name(host);
    let open = core::active(host, &MODULE) && open();
    with_layouts(host, |l| {
        l.current(&name).is_none_or(|l| l.hud(element).shows(open))
    })
}

// ------------------------------------------------------------ keys

/// A key event (vanilla key name): the overlay key's tap/combo logic, F6,
/// F7 and the layout key. Releases always arrive; presses only while playing.
pub fn key_event(host: &mut dyn Host, key: &str, pressed: bool) {
    if !core::active(host, &MODULE) {
        return;
    }
    let named = |id: &str| {
        let code = core::value(host, &MODULE, id).key();
        code != super::omnitool_setting::NONE
            && core::vanilla_key_name(code).as_deref() == Some(key)
    };
    let (overlay, layout) = (named("overlay_key"), named("layout_key"));
    let in_game = !host.screen_open();
    if overlay {
        if pressed && in_game {
            state(|s| {
                s.key_down = true;
                s.combo_used = false;
            });
        } else if !pressed {
            state(|s| {
                if s.key_down && !s.combo_used {
                    s.open = !s.open;
                }
                s.key_down = false;
            });
        }
    }
    if pressed && in_game {
        let down = state(|s| s.key_down);
        if down && key == "key.keyboard.f6" {
            // The layout editor screen is not ported yet.
            state(|s| s.combo_used = true);
        } else if down && key == "key.keyboard.f7" {
            state(|s| s.combo_used = true);
            next_layout(host);
        } else if layout {
            next_layout(host);
        }
    }
}

// ------------------------------------------------------------ tick

fn tick(host: &mut dyn Host) {
    let Some(p) = host.player() else {
        state(|s| s.in_world = false);
        return;
    };
    state(|s| {
        if !s.in_world {
            s.in_world = true;
            s.rest_value = -1;
            s.last_request = i64::MIN / 2;
        }
        s.ticks += 1;
        let (lx, lz) = s.last_xz.unwrap_or((0.0, 0.0));
        s.speed = (p.x - lx).hypot(p.z - lz) * 20.0;
        s.last_xz = Some((p.x, p.z));
    });
    if !core::active(host, &MODULE) {
        return;
    }
    let info = host.overlay_info();
    if let Some(pos) = info.sleeping_pos {
        remember_bed(host, pos, &p.dimension);
    }
    rest_stat(host);
}

/// The key of a world's beds (`server:<ip>` / `world:<name>`).
fn world_key(host: &dyn Host) -> String {
    match host.server() {
        Server::Remote(ip) => format!("server:{}", ip.to_lowercase()),
        _ => format!(
            "world:{}",
            host.world().map(|w| w.world_name).unwrap_or_default()
        ),
    }
}

fn remember_bed(host: &mut dyn Host, pos: [i32; 3], dimension: &str) {
    let bed = Bed {
        x: pos[0],
        y: pos[1],
        z: pos[2],
        dim: entries::dim_short(dimension),
    };
    let key = world_key(host);
    let changed = with_layouts(host, |l| {
        l.beds.get(&key) != Some(&bed) && {
            l.beds.insert(key, bed);
            true
        }
    });
    if changed {
        save_layouts(host);
    }
}

/// Java's `restStat`.
fn rest_stat(host: &mut dyn Host) {
    let v = host.time_since_rest();
    let request = state(|s| {
        let answered = s.ticks - s.last_request >= 40;
        if let Some(v) = v
            && answered
            && s.last_request > 0
            && (s.rest_value < 0 || v != s.rest_value)
        {
            s.rest_value = v;
            s.rest_at = s.ticks;
        }
        let ask = s.rest_wanted && s.ticks - s.last_request >= STATS_INTERVAL;
        if ask {
            s.last_request = s.ticks;
        }
        s.rest_wanted = false;
        ask
    });
    if request {
        host.request_stats();
    }
}

// ------------------------------------------------------------ rendering

/// Geometry of one column (spec 5.3 steps 4-5), relative to `x`.
#[derive(Debug, PartialEq)]
pub struct Geometry {
    pub x: f32,
    pub width: f32,
    pub height: f32,
    /// Per line: background rect `[x1, y1, x2, y2]` and text position.
    pub rows: Vec<([f32; 4], [f32; 2])>,
}

pub fn geometry(
    widths: &[f32],
    right: bool,
    x: f32,
    screen_w: f32,
    old_width: f32,
) -> Option<Geometry> {
    if widths.is_empty() {
        return None;
    }
    let x = x.clamp(0.0, (screen_w - old_width).max(0.0));
    let anchored = right && x + old_width >= screen_w - 3.0;
    let w = widths.iter().copied().fold(0.0, f32::max);
    let lh = LINE + 1.0;
    let width = w + 4.0;
    let height = widths.len() as f32 * lh + 1.0;
    let x = if anchored { screen_w - width - 2.0 } else { x };
    let rows = widths
        .iter()
        .enumerate()
        .map(|(i, sw)| {
            let ty = i as f32 * lh;
            let lx = if right { x + width - sw - 4.0 } else { x };
            ([lx, ty, lx + sw + 4.0, ty + lh], [lx + 2.0, ty + 1.0])
        })
        .collect();
    Some(Geometry {
        x,
        width,
        height,
        rows,
    })
}

/// Java's `columnLines`: the lines of one column.
fn column_lines(host: &mut dyn Host, right: bool, preview: bool) -> Vec<String> {
    let Some(player) = host.player() else {
        return Vec::new();
    };
    let info: OverlayInfo = host.overlay_info();
    let name = layout_name(host);
    let german = core::read(host, |s| s.german(host));
    let beta = core::read(host, |s| s.beta);
    let open = open();
    let (speed, rest) = (state(|s| s.speed), rest_ticks());
    let key = world_key(host);
    let bed = with_layouts(host, |l| l.beds.get(&key).cloned());
    let layout: Layout = with_layouts(host, |l| l.current(&name).cloned()).unwrap_or_default();
    let ctx = Ctx {
        info: &info,
        player: &player,
        german,
        preview,
        speed,
        rest,
        bed: bed.as_ref(),
    };
    let mut lines = Vec::new();
    for id in &layout.order {
        if layout.right.contains(id) != right {
            continue;
        }
        let v = layout.vis(id);
        if v == super::omnitool_overlay_layouts::Visibility::Never || (!preview && !v.shows(open)) {
            continue;
        }
        let Some(entry) = super::omnitool_overlay_layouts::entry(id, beta) else {
            continue;
        };
        if state(|s| s.failures.contains_key(id)) {
            continue;
        }
        let text = match catch_unwind(AssertUnwindSafe(|| entries::text(id, &ctx))) {
            Ok(t) => t,
            Err(_) => {
                state(|s| s.failures.insert(id.clone(), "panic".into()));
                core::log_error(&format!(
                    "[Omnitool] Overlay entry {id} failed and is hidden for this session"
                ));
                None
            }
        };
        match text.filter(|t| !t.is_empty()) {
            Some(t) => lines.extend(t.split('\n').map(str::to_owned)),
            None if preview => lines.push(format!("[{}]", ctx.tr(entry.name))),
            None => {}
        }
    }
    // Java's split drops trailing empty parts only.
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

fn column(host: &mut dyn Host, right: bool) -> Option<HudBox> {
    let hide = core::value(host, &MODULE, "hide_f3").on();
    if hide && host.overlay_info().vanilla_debug {
        return None;
    }
    let lines = column_lines(host, right, false);
    let widths: Vec<f32> = lines.iter().map(|l| host.text_width(l)).collect();
    let g = geometry(&widths, right, 0.0, 100_000.0, 0.0)?;
    let background = core::value(host, &MODULE, "background").on();
    let rgb = core::value(host, &MODULE, "color");
    let rgb = match rgb {
        super::omnitool_setting::Value::Color(c) => c as u32 & 0x00FF_FFFF,
        _ => 0xFF_FFFF,
    };
    let mut ops = Vec::new();
    for (line, (bg, at)) in lines.iter().zip(&g.rows) {
        if background {
            ops.push(HudOp::Fill {
                rect: [bg[0], bg[1], bg[2] - bg[0], bg[3] - bg[1]],
                argb: 0x9050_5050,
            });
        }
        ops.push(HudOp::Text {
            text: line.clone(),
            at: *at,
            rgb,
            shadow: !background,
        });
    }
    Some(HudBox {
        corner: if right {
            HudCorner::TopRight
        } else {
            HudCorner::TopLeft
        },
        w: g.width,
        h: g.height,
        ops,
    })
}

fn hud_left(host: &mut dyn Host) -> Option<HudBox> {
    column(host, false)
}

/// The right column, drawn by `core::hud_frame` after the left one.
pub fn hud_right(host: &mut dyn Host) -> Option<HudBox> {
    column(host, true)
}

// ------------------------------------------------------------ /ot layout

const LAYOUT_USAGE: &str = "Usage: /ot layout list | use <name> | new <name> | rename <name> | delete | reset | entries | vis <entry> [always|overlay|never] | move <entry> up|down | side <entry> | json | import <json>";

fn word(s: &str) -> (&str, &str) {
    let s = s.trim();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim()),
        None => (s, ""),
    }
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    if s.len() > 1 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Selects layout `name` (the `layout` setting) and saves the config.
fn select_layout(host: &mut dyn Host, name: &str) {
    core::with(host, |s, _| {
        let _ = s.set_setting(&MODULE, "layout", name);
        s.save();
    });
}

/// `/ot layout ...`: the Omni Overlay layout editor as commands (the layout
/// screen of Java has no Horizon counterpart yet). Works on the selected
/// layout.
pub fn ot_layout(host: &mut dyn Host, args: &str) {
    let (sub, rest) = word(args);
    let name = layout_name(host);
    let beta = core::read(host, |s| s.beta);
    match sub {
        "" => core::fail(host, LAYOUT_USAGE),
        "list" => {
            let (names, cur) = with_layouts(host, |l| {
                (
                    l.names()
                        .iter()
                        .map(|n| (*n).to_owned())
                        .collect::<Vec<_>>(),
                    l.current_name(&name),
                )
            });
            let shown: Vec<String> = names
                .iter()
                .map(|n| {
                    if *n == cur {
                        format!("§a{n}§r")
                    } else {
                        n.clone()
                    }
                })
                .collect();
            core::ok(host, &format!("Layouts: {}", shown.join(", ")));
        }
        "use" => {
            let name = unquote(rest);
            let known = with_layouts(host, |l| l.names().contains(&name));
            if known {
                select_layout(host, name);
                core::ok(host, &format!("Overlay layout: {name}"));
            } else {
                core::fail(host, &format!("Unknown layout: {name}"));
            }
        }
        "new" => {
            let made = with_layouts(host, |l| l.create(&name, unquote(rest)));
            match made {
                Some(n) => {
                    save_layouts(host);
                    select_layout(host, &n);
                    core::ok(host, &format!("Layout {n} created"));
                }
                None => core::fail(host, "Enter a name that is not used yet"),
            }
        }
        "rename" => {
            let renamed = with_layouts(host, |l| l.rename(&name, unquote(rest)));
            match renamed {
                Some(n) => {
                    save_layouts(host);
                    select_layout(host, &n);
                    core::ok(host, &format!("Layout renamed to {n}"));
                }
                None => core::fail(host, "Enter a name that is not used yet"),
            }
        }
        "delete" => {
            let next = with_layouts(host, |l| l.delete(&name));
            match next {
                Some(n) => {
                    save_layouts(host);
                    select_layout(host, &n);
                    core::ok(host, &format!("Layout deleted, now {n}"));
                }
                None => core::fail(host, "The last layout can't be deleted"),
            }
        }
        "reset" => {
            with_layouts(host, |l| l.reset(&name));
            save_layouts(host);
            core::ok(host, "Layout reset to its preset");
        }
        "entries" => {
            let lines: Vec<String> = with_layouts(host, |l| {
                let Some(layout) = l.current(&name) else {
                    return Vec::new();
                };
                layout
                    .order
                    .iter()
                    .filter_map(|id| {
                        let info = super::omnitool_overlay_layouts::entry(id, beta)?;
                        Some(format!(
                            "§7 {id} ({}, {}): {}, {}",
                            info.name,
                            info.group,
                            layout.vis(id).label(),
                            if layout.right.contains(id) {
                                "right"
                            } else {
                                "left"
                            }
                        ))
                    })
                    .collect()
            });
            for line in lines {
                host.chat(&line);
            }
        }
        "vis" | "move" | "side" => {
            let (id, arg) = word(rest);
            let known = with_layouts(host, |l| {
                l.current(&name)
                    .is_some_and(|l| l.order.iter().any(|x| x == id))
            });
            if !known {
                return core::fail(host, &format!("Unknown entry: {id}"));
            }
            let result: Result<String, String> = with_layouts(host, |l| {
                let layout = l.current_mut(&name).ok_or("No layout")?;
                match sub {
                    "vis" => {
                        let v = if arg.is_empty() {
                            layout.vis(id).next(1)
                        } else {
                            match arg.to_ascii_lowercase().as_str() {
                                "always" => Visibility::Always,
                                "overlay" => Visibility::Overlay,
                                "never" => Visibility::Never,
                                _ => {
                                    return Err(
                                        "Usage: /ot layout vis <entry> [always|overlay|never]"
                                            .into(),
                                    );
                                }
                            }
                        };
                        layout.vis.insert(id.to_owned(), v);
                        Ok(format!("{id}: {}", v.label()))
                    }
                    "move" => {
                        let dir = match arg {
                            "up" => -1,
                            "down" => 1,
                            _ => return Err("Usage: /ot layout move <entry> up|down".into()),
                        };
                        layout.move_entry(id, dir);
                        Ok(format!("{id} moved {arg}"))
                    }
                    _ => {
                        layout.switch_column(id);
                        let col = if layout.right.contains(id) {
                            "right"
                        } else {
                            "left"
                        };
                        Ok(format!("{id}: {col} column"))
                    }
                }
            });
            match result {
                Ok(msg) => {
                    save_layouts(host);
                    core::ok(host, &msg);
                }
                Err(e) => core::fail(host, &e),
            }
        }
        "json" => match layout_json(host) {
            Some(j) => core::ok(host, &j.compact()),
            None => core::fail(host, "No layout"),
        },
        "import" => {
            let parsed = Json::parse(rest).ok().and_then(|j| j.as_obj().cloned());
            let Some(obj) = parsed else {
                return core::fail(
                    host,
                    "Usage: /ot layout import {\"<name>\": <layout json>, ...}",
                );
            };
            match with_layouts(host, |l| l.import(&obj)) {
                Ok(()) => {
                    save_layouts(host);
                    core::ok(host, "Layouts imported");
                }
                Err(e) => core::fail(host, &format!("Invalid layout: {e}")),
            }
        }
        _ => core::fail(host, LAYOUT_USAGE),
    }
}

/// The current layout as Json (`/ot layout json`).
pub fn layout_json(host: &dyn Host) -> Option<Json> {
    let name = layout_name(host);
    with_layouts(host, |l| l.current(&name).map(Layout::to_json))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::Player;
    use crate::omnitool_core::tests::{Fake, restart};
    use serde_json::Value;

    fn fixtures() -> Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/omni-overlay.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn reset() {
        restart();
        STATE.with(|s| *s.borrow_mut() = State::default());
    }

    fn player() -> Player {
        Player {
            x: 0.0,
            y: 64.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            dimension: "minecraft:overworld".into(),
        }
    }

    #[test]
    fn geometry_fixture() {
        let f = &fixtures()["column_geometry"];
        let cw = f["char_width"].as_f64().unwrap() as f32;
        for c in f["cases"].as_array().unwrap() {
            let i = &c["in"];
            let widths: Vec<f32> = i["lines"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l.as_str().unwrap().chars().count() as f32 * cw)
                .collect();
            let g = geometry(
                &widths,
                i["right"].as_bool().unwrap(),
                i["x"].as_f64().unwrap() as f32,
                i["screen_width"].as_f64().unwrap() as f32,
                i["old_width"].as_f64().unwrap() as f32,
            )
            .unwrap();
            let o = &c["out"];
            assert_eq!(g.x as f64, o["x"].as_f64().unwrap());
            assert_eq!(g.width as f64, o["width"].as_f64().unwrap());
            assert_eq!(g.height as f64, o["height"].as_f64().unwrap());
            for (row, want) in g.rows.iter().zip(o["rows"].as_array().unwrap()) {
                let bg: Vec<f64> = want["bg"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap())
                    .collect();
                let at: Vec<f64> = want["text"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap())
                    .collect();
                assert_eq!(row.0.map(f64::from).to_vec(), bg);
                assert_eq!(row.1.map(f64::from).to_vec(), at);
            }
        }
    }

    #[test]
    fn rest_estimate_fixture() {
        for c in fixtures()["rest_estimate"].as_array().unwrap() {
            let i = &c["in"];
            assert_eq!(
                rest_estimate(
                    i["rest_value"].as_i64().unwrap(),
                    i["ticks"].as_i64().unwrap(),
                    i["rest_at"].as_i64().unwrap()
                ),
                c["out"].as_i64().unwrap()
            );
        }
    }

    #[test]
    fn module_flags_and_settings() {
        const { assert!(MODULE.default_on && !MODULE.cheaty && !MODULE.beta) };
        assert_eq!(MODULE.settings.len(), 6);
    }

    #[test]
    fn layout_decides_what_is_shown() {
        reset();
        let mut h = Fake {
            player: Some(player()),
            ..Fake::default()
        };
        // Default layout: everything "In overlay", so nothing while closed.
        assert!(hud_left(&mut h).is_none());
        // The Build layout shows coordinates always.
        core::with(&mut h, |s, _| {
            s.set_setting(&MODULE, "layout", "Build").unwrap()
        });
        let b = hud_left(&mut h).unwrap();
        let texts: Vec<_> = b
            .ops
            .iter()
            .filter_map(|o| match o {
                HudOp::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts,
            [
                "XYZ: 0.000 / 64.000 / 0.000",
                "Facing: south (+Z) (0.0 / 0.0)"
            ]
        );
        assert_eq!(b.corner, HudCorner::TopLeft);
        assert!(hud_right(&mut h).is_none(), "target entries have no data");
    }

    #[test]
    fn overlay_key_taps_open_and_combo_does_not() {
        reset();
        let mut h = Fake {
            player: Some(player()),
            ..Fake::default()
        };
        let press = |h: &mut Fake, k: &str, d: bool| key_event(h, k, d);
        press(&mut h, "key.keyboard.f4", true);
        press(&mut h, "key.keyboard.f4", false);
        assert!(open());
        press(&mut h, "key.keyboard.f4", true);
        press(&mut h, "key.keyboard.f7", true);
        press(&mut h, "key.keyboard.f4", false);
        assert!(open(), "combo does not toggle");
        assert_eq!(h.bar, ["Overlay layout: Build"]);
        // open overlay shows the "In overlay" entries of the Default layout
        core::with(&mut h, |s, _| {
            s.set_setting(&MODULE, "layout", "Default").unwrap()
        });
        assert!(hud_left(&mut h).is_some());
    }

    #[test]
    fn hud_visibility_of_other_modules() {
        reset();
        let h = Fake {
            player: Some(player()),
            ..Fake::default()
        };
        assert!(shows_hud(&h, "armor"));
        with_layouts(&h, |l| {
            l.list[0].1.hud.insert(
                "armor".into(),
                crate::omnitool_overlay_layouts::Visibility::Overlay,
            );
        });
        assert!(!shows_hud(&h, "armor"));
        STATE.with(|s| s.borrow_mut().open = true);
        assert!(shows_hud(&h, "armor"));
    }

    fn layout_cmd(h: &mut Fake, args: &str) -> Vec<String> {
        h.lines.clear();
        assert!(core::command(h, &format!("ot layout {args}")));
        h.lines.clone()
    }

    #[test]
    fn layout_command_manages_layouts() {
        reset();
        let mut h = Fake::default();
        assert_eq!(
            layout_cmd(&mut h, "list"),
            ["§6[Omnitool] §fLayouts: §aDefault§r, Build, Farm"]
        );
        assert_eq!(
            layout_cmd(&mut h, "use Build"),
            ["§6[Omnitool] §fOverlay layout: Build"]
        );
        assert_eq!(layout_name(&h), "Build");
        assert_eq!(
            layout_cmd(&mut h, "use Nope"),
            ["§c[Omnitool] Unknown layout: Nope"]
        );
        assert_eq!(
            layout_cmd(&mut h, "new \"My Layout\""),
            ["§6[Omnitool] §fLayout My Layout created"]
        );
        assert_eq!(layout_name(&h), "My Layout");
        assert_eq!(
            layout_cmd(&mut h, "new Farm"),
            ["§c[Omnitool] Enter a name that is not used yet"]
        );
        layout_cmd(&mut h, "rename Mine");
        assert_eq!(layout_name(&h), "Mine");
        assert_eq!(
            layout_cmd(&mut h, "delete"),
            ["§6[Omnitool] §fLayout deleted, now Default"]
        );
        assert_eq!(layout_name(&h), "Default");
        assert_eq!(with_layouts(&h, |l| l.names().len()), 3);
    }

    #[test]
    fn layout_command_edits_entries() {
        reset();
        let mut h = Fake::default();
        // fps: In overlay by default -> cycles to Never -> Always
        assert_eq!(layout_cmd(&mut h, "vis fps"), ["§6[Omnitool] §ffps: Never"]);
        assert_eq!(
            layout_cmd(&mut h, "vis fps always"),
            ["§6[Omnitool] §ffps: Always"]
        );
        assert_eq!(
            with_layouts(&h, |l| l.current("Default").unwrap().vis("fps")),
            Visibility::Always
        );
        assert_eq!(
            layout_cmd(&mut h, "vis fps sometimes"),
            ["§c[Omnitool] Usage: /ot layout vis <entry> [always|overlay|never]"]
        );
        assert_eq!(
            layout_cmd(&mut h, "vis nothing"),
            ["§c[Omnitool] Unknown entry: nothing"]
        );
        layout_cmd(&mut h, "move fps down");
        // fps swaps with the next entry of its (left) column, skipping memory (right)
        assert_eq!(
            with_layouts(&h, |l| l.current("Default").unwrap().order[..3].to_vec()),
            ["ping", "memory", "fps"]
        );
        assert_eq!(
            layout_cmd(&mut h, "side fps"),
            ["§6[Omnitool] §ffps: right column"]
        );
        layout_cmd(&mut h, "reset");
        assert_eq!(
            with_layouts(&h, |l| l.current("Default").unwrap().vis("fps")),
            Visibility::Overlay
        );
        let json = layout_cmd(&mut h, "json");
        assert!(json[0].contains("\"entries\""));
        assert_eq!(layout_cmd(&mut h, "").len(), 1);
    }

    #[test]
    fn speed_from_position_delta() {
        reset();
        let mut h = Fake {
            player: Some(player()),
            ..Fake::default()
        };
        tick(&mut h);
        h.player = Some(Player {
            x: 0.3,
            z: 0.4,
            ..player()
        });
        tick(&mut h);
        assert!((state(|s| s.speed) - 10.0).abs() < 1e-6);
    }
}
