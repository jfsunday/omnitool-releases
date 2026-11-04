// Freecam (docs/spec/omnitool/freecam-paths.md, section 1): a detached
// camera that flies while the character stays. The game is behind `Env`
// (player, collisions, held movement keys); the glue ticks it from
// `native_tick`, takes the mouse in `turn_player` and renders from `view` in
// `camera_view`. Camera paths (section 2) are not ported yet. Added to the
// game as `game::omnitool_freecam`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, NONE, SettingDef};
use std::cell::RefCell;

const NUM_SPEED: Kind = Kind::Number {
    default: 1.0,
    min: 0.1,
    max: 10.0,
    step: 0.1,
};

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "freecam",
    name: "Freecam",
    category: Category::Player,
    description: "Fly the camera around while your character stays. Fair play under Safe Mode: collision, no interaction from the camera.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: true,
    replaces: "Freecam",
    key: NONE,
    hold: false,
    settings: &[
        SettingDef::new(
            "flight_mode",
            "Flight mode",
            Kind::Enum {
                values: &["DEFAULT", "CREATIVE"],
                default: 0,
            },
        ),
        SettingDef::new("horizontal_speed", "Horizontal speed", NUM_SPEED),
        SettingDef::new("vertical_speed", "Vertical speed", NUM_SPEED),
        SettingDef::new(
            "sprint_multiplier",
            "Speed multiplier while holding sprint",
            Kind::Number {
                default: 2.5,
                min: 1.0,
                max: 10.0,
                step: 0.5,
            },
        ),
        SettingDef::new("ignore_all", "Fly through all blocks", Kind::Bool(true)),
        // Horizon: the collision check has no per-block filter yet; the
        // three settings below are stored but every block collides.
        SettingDef::new(
            "ignore_transparent",
            "Fly through glass and panes",
            Kind::Bool(false),
        ),
        SettingDef::new(
            "ignore_openable",
            "Fly through doors, trapdoors and gates",
            Kind::Bool(false),
        ),
        SettingDef::new(
            "ignore_custom",
            "Fly through listed blocks",
            Kind::Bool(false),
        ),
        SettingDef::new(
            "custom_blocks",
            "Listed blocks (comma separated ids)",
            Kind::Text(""),
        ),
        SettingDef::new(
            "always_check",
            "Check collision for the start position",
            Kind::Bool(false),
        ),
        SettingDef::new(
            "perspective",
            "Start position",
            Kind::Enum {
                values: &["INSIDE", "FIRST_PERSON", "THIRD_PERSON"],
                default: 0,
            },
        ),
        SettingDef::new("show_player", "Show your player", Kind::Bool(true)),
        // Horizon draws the hand only in first person: shown exactly when the
        // player is hidden (see `camera`).
        SettingDef::new("show_hand", "Show hand", Kind::Bool(false)),
        // Not wired yet (no hook): stored only.
        SettingDef::new(
            "full_bright",
            "Fullbright while in freecam",
            Kind::Bool(false),
        ),
        SettingDef::new(
            "show_submersion",
            "Show underwater/lava fog at the camera",
            Kind::Bool(false),
        ),
        SettingDef::new("outline_player", "Outline your player", Kind::Bool(false)),
        SettingDef::new(
            "disable_on_damage",
            "Turn off when you take damage",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "freeze_player",
            "Freeze your player (no falling/knockback)",
            Kind::Bool(false),
        ),
        SettingDef::new("allow_interact", "Allow interaction", Kind::Bool(true)),
        SettingDef::new(
            "interaction_mode",
            "Interact from",
            Kind::Enum {
                values: &["CAMERA", "PLAYER"],
                default: 0,
            },
        ),
        SettingDef::new(
            "control",
            "Control",
            Kind::Enum {
                values: &["CAMERA", "PLAYER"],
                default: 0,
            },
        ),
        SettingDef::new(
            "control_key",
            "Switch control key (optional)",
            Kind::Key(NONE),
        ),
        SettingDef::new("notify", "Action bar message on toggle", Kind::Bool(true)),
        SettingDef::new(
            "fair_play",
            "Fair play (always on with Safe Mode)",
            Kind::Bool(false),
        ),
        SettingDef::new(
            "max_distance_limit",
            "Max distance from your player (0 = no limit)",
            Kind::Number {
                default: 0.0,
                min: 0.0,
                max: 512.0,
                step: 8.0,
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

/// Half the edge of the camera's box (`Freecam.HALF`).
pub const HALF: f64 = 0.15;

type V3 = [f64; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}

/// `Vec3.directionFromRotation(pitch, yaw)` (float trig as vanilla's `Mth`
/// is close enough here; the camera is not fixture-tested).
pub fn direction(pitch: f32, yaw: f32) -> V3 {
    let (p, y) = ((pitch as f64).to_radians(), (yaw as f64).to_radians());
    let h = p.cos();
    [-y.sin() * h, -p.sin(), y.cos() * h]
}

/// The held movement keys (`steer` already applied by the caller).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
}

fn axis(a: bool, b: bool) -> f64 {
    f64::from(u8::from(a)) - f64::from(u8::from(b))
}

/// Java's `velocity` (spec 1.8): `keys` only count while steering.
pub fn velocity(
    creative: bool,
    old: V3,
    yaw: f32,
    keys: Keys,
    steer: bool,
    speeds: (f64, f64, f64),
) -> V3 {
    let keys = if steer { keys } else { Keys::default() };
    let (f, s, u) = (
        axis(keys.forward, keys.back),
        axis(keys.left, keys.right),
        axis(keys.jump, keys.sneak),
    );
    let r = (yaw as f64).to_radians();
    let (mut x, mut z) = (-r.sin() * f + r.cos() * s, r.cos() * f + r.sin() * s);
    let l = (x * x + z * z).sqrt();
    if l > 1e-6 {
        x /= l;
        z /= l;
    }
    let boost = if keys.sprint { speeds.2 } else { 1.0 };
    let (h, v) = (speeds.0 * boost, speeds.1 * boost);
    if creative {
        let a = 0.1 * h;
        [
            (old[0] + x * a) * 0.91,
            (old[1] + u * 0.3 * v) * 0.6,
            (old[2] + z * a) * 0.91,
        ]
    } else {
        [x * h, u * v, z * h]
    }
}

fn cam_box(at: V3) -> (V3, V3) {
    (sub(at, [HALF; 3]), add(at, [HALF; 3]))
}

/// Java's `move` (spec 1.8): axes Y, X, Z in steps of at most 0.2; a blocked
/// step is bisected 6 times. `collides(min, max)` is the block check.
/// Returns the new position and which axes were blocked.
pub fn moved(
    from: V3,
    delta: V3,
    collide: bool,
    collides: &mut dyn FnMut(V3, V3) -> bool,
) -> (V3, [bool; 3]) {
    let mut hit = [false; 3];
    if !collide || {
        let (a, b) = cam_box(from);
        collides(a, b)
    } {
        return (add(from, delta), hit);
    }
    let mut pos = from;
    for axis in [1usize, 0, 2] {
        let d = delta[axis];
        if d == 0.0 {
            continue;
        }
        let steps = (d.abs() / 0.2).ceil().max(1.0) as usize;
        let step = d / steps as f64;
        for _ in 0..steps {
            let mut next = pos;
            next[axis] += step;
            let (a, b) = cam_box(next);
            if !collides(a, b) {
                pos = next;
                continue;
            }
            let (mut lo, mut hi) = (0.0, step);
            for _ in 0..6 {
                let mid = (lo + hi) / 2.0;
                let mut at = pos;
                at[axis] += mid;
                let (a, b) = cam_box(at);
                if collides(a, b) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            pos[axis] += lo;
            hit[axis] = true;
            break;
        }
    }
    (pos, hit)
}

/// Java's `clampDistance`: `max <= 0` = no limit.
pub fn clamp_distance(eye: V3, at: V3, max: f64) -> V3 {
    let off = sub(at, eye);
    let l = len(off);
    if max <= 0.0 || l <= max {
        at
    } else {
        add(eye, scale(off, max / l))
    }
}

/// Java's `withoutOutward`: at the limit the camera slides along the sphere.
pub fn without_outward(eye: V3, pos: V3, v: V3, max: f64) -> V3 {
    let off = sub(pos, eye);
    let l = len(off);
    if l < 1e-6 || len(sub(add(pos, v), eye)) <= max {
        return v;
    }
    let n = scale(off, 1.0 / l);
    let out = dot(v, n);
    if out > 0.0 { sub(v, scale(n, out)) } else { v }
}

/// The start offset from the eyes (`perspective`: INSIDE, FIRST_PERSON,
/// THIRD_PERSON).
pub fn start_offset(perspective: usize, pitch: f32, yaw: f32) -> V3 {
    let look = direction(pitch, yaw);
    match perspective {
        1 => scale(look, 0.5),
        2 => scale(look, -4.0),
        _ => [0.0; 3],
    }
}

/// The player as Freecam reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    pub eye: V3,
    pub yaw: f32,
    pub pitch: f32,
    pub health: f32,
    /// Changes on respawn (Java compares the `LocalPlayer` instance).
    pub id: u64,
}

/// What Freecam needs from the game.
pub trait Env: Host {
    fn body(&self) -> Option<Body>;
    /// Java's `worldKey`: server or world name + `|` + dimension.
    fn world_key(&self) -> String;
    /// Whether a block's collision shape meets the box.
    fn collides(&mut self, min: V3, max: V3) -> bool;
    /// The movement keys the player holds (physical, through the key binds).
    fn keys(&self) -> Keys;
    /// A GLFW key physically held (`control_key`).
    fn glfw_down(&self, key: i32) -> bool;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tripod {
    pub pos: V3,
    pub yaw: f32,
    pub pitch: f32,
}

/// The running camera (spec 1.4).
#[derive(Clone, Debug, Default)]
pub struct Cam {
    pub running: bool,
    owner: u64,
    world: String,
    pub pos: V3,
    pub prev: V3,
    vel: V3,
    pub yaw: f32,
    pub pitch: f32,
    last_health: f32,
    control_key_down: bool,
    pending: Option<Tripod>,
    tripods: Vec<(String, [Option<Tripod>; 9])>,
}

/// What the glue does with the game after a tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Out {
    /// The camera runs (keep the camera type, see `show_player`).
    pub running: bool,
    /// The camera takes the movement keys (the player stands).
    pub block_input: bool,
    /// The camera just stopped (give the keys back, restore the camera type).
    pub stopped: bool,
}

thread_local! {
    static CAM: RefCell<Cam> = RefCell::new(Cam::default());
}

fn flag(host: &dyn Host, id: &str) -> bool {
    core::value(host, &MODULE, id).on()
}
fn num(host: &dyn Host, id: &str) -> f64 {
    core::value(host, &MODULE, id).num()
}
fn index(host: &dyn Host, id: &str) -> usize {
    core::value(host, &MODULE, id).index()
}

/// Java's `fair()`.
pub fn fair(host: &dyn Host) -> bool {
    flag(host, "fair_play") || core::safe_mode(host, &MODULE)
}
fn collision_on(host: &dyn Host) -> bool {
    fair(host) || !flag(host, "ignore_all")
}
fn controlling_camera(host: &dyn Host) -> bool {
    index(host, "control") == 0
}
fn max_distance(host: &dyn Host) -> f64 {
    num(host, "max_distance_limit")
}

fn notify(host: &mut dyn Host, text: &str) {
    if flag(host, "notify") {
        let t = core::tr(host, text);
        host.action_bar(&t);
    }
}

fn fair_suffix(host: &dyn Host) -> String {
    if fair(host) {
        format!(" {}", core::tr(host, "(fair play)"))
    } else {
        String::new()
    }
}

impl Cam {
    fn start(&mut self, env: &mut dyn Env, body: &Body) {
        self.owner = body.id;
        self.world = env.world_key();
        self.vel = [0.0; 3];
        let at = if let Some(t) = self.pending.take() {
            self.yaw = t.yaw;
            self.pitch = t.pitch;
            t.pos
        } else {
            self.yaw = body.yaw;
            self.pitch = body.pitch;
            let offset = start_offset(index(env, "perspective"), body.pitch, body.yaw);
            let check = collision_on(env) || flag(env, "always_check");
            moved(body.eye, offset, check, &mut |a, b| env.collides(a, b)).0
        };
        self.pos = clamp_distance(body.eye, at, max_distance(env));
        self.prev = self.pos;
        self.last_health = body.health;
        self.running = true;
        let text = format!("{}{}", core::tr(env, "Freecam on"), fair_suffix(env));
        if flag(env, "notify") {
            env.action_bar(&text);
        }
    }

    fn stop(&mut self, env: &mut dyn Env) {
        self.running = false;
        self.owner = 0;
        notify(env, "Freecam off");
    }

    /// Java's `tick` (spec 1.7), incl. `sync`.
    pub fn tick(&mut self, env: &mut dyn Env) -> Out {
        let body = env.body();
        let want = core::active(env, &MODULE) && body.is_some();
        let mut out = Out::default();
        if !want {
            self.pending = None;
            if self.running {
                self.stop(env);
                out.stopped = true;
            }
            // Java's onEnable: no player → switch off again.
            if body.is_none() && core::enabled(env, &MODULE) {
                core::set_enabled(env, &MODULE, false);
            }
            return out;
        }
        let body = body.unwrap();
        if !self.running {
            self.start(env, &body);
        } else if body.id != self.owner || env.world_key() != self.world {
            core::set_enabled(env, &MODULE, false);
            self.stop(env);
            out.stopped = true;
            return out;
        }
        if flag(env, "disable_on_damage") && body.health < self.last_health {
            core::set_enabled(env, &MODULE, false);
            self.stop(env);
            out.stopped = true;
            return out;
        }
        self.last_health = body.health;
        let key = core::value(env, &MODULE, "control_key").key();
        let down = !env.screen_open() && key != NONE && env.glfw_down(key);
        if down && !self.control_key_down {
            let to_player = controlling_camera(env);
            control(env, to_player, self.running);
        }
        self.control_key_down = down;
        let camera = controlling_camera(env);
        out.running = true;
        out.block_input = camera;
        self.prev = self.pos;
        let steer = camera && !env.screen_open();
        let speeds = (
            num(env, "horizontal_speed"),
            num(env, "vertical_speed"),
            num(env, "sprint_multiplier"),
        );
        self.vel = velocity(
            index(env, "flight_mode") == 1,
            self.vel,
            self.yaw,
            env.keys(),
            steer,
            speeds,
        );
        let max = max_distance(env);
        if max > 0.0 {
            self.vel = without_outward(body.eye, self.pos, self.vel, max);
        }
        let collide = collision_on(env);
        let (pos, hit) = moved(self.pos, self.vel, collide, &mut |a, b| env.collides(a, b));
        self.pos = pos;
        for (i, h) in hit.iter().enumerate() {
            if *h {
                self.vel[i] = 0.0;
            }
        }
        if max > 0.0 {
            let target = clamp_distance(body.eye, self.pos, max);
            let d = sub(target, self.pos);
            if dot(d, d) > 1e-8 {
                self.pos = moved(self.pos, d, true, &mut |a, b| env.collides(a, b)).0;
            }
        }
        out
    }

    /// Java's `onTurn` (spec 1.9): true = the mouse turned the camera.
    pub fn turn(&mut self, camera: bool, dyaw: f64, dpitch: f64) -> bool {
        if !self.running || !camera {
            return false;
        }
        self.yaw += dyaw as f32 * 0.15;
        self.pitch = (self.pitch + dpitch as f32 * 0.15).clamp(-90.0, 90.0);
        true
    }

    /// The pose to render from (`applyCamera`): interpolated position.
    pub fn view(&self, partial: f64) -> Option<(V3, f32, f32)> {
        self.running.then(|| {
            (
                add(self.prev, scale(sub(self.pos, self.prev), partial)),
                self.yaw,
                self.pitch,
            )
        })
    }

    #[allow(dead_code)] // used by the tripods
    fn slots(&mut self, key: &str) -> &mut [Option<Tripod>; 9] {
        let i = match self.tripods.iter().position(|(k, _)| k == key) {
            Some(i) => i,
            None => {
                self.tripods.push((key.to_owned(), [None; 9]));
                self.tripods.len() - 1
            }
        };
        &mut self.tripods[i].1
    }
}

/// Java's `setControl`: flips CAMERA/PLAYER (`to_player` = new value PLAYER).
#[allow(dead_code)] // the workflow step freecam_control comes with the next step
pub fn set_control(host: &mut dyn Host, to_player: bool) {
    let running = CAM.with_borrow(|c| c.running);
    control(host, to_player, running);
}

fn control(host: &mut dyn Host, to_player: bool, running: bool) {
    let value = if to_player { "PLAYER" } else { "CAMERA" };
    let _ = core::with(host, |s, _| s.set_setting(&MODULE, "control", value));
    if running {
        notify(
            host,
            if to_player {
                "Freecam: controlling player"
            } else {
                "Freecam: controlling camera"
            },
        );
    }
}

/// Java's `tripodNumber`.
#[allow(dead_code)] // for the workflow steps tripod_save/tripod_load
pub fn tripod_number(arg: &str) -> i32 {
    arg.trim().parse().unwrap_or(-1)
}

/// Java's `saveTripod(n)`.
#[allow(dead_code)] // the workflow steps tripod_save/tripod_load come with the next step
pub fn save_tripod(env: &mut dyn Env, n: i32) {
    if !(1..=9).contains(&n) {
        return;
    }
    let Some(body) = env.body() else { return };
    let key = env.world_key();
    CAM.with_borrow_mut(|c| {
        let t = if c.running {
            Tripod {
                pos: c.pos,
                yaw: c.yaw,
                pitch: c.pitch,
            }
        } else {
            Tripod {
                pos: body.eye,
                yaw: body.yaw,
                pitch: body.pitch,
            }
        };
        c.slots(&key)[n as usize - 1] = Some(t);
    });
    let text = format!("{}: {n}", core::tr(env, "Tripod saved"));
    if flag(env, "notify") {
        env.action_bar(&text);
    }
}

/// Java's `loadTripod(n)`.
#[allow(dead_code)]
pub fn load_tripod(env: &mut dyn Env, n: i32) {
    if !(1..=9).contains(&n) {
        return;
    }
    let key = env.world_key();
    let Some(t) = CAM.with_borrow_mut(|c| c.slots(&key)[n as usize - 1]) else {
        let text = format!("{}: {n}", core::tr(env, "Tripod is empty"));
        env.action_bar(&text);
        return;
    };
    let max = max_distance(env);
    let eye = env.body().map(|b| b.eye);
    let running = CAM.with_borrow_mut(|c| {
        if c.running {
            c.pos = eye.map_or(t.pos, |e| clamp_distance(e, t.pos, max));
            c.prev = c.pos;
            c.vel = [0.0; 3];
            c.yaw = t.yaw;
            c.pitch = t.pitch;
        } else {
            c.pending = Some(t);
        }
        c.running
    });
    if !running {
        core::set_enabled(env, &MODULE, true);
    }
    let text = format!("{} {n}{}", core::tr(env, "Tripod"), fair_suffix(env));
    if flag(env, "notify") {
        env.action_bar(&text);
    }
}

/// One client tick.
pub fn tick(env: &mut dyn Env) -> Out {
    CAM.with_borrow_mut(|c| c.tick(env))
}

/// The mouse turn.
pub fn turn(host: &dyn Host, dyaw: f64, dpitch: f64) -> bool {
    let camera = controlling_camera(host);
    CAM.with_borrow_mut(|c| c.turn(camera, dyaw, dpitch))
}

/// The camera pose this frame.
pub fn view(partial: f64) -> Option<(V3, f32, f32)> {
    CAM.with_borrow(|c| c.view(partial))
}

/// Whether the player model is drawn (`show_player`) while running.
pub fn show_player(host: &dyn Host) -> bool {
    flag(host, "show_player")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};

    struct World {
        host: Fake,
        body: Option<Body>,
        keys: Keys,
        floor: f64,
    }
    impl Host for World {
        fn chat(&mut self, line: &str) {
            self.host.chat(line)
        }
        fn player(&self) -> Option<crate::omnitool_core::Player> {
            None
        }
        fn data_dir(&self) -> Option<std::path::PathBuf> {
            None
        }
        fn action_bar(&mut self, text: &str) {
            self.host.bar.push(text.to_owned());
        }
    }
    impl Env for World {
        fn body(&self) -> Option<Body> {
            self.body.clone()
        }
        fn world_key(&self) -> String {
            "local|minecraft:overworld".into()
        }
        fn collides(&mut self, min: V3, _max: V3) -> bool {
            min[1] < self.floor
        }
        fn keys(&self) -> Keys {
            self.keys
        }
        fn glfw_down(&self, _key: i32) -> bool {
            false
        }
    }
    fn body() -> Body {
        Body {
            eye: [0.5, 65.62, 0.5],
            yaw: 0.0,
            pitch: 0.0,
            health: 20.0,
            id: 1,
        }
    }

    #[test]
    fn velocity_default_and_creative() {
        let k = Keys {
            forward: true,
            ..Keys::default()
        };
        // yaw 0 faces +z.
        let v = velocity(false, [0.0; 3], 0.0, k, true, (1.0, 1.0, 2.5));
        assert!((v[2] - 1.0).abs() < 1e-9 && v[0].abs() < 1e-9);
        let k = Keys {
            forward: true,
            left: true,
            sprint: true,
            ..Keys::default()
        };
        let v = velocity(false, [0.0; 3], 0.0, k, true, (1.0, 1.0, 2.5));
        assert!((len(v) - 2.5).abs() < 1e-9);
        let v = velocity(false, [0.0; 3], 0.0, k, false, (1.0, 1.0, 2.5));
        assert_eq!(v, [0.0; 3]);
        let up = Keys {
            jump: true,
            ..Keys::default()
        };
        let v = velocity(true, [0.0; 3], 0.0, up, true, (1.0, 1.0, 1.0));
        assert!((v[1] - 0.3 * 0.6).abs() < 1e-9);
    }

    #[test]
    fn move_stops_at_floor() {
        let mut floor = |a: V3, _b: V3| a[1] < 64.0;
        let (p, hit) = moved([0.0, 65.0, 0.0], [0.0, -3.0, 0.0], true, &mut floor);
        assert!(hit[1]);
        assert!(
            p[1] - HALF >= 64.0 && p[1] - HALF < 64.0 + 0.2 / 64.0 + 1e-9,
            "{p:?}"
        );
        let (p, hit) = moved([0.0, 65.0, 0.0], [0.0, -3.0, 0.0], false, &mut floor);
        assert_eq!((p[1], hit), (62.0, [false; 3]));
        // Starting inside blocks: no check.
        let (p, _) = moved([0.0, 63.0, 0.0], [0.0, -1.0, 0.0], true, &mut floor);
        assert_eq!(p[1], 62.0);
    }

    #[test]
    fn distance_limit() {
        let eye = [0.0; 3];
        assert_eq!(clamp_distance(eye, [10.0, 0.0, 0.0], 0.0), [10.0, 0.0, 0.0]);
        assert_eq!(clamp_distance(eye, [10.0, 0.0, 0.0], 8.0), [8.0, 0.0, 0.0]);
        let v = without_outward(eye, [8.0, 0.0, 0.0], [1.0, 1.0, 0.0], 8.0);
        assert_eq!(v, [0.0, 1.0, 0.0]);
        let v = without_outward(eye, [8.0, 0.0, 0.0], [-1.0, 0.0, 0.0], 8.0);
        assert_eq!(v, [-1.0, 0.0, 0.0]);
    }

    #[test]
    fn start_fly_stop() {
        restart();
        let mut w = World {
            host: Fake::default(),
            body: Some(body()),
            keys: Keys::default(),
            floor: -100.0,
        };
        let mut c = Cam::default();
        assert_eq!(c.tick(&mut w), Out::default());
        core::set_enabled(&mut w, &MODULE, true);
        let out = c.tick(&mut w);
        assert!(out.running && out.block_input);
        assert_eq!(c.pos, body().eye);
        // Safe Mode is on in a fresh config: fair play.
        assert_eq!(w.host.bar.last().unwrap(), "Freecam on (fair play)");
        w.keys.forward = true;
        c.tick(&mut w);
        assert!((c.pos[2] - (body().eye[2] + 1.0)).abs() < 1e-9);
        let (p, ..) = c.view(0.5).unwrap();
        assert!((p[2] - (body().eye[2] + 0.5)).abs() < 1e-9);
        assert!(c.turn(true, 100.0, 0.0));
        assert!((c.yaw - 15.0).abs() < 1e-4);
        assert!(!c.turn(false, 100.0, 0.0));
        // Damage turns it off.
        w.body.as_mut().unwrap().health = 19.0;
        let out = c.tick(&mut w);
        assert!(out.stopped && !c.running);
        assert!(!core::enabled(&w, &MODULE));
        assert_eq!(w.host.bar.last().unwrap(), "Freecam off");
    }

    #[test]
    fn respawn_stops() {
        restart();
        let mut w = World {
            host: Fake::default(),
            body: Some(body()),
            keys: Keys::default(),
            floor: -100.0,
        };
        let mut c = Cam::default();
        core::set_enabled(&mut w, &MODULE, true);
        c.tick(&mut w);
        w.body.as_mut().unwrap().id = 2;
        assert!(c.tick(&mut w).stopped);
        assert!(!core::enabled(&w, &MODULE));
    }

    #[test]
    fn third_person_start_collides() {
        restart();
        let mut b = body();
        b.pitch = -90.0; // looking up: 4 blocks down, floor at 64
        let mut w = World {
            host: Fake::default(),
            body: Some(b),
            keys: Keys::default(),
            floor: 64.0,
        };
        core::with(&mut w, |s, _| {
            s.set_setting(&MODULE, "perspective", "THIRD_PERSON")
                .unwrap();
            s.set_setting(&MODULE, "ignore_all", "false").unwrap();
        });
        core::set_enabled(&mut w, &MODULE, true);
        let mut c = Cam::default();
        c.tick(&mut w);
        assert!(c.pos[1] - HALF >= 64.0 && c.pos[1] < 65.62, "{:?}", c.pos);
    }

    #[test]
    fn tripod_number_parse() {
        assert_eq!(tripod_number(" 3 "), 3);
        assert_eq!(tripod_number("x"), -1);
    }
}
