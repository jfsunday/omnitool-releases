// Workflows (Java's core/Workflows, Binding, Bindings): named step lists, their
// stored form in config.json (`bindings`), the step catalog and parsers, the
// runner with Java's timing (wait, if/else/repeat/end, run depth, per-tick
// limits) and the input actions over the `Input` trait. Pure: the game is
// behind `Env` (what a step needs) and `Input` (simulated player input);
// core.rs implements `Env` on the `Host`. Checked against
// tests/fixtures/workflows.json. Added to the game as
// `game::omnitool_workflows`; also a module of this crate.

use super::omnitool_json::{Json, Obj, parse_java_double};
use super::omnitool_setting::{NONE, java_round};
use super::omnitool_trading::{self as trading, Trader};
use std::sync::atomic::{AtomicU64, Ordering};

// ------------------------------------------------------------ text helpers

/// Java's `String.trim`: strips chars <= ' '.
pub fn jtrim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// Java's `split("\\s+")` without empty tokens.
pub fn tokens(s: &str) -> impl Iterator<Item = &str> {
    s.split([' ', '\t', '\n', '\x0B', '\x0C', '\r'])
        .filter(|t| !t.is_empty())
}

// ------------------------------------------------------------ steps

/// The editor's argument kind (Java's `Step.Arg`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg {
    None,
    Module,
    Profile,
    Workflow,
    OnOff,
    Text,
}

impl Arg {
    pub fn name(self) -> &'static str {
        match self {
            Arg::None => "NONE",
            Arg::Module => "MODULE",
            Arg::Profile => "PROFILE",
            Arg::Workflow => "WORKFLOW",
            Arg::OnOff => "ONOFF",
            Arg::Text => "TEXT",
        }
    }
}

/// One step type (Java's `Workflows.Step`).
#[derive(Debug, PartialEq, Eq)]
pub struct Step {
    pub id: &'static str,
    pub arg: Arg,
    /// Simulates player input: skipped while Safe Mode blocks.
    pub input: bool,
    /// Beta channel only (hidden and skipped otherwise).
    pub beta: bool,
}

const fn st(id: &'static str, arg: Arg, input: bool, beta: bool) -> Step {
    Step {
        id,
        arg,
        input,
        beta,
    }
}

/// All 39 steps in Java's enum order.
pub const STEPS: [Step; 39] = [
    st("toggle", Arg::Module, false, false),
    st("enable", Arg::Module, false, false),
    st("disable", Arg::Module, false, false),
    st("set", Arg::Text, false, false),
    st("profile", Arg::Profile, false, false),
    st("safemode", Arg::OnOff, false, false),
    st("chat", Arg::Text, false, false),
    st("command", Arg::Text, false, false),
    st("wait", Arg::Text, false, false),
    st("run", Arg::Workflow, false, false),
    st("waypoint", Arg::Text, false, false),
    st("sort", Arg::None, false, false),
    st("panorama", Arg::None, false, false),
    st("disconnect", Arg::None, false, false),
    st("tripod_save", Arg::Text, false, false),
    st("tripod_load", Arg::Text, false, false),
    st("freecam_control", Arg::OnOff, false, false),
    st("walk", Arg::Text, true, true),
    st("look", Arg::Text, true, true),
    st("turn", Arg::Text, true, true),
    st("jump", Arg::None, true, true),
    st("attack", Arg::None, true, true),
    st("use", Arg::None, true, true),
    st("hold", Arg::Text, true, true),
    st("hotbar", Arg::Text, true, true),
    st("move", Arg::Text, true, true),
    st("open", Arg::Text, true, true),
    st("craft", Arg::Text, true, true),
    st("close", Arg::None, false, false),
    st("if", Arg::Text, false, true),
    st("else", Arg::None, false, true),
    st("end", Arg::None, false, true),
    st("repeat", Arg::Text, false, true),
    st("stop", Arg::None, false, true),
    st("hotbar_preset", Arg::Text, false, true),
    st("trade", Arg::Text, true, true),
    st("villager", Arg::Text, true, true),
    st("camera_path", Arg::Text, false, true),
    st("camera_stop", Arg::None, false, true),
];

impl Step {
    /// Java's `available()`.
    pub fn available(&self, beta: bool, curseforge: bool) -> bool {
        (!self.beta || beta) && !(self.input && curseforge)
    }
}

/// Java's `Step.of`: the step of `raw`'s verb, None if unknown.
pub fn step_of(raw: &str) -> Option<&'static Step> {
    let v = verb(raw);
    STEPS.iter().find(|s| s.id == v)
}

/// Text before the first `:`, trimmed, lowercase.
pub fn verb(step: &str) -> String {
    let v = step.split_once(':').map_or(step, |(v, _)| v);
    jtrim(v).to_lowercase()
}

/// Text after the first `:`, trimmed; "" without one.
pub fn arg(step: &str) -> &str {
    step.split_once(':').map_or("", |(_, a)| jtrim(a))
}

// ------------------------------------------------------------ numbers

/// Java's `parse`: `,` → `.`, every `s` removed, `Double.parseDouble`.
pub fn parse(s: &str, fallback: f64) -> f64 {
    parse_java_double(&s.replace(',', ".").replace('s', "")).unwrap_or(fallback)
}

/// Seconds from `2`, `1.5s`, `1,5` or `250ms`.
pub fn seconds(s: &str, fallback: f64) -> f64 {
    let t = jtrim(s).to_lowercase();
    match t.strip_suffix("ms") {
        Some(ms) => parse(ms, fallback * 1000.0) / 1000.0,
        None => parse(&t, fallback),
    }
}

/// Game ticks for a duration; anything above 0 is at least one tick.
pub fn ticks(seconds: f64) -> i32 {
    if seconds <= 0.0 {
        0
    } else {
        // (int) Math.round(x): the long cast to int
        ((java_round(seconds * 20.0) as i64) as i32).max(1)
    }
}

/// `3` = 3 times, `forever` = until a stop step.
pub fn repeat_count(arg: &str) -> i32 {
    let a = jtrim(arg).to_lowercase();
    if matches!(
        a.as_str(),
        "forever" | "infinite" | "endless" | "immer" | "unendlich"
    ) {
        return i32::MAX;
    }
    parse(&a.replace('x', ""), 1.0).max(0.0) as i32
}

// ------------------------------------------------------------ blocks

/// Java's `blocks`: jump target and nesting level per step. if → its else or
/// end, else → its end, repeat → its end, end → its opener; unclosed blocks
/// jump to `steps.len()`, unmatched else/end get -1.
pub fn blocks(steps: &[String]) -> (Vec<i32>, Vec<i32>) {
    let n = steps.len();
    let mut jump = vec![-1i32; n];
    let mut level = vec![0i32; n];
    // (opener, current = opener or its else)
    let mut open: Vec<(usize, usize)> = Vec::new();
    for i in 0..n {
        let t = step_of(&steps[i]).map(|s| s.id);
        level[i] = open.len() as i32;
        match t {
            Some("if" | "repeat") => open.push((i, i)),
            Some("else") => {
                if let Some(top) = open.last_mut()
                    && step_of(&steps[top.0]).is_some_and(|s| s.id == "if")
                    && top.1 == top.0
                {
                    jump[top.0] = i as i32;
                    top.1 = i;
                    level[i] = open.len() as i32 - 1;
                }
            }
            Some("end") => {
                if let Some(top) = open.pop() {
                    jump[top.1] = i as i32;
                    jump[i] = top.0 as i32;
                    level[i] = open.len() as i32;
                }
            }
            _ => {}
        }
    }
    for top in open {
        jump[top.1] = n as i32;
    }
    (jump, level)
}

// ------------------------------------------------------------ input parsers

fn direction(tok: &str) -> Option<&'static str> {
    Some(match tok {
        "forward" | "forwards" | "f" | "w" | "vor" | "vorne" | "vorwärts" => "forward",
        "back" | "backward" | "backwards" | "b" | "s" | "zurück" | "rückwärts" => "back",
        "left" | "l" | "a" | "links" => "left",
        "right" | "r" | "d" | "rechts" => "right",
        _ => return None,
    })
}

fn starts_numeric(tok: &str) -> bool {
    tok.starts_with(|c: char| c.is_ascii_digit() || c == '.')
}

/// Parsed `walk` argument: direction plus blocks or seconds.
#[derive(Clone, Debug, PartialEq)]
pub struct Walk {
    pub dir: &'static str,
    pub amount: f64,
    pub seconds: bool,
}

/// `forward 3` = 3 blocks, `left 2s` = 2 seconds, `5` = 5 blocks forward.
pub fn parse_walk(arg: &str) -> Walk {
    let (mut dir, mut amount, mut secs) = ("forward", 1.0, false);
    let lower = jtrim(arg).to_lowercase();
    for tok in tokens(&lower) {
        if let Some(d) = direction(tok) {
            dir = d;
            continue;
        }
        secs = tok.ends_with('s');
        amount = if secs {
            seconds(tok, amount)
        } else {
            parse(tok, amount)
        };
    }
    Walk {
        dir,
        amount: amount.max(0.0),
        seconds: secs,
    }
}

/// Parsed `hold` argument: key name plus seconds.
#[derive(Clone, Debug, PartialEq)]
pub struct Hold {
    pub key: String,
    pub seconds: f64,
}

pub fn parse_hold(arg: &str) -> Hold {
    let (mut key, mut secs) = ("use".to_owned(), 1.0);
    let lower = jtrim(arg).to_lowercase();
    for tok in tokens(&lower) {
        if starts_numeric(tok) {
            secs = seconds(tok, secs);
        } else {
            key = direction(tok).unwrap_or(tok).to_owned();
        }
    }
    Hold {
        key,
        seconds: secs.max(0.0),
    }
}

/// Keys a `move` step can hold, in their written order (also the
/// recorder's bit order).
pub const MOVE_KEYS: [&str; 9] = [
    "forward", "back", "left", "right", "jump", "sneak", "sprint", "attack", "use",
];

/// Parsed `move` argument: keys held at once for a duration.
#[derive(Clone, Debug, PartialEq)]
pub struct Move {
    pub keys: Vec<&'static str>,
    pub seconds: f64,
}

/// `forward+left+sprint 1.2s`, `jump sneak 500ms`.
pub fn parse_move(arg: &str) -> Move {
    let mut keys = Vec::new();
    let mut secs = 1.0;
    let lower = jtrim(arg).to_lowercase();
    for tok in lower
        .split([' ', '\t', '\n', '\x0B', '\x0C', '\r', '+', ','])
        .filter(|t| !t.is_empty())
    {
        if starts_numeric(tok) {
            secs = seconds(tok, secs);
            continue;
        }
        if let Some(k) = move_key(tok) {
            keys.push(k);
        }
    }
    Move {
        keys: MOVE_KEYS
            .iter()
            .copied()
            .filter(|k| keys.contains(k))
            .collect(),
        seconds: secs.max(0.0),
    }
}

/// A key name (with aliases) as one of `MOVE_KEYS` (Java's `key()`).
pub fn move_key(tok: &str) -> Option<&'static str> {
    direction(tok).or(match tok {
        "jump" | "space" => Some("jump"),
        "sneak" | "shift" => Some("sneak"),
        "sprint" | "run" => Some("sprint"),
        "attack" | "mine" | "left_click" => Some("attack"),
        "use" | "eat" | "right_click" => Some("use"),
        _ => None,
    })
}

/// Java's `parseLook`: target `(yaw, pitch)`, NaN = keep the current value.
pub fn parse_look(
    arg: &str,
    relative: bool,
    yaw: f32,
    pitch: f32,
    eye: Option<[f64; 3]>,
) -> (f32, f32) {
    let lower = jtrim(arg).to_lowercase();
    let mut n: Vec<f64> = Vec::new();
    let mut named: Option<f32> = None;
    let mut named_pitch: Option<f32> = None;
    for tok in tokens(&lower) {
        match tok {
            "south" | "süd" | "süden" => named = Some(0.0),
            "west" | "westen" => named = Some(90.0),
            "north" | "nord" | "norden" => named = Some(180.0),
            "east" | "ost" | "osten" => named = Some(-90.0),
            "up" | "hoch" | "oben" => named_pitch = Some(-90.0),
            "down" | "runter" | "unten" => named_pitch = Some(90.0),
            "straight" | "gerade" | "ahead" => named_pitch = Some(0.0),
            "left" | "links" => named = relative.then_some(-90.0),
            "right" | "rechts" => named = relative.then_some(90.0),
            "around" | "umdrehen" => named = relative.then_some(180.0),
            // Java counts the slot before parsing: an unknown word is a 0
            _ => n.push(parse_java_double(&tok.replace(',', ".")).unwrap_or(0.0)),
        }
    }
    let (mut y, mut p) = (f32::NAN, f32::NAN);
    match eye {
        Some(eye) if !relative && n.len() == 3 => {
            // block coordinates: aim at the block centre
            let dx = n[0] + 0.5 - eye[0];
            let dy = n[1] + 0.5 - eye[1];
            let dz = n[2] + 0.5 - eye[2];
            y = (-dx).atan2(dz).to_degrees() as f32;
            p = -(dy.atan2((dx * dx + dz * dz).sqrt()).to_degrees()) as f32;
        }
        _ => {
            let mut i = 0;
            if let Some(v) = named {
                y = v;
            } else if n.len() > i {
                y = n[i] as f32;
                i += 1;
            }
            if let Some(v) = named_pitch {
                p = v;
            } else if n.len() > i {
                p = n[i] as f32;
            }
            if relative {
                if !y.is_nan() {
                    y += yaw;
                }
                if !p.is_nan() && named_pitch.is_none() {
                    p += pitch;
                }
            }
        }
    }
    if !p.is_nan() {
        p = p.clamp(-90.0, 90.0);
    }
    (y, p)
}

/// Parsed `craft` argument.
#[allow(dead_code)] // the craft action (recipe book) uses it
#[derive(Clone, Debug, PartialEq)]
pub struct Craft {
    pub item: String,
    /// 0 = one craft.
    pub amount: i32,
    pub all: bool,
    /// Inventory full: drop the results instead of stopping.
    pub drop: bool,
}

/// `stick 16`, `minecraft:torch all`, `crafting table` (= 1 craft).
#[allow(dead_code)] // the craft action (recipe book) uses it
pub fn parse_craft(arg: &str) -> Craft {
    let mut words = Vec::new();
    let (mut amount, mut all, mut drop) = (0, false, false);
    let lower = jtrim(arg).to_lowercase();
    for tok in tokens(&lower) {
        let digits = tok.strip_suffix('x').unwrap_or(tok);
        if matches!(tok, "all" | "max" | "alle") {
            all = true;
        } else if tok == "drop" {
            drop = true;
        } else if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            // Java throws on too big numbers; the largest int here
            amount = digits.parse().unwrap_or(i32::MAX);
        } else {
            words.push(tok);
        }
    }
    Craft {
        item: words.join(" "),
        amount,
        all,
        drop,
    }
}

fn singular(s: &str) -> &str {
    if s.chars().count() > 3 && s.ends_with('s') {
        &s[..s.len() - 1]
    } else {
        s
    }
}

/// Matches `stick`, `minecraft:stick`, `oak planks` or the shown name.
pub fn item_matches(query: &str, id: &str, shown: &str) -> bool {
    let q = jtrim(query).to_lowercase().replace(' ', "_");
    let path = id.split_once(':').map_or(id, |(_, p)| p);
    let name = jtrim(shown).to_lowercase().replace(' ', "_");
    if q.is_empty() {
        return false;
    }
    if id == q || path == q || name == q {
        return true;
    }
    if let (Some((qn, _)), Some((idn, _))) = (q.split_once(':'), id.split_once(':'))
        && qn != idn
    {
        return false;
    }
    let qp = singular(q.split_once(':').map_or(q.as_str(), |(_, p)| p));
    qp == singular(path) || (!name.is_empty() && qp == singular(&name))
}

/// Whether a craft/trade argument has the `drop` option.
pub fn has_drop(arg: &str) -> bool {
    tokens(&jtrim(arg).to_lowercase()).any(|t| t == "drop")
}

/// Adds or removes the `drop` option word.
pub fn with_drop(arg: &str, drop: bool) -> String {
    let mut out: Vec<&str> = tokens(jtrim(arg))
        .filter(|t| !t.eq_ignore_ascii_case("drop"))
        .collect();
    if drop {
        out.push("drop");
    }
    out.join(" ")
}

// ------------------------------------------------------------ bindings

/// `PRESS` or `HOLD` (steps undone on release).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Press,
    Hold,
}

/// Java's `Binding.Trigger`: name, label, argument hint ("" = no argument).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    None,
    ServerJoin,
    WorldJoin,
    Death,
    Respawn,
    Chat,
    HealthBelow,
    HungerBelow,
    InventoryFull,
    DimensionChange,
    Interval,
}

impl Trigger {
    pub const ALL: [Trigger; 11] = [
        Trigger::None,
        Trigger::ServerJoin,
        Trigger::WorldJoin,
        Trigger::Death,
        Trigger::Respawn,
        Trigger::Chat,
        Trigger::HealthBelow,
        Trigger::HungerBelow,
        Trigger::InventoryFull,
        Trigger::DimensionChange,
        Trigger::Interval,
    ];

    pub fn name(self) -> &'static str {
        self.info().0
    }

    /// Editor label.
    pub fn label(self) -> &'static str {
        self.info().1
    }

    /// Placeholder of the argument field; "" = no argument.
    pub fn hint(self) -> &'static str {
        self.info().2
    }

    fn info(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Trigger::None => ("NONE", "No trigger", ""),
            Trigger::ServerJoin => (
                "SERVER_JOIN",
                "Server join",
                "server ip contains (optional)",
            ),
            Trigger::WorldJoin => (
                "WORLD_JOIN",
                "World join",
                "world / server contains (optional)",
            ),
            Trigger::Death => ("DEATH", "Death", ""),
            Trigger::Respawn => ("RESPAWN", "Respawn", ""),
            Trigger::Chat => ("CHAT", "Chat message contains", "text"),
            Trigger::HealthBelow => ("HEALTH_BELOW", "Health below", "e.g. 6"),
            Trigger::HungerBelow => ("HUNGER_BELOW", "Hunger below", "e.g. 6"),
            Trigger::InventoryFull => ("INVENTORY_FULL", "Inventory full", ""),
            Trigger::DimensionChange => (
                "DIMENSION_CHANGE",
                "Dimension change",
                "nether / overworld / end (optional)",
            ),
            Trigger::Interval => ("INTERVAL", "Every ... seconds", "seconds, e.g. 30"),
        }
    }

    pub fn by_name(name: &str) -> Option<Trigger> {
        Trigger::ALL.into_iter().find(|t| t.name() == name)
    }
}

static NEXT_UID: AtomicU64 = AtomicU64::new(1);

/// A new identity for a workflow (Java compares `Binding`s by identity).
pub fn uid() -> u64 {
    NEXT_UID.fetch_add(1, Ordering::Relaxed)
}

/// A named workflow (Java's `Binding`).
#[derive(Clone, Debug)]
pub struct Binding {
    pub name: String,
    /// GLFW key code, `NONE` = no key.
    pub key: i32,
    /// None = an unknown mode in the file (Gson: null; acts as PRESS, not written).
    pub mode: Option<Mode>,
    pub steps: Vec<String>,
    pub trigger: Trigger,
    pub trigger_arg: String,
    /// Identity for runners, triggers and the recorder; not stored.
    pub uid: u64,
}

impl PartialEq for Binding {
    /// Same stored fields (the identity doesn't count).
    fn eq(&self, o: &Binding) -> bool {
        self.to_json() == o.to_json()
    }
}

impl Binding {
    /// Java's `new Binding()`.
    pub fn new(name: &str) -> Binding {
        Binding {
            name: name.to_owned(),
            key: NONE,
            mode: Some(Mode::Press),
            steps: Vec::new(),
            trigger: Trigger::None,
            trigger_arg: String::new(),
            uid: uid(),
        }
    }

    pub fn hold(&self) -> bool {
        self.mode == Some(Mode::Hold)
    }

    /// Gson's `fromJson(Binding.class)` plus `migrate(index)`; None where
    /// Gson throws (wrong types).
    pub fn from_json(j: &Json, index: usize) -> Option<Binding> {
        let o = j.as_obj()?;
        let mut b = Binding::new("");
        let text = |key: &str| -> Option<Option<String>> {
            match o.get(key) {
                None | Some(Json::Null) => Some(None),
                Some(v) if v.is_primitive() => Some(v.as_string()),
                Some(_) => None,
            }
        };
        let name = text("name")?;
        if let Some(k) = o.get("key").filter(|k| **k != Json::Null) {
            b.key = k.as_int()?;
        }
        b.mode = match text("mode")? {
            None => Some(Mode::Press),
            Some(m) if m == "PRESS" => Some(Mode::Press),
            Some(m) if m == "HOLD" => Some(Mode::Hold),
            Some(_) => None,
        };
        let steps = match o.get("steps") {
            None | Some(Json::Null) => None,
            Some(Json::Arr(a)) => Some(
                a.iter()
                    .map(|s| s.as_string())
                    .collect::<Option<Vec<String>>>()?,
            ),
            Some(_) => return None,
        };
        let actions = text("actions")?;
        let trigger = text("trigger")?;
        let trigger_arg = text("triggerArg")?;
        // migrate
        b.steps = steps.unwrap_or_default();
        if let Some(actions) = actions
            && b.steps.is_empty()
        {
            b.steps = actions
                .split(',')
                .filter(|a| !a.trim().is_empty())
                .map(|a| jtrim(a).to_owned())
                .collect();
        }
        b.trigger = trigger
            .and_then(|t| Trigger::by_name(&t))
            .unwrap_or(Trigger::None);
        b.trigger_arg = trigger_arg.unwrap_or_default();
        b.name = match name {
            Some(n) if !n.trim().is_empty() => n,
            _ => format!("Workflow {}", index + 1),
        };
        Some(b)
    }

    /// Gson's form: name, key, mode, steps, trigger, triggerArg.
    pub fn to_json(&self) -> Json {
        let mut o = Obj::new();
        o.insert("name", Json::str(&self.name));
        o.insert("key", Json::Int(self.key.into()));
        if let Some(m) = self.mode {
            o.insert(
                "mode",
                Json::str(if m == Mode::Hold { "HOLD" } else { "PRESS" }),
            );
        }
        o.insert(
            "steps",
            Json::Arr(self.steps.iter().map(|s| Json::str(s)).collect()),
        );
        o.insert("trigger", Json::str(self.trigger.name()));
        o.insert("triggerArg", Json::str(&self.trigger_arg));
        Json::Obj(o)
    }
}

/// Java's `Config.parseBindings`: None if an entry has wrong types.
pub fn parse_bindings(a: &[Json]) -> Option<Vec<Binding>> {
    a.iter()
        .enumerate()
        .map(|(i, j)| Binding::from_json(j, i))
        .collect()
}

/// Java's `byName`: trimmed, case-insensitive.
pub fn by_name<'a>(list: &'a [Binding], name: &str) -> Option<&'a Binding> {
    let name = name.trim();
    list.iter()
        .find(|b| b.name.to_lowercase() == name.to_lowercase())
}

// ------------------------------------------------------------ host side

/// Simulated player input. glue.rs implements it on the game; the defaults
/// do nothing (tests). docs/HORIZON.md lists the Horizon sources.
#[allow(unused_variables)]
pub trait Input {
    /// Holds or releases a key mapping (one of `MOVE_KEYS`).
    fn set_key(&mut self, key: &str, down: bool) {}
    /// One attack (left) or use (right) click at the crosshair.
    fn click(&mut self, attack: bool) {}
    /// Player yaw and pitch.
    fn rotation(&self) -> (f32, f32) {
        (0.0, 0.0)
    }
    fn set_rotation(&mut self, yaw: f32, pitch: f32) {}
    /// Eye position.
    fn eye(&self) -> [f64; 3] {
        [0.0; 3]
    }
    /// Feet position.
    fn position(&self) -> [f64; 3] {
        [0.0; 3]
    }
    /// Selects hotbar slot 0-8.
    fn select_slot(&mut self, slot: usize) {}
    /// Closes the open screen (a container: `closeContainer`).
    fn close_screen(&mut self) {}
    fn open_inventory(&mut self) {}
    /// Trading with villagers (`trade`/`villager` steps); None = not possible.
    fn trader(&mut self) -> Option<&mut dyn Trader> {
        None
    }
}

/// What the runner needs from the game. core.rs implements it on the host.
pub trait Env {
    fn beta(&self) -> bool;
    fn curseforge(&self) -> bool;
    fn safe_blocking(&mut self) -> bool;
    fn screen_open(&mut self) -> bool;
    /// `Conditions.test(raw)` incl. the unknown-condition hint.
    fn condition(&mut self, raw: &str) -> bool;
    fn action_bar(&mut self, text: &str);
    /// The non-input immediate steps (toggle, set, chat, …); `revert` =
    /// undo of a HOLD workflow.
    fn step(&mut self, raw: &str, revert: bool);
    /// The steps of the workflow named so (`run` step).
    fn workflow(&mut self, name: &str) -> Option<Vec<String>>;
    /// The player's input; None without a player.
    fn input(&mut self) -> Option<&mut dyn Input>;
    /// Java's `Trace.log` of a known step that runs (`blocked`: skipped by Safe Mode).
    fn trace(&mut self, _step: &str, _blocked: bool) {}
}

/// A step that runs over several ticks.
pub trait Action {
    /// True when finished.
    fn tick(&mut self, input: &mut dyn Input) -> bool;
    fn stop(&mut self, input: &mut dyn Input);
    /// Keeps running while a screen is open; movement pauses instead.
    fn in_screen(&self) -> bool {
        false
    }
}

/// Holds keys for a number of ticks; optional clicks at the start.
pub struct HoldKey {
    keys: Vec<&'static str>,
    left: i32,
    click_attack: bool,
    click_use: bool,
    started: bool,
}

impl Action for HoldKey {
    fn tick(&mut self, input: &mut dyn Input) -> bool {
        if !self.started {
            if self.click_attack {
                input.click(true);
            }
            if self.click_use {
                input.click(false);
            }
        }
        self.started = true;
        for k in &self.keys {
            input.set_key(k, true);
        }
        let done = self.left <= 0;
        self.left -= 1;
        done
    }
    fn stop(&mut self, input: &mut dyn Input) {
        for k in &self.keys {
            input.set_key(k, false);
        }
    }
}

/// Walks until the horizontal distance is reached or a timeout hits.
pub struct WalkBlocks {
    key: &'static str,
    blocks: f64,
    start: [f64; 3],
    last: f64,
    timeout: i32,
}

impl Action for WalkBlocks {
    fn tick(&mut self, input: &mut dyn Input) -> bool {
        let p = input.position();
        let (dx, dz) = (p[0] - self.start[0], p[2] - self.start[2]);
        let dist = (dx * dx + dz * dz).sqrt();
        let speed = dist - self.last;
        self.last = dist;
        // on the ground the player coasts ~1.2x the last tick's speed
        let timed_out = self.timeout <= 0;
        self.timeout -= 1;
        if dist + speed * 1.2 >= self.blocks - 0.05 || timed_out {
            return true;
        }
        input.set_key(self.key, true);
        false
    }
    fn stop(&mut self, input: &mut dyn Input) {
        input.set_key(self.key, false);
    }
}

fn hold_key(keys: Vec<&'static str>, ticks: i32, attack: bool, use_: bool) -> Box<dyn Action> {
    Box::new(HoldKey {
        keys,
        left: ticks,
        click_attack: attack,
        click_use: use_,
        started: false,
    })
}

/// Java's `action(type, arg)` for the actions this port has. `open` of a
/// block/entity, `craft` and `camera_path` need the game (Interact, recipe
/// book, camera paths) and return None until those exist, so the step does
/// nothing. `trade`/`villager` need a `Trader`.
fn action(id: &str, arg: &str, input: &mut dyn Input) -> Option<Box<dyn Action>> {
    match id {
        // Java passes Boolean.FALSE here, which its constructor reads as "click
        // use at the start": a right click with every jump. Deliberate fix: no click.
        "jump" => Some(hold_key(vec!["jump"], 2, false, false)),
        "hold" => {
            let h = parse_hold(arg);
            let k = move_key(&h.key)?;
            let t = ticks(h.seconds);
            (t > 0).then(|| hold_key(vec![k], t, k == "attack", k == "use"))
        }
        "move" => {
            let m = parse_move(arg);
            let t = ticks(m.seconds);
            let (a, u) = (m.keys.contains(&"attack"), m.keys.contains(&"use"));
            (t > 0).then(|| hold_key(m.keys, t, a, u))
        }
        "walk" => {
            let w = parse_walk(arg);
            if w.amount <= 0.0 {
                return None;
            }
            Some(if w.seconds {
                hold_key(vec![w.dir], ticks(w.amount), false, false)
            } else {
                Box::new(WalkBlocks {
                    key: w.dir,
                    blocks: w.amount,
                    start: input.position(),
                    last: 0.0,
                    timeout: (w.amount * 20.0) as i32 + 40,
                })
            })
        }
        "trade" | "villager" => {
            input.trader()?;
            trading::action(id, arg)
        }
        "open" => {
            let what = jtrim(arg).to_lowercase();
            if what.is_empty() || what == "inventory" || what == "inventar" {
                input.open_inventory();
            }
            None
        }
        _ => None,
    }
}

/// Java's `look()`: yaw the short way when absolute.
fn look(input: &mut dyn Input, arg: &str, relative: bool) {
    let (cy, cp) = input.rotation();
    let (y, p) = parse_look(arg, relative, cy, cp, Some(input.eye()));
    let yaw = if y.is_nan() {
        cy
    } else if relative {
        y
    } else {
        cy + (((y - cy) % 360.0 + 540.0) % 360.0 - 180.0)
    };
    input.set_rotation(yaw, if p.is_nan() { cp } else { p });
}

// ------------------------------------------------------------ runner

/// Per-tick limits so an endless loop without waits can't freeze the game.
pub const STEPS_PER_TICK: i32 = 10000;
pub const LOOPS_PER_TICK: i32 = 100;
/// A `run` step starts another workflow only below this depth.
pub const MAX_DEPTH: u32 = 8;

/// One running workflow.
struct Runner {
    steps: Vec<String>,
    jumps: Vec<i32>,
    depth: u32,
    owner: u64,
    /// (opener, runs left)
    loops: Vec<(usize, i32)>,
    index: usize,
    wait: i32,
    action: Option<Box<dyn Action>>,
}

/// The running workflows (Java's `RUNNING`), in start order.
#[derive(Default)]
pub struct Runners {
    list: Vec<Runner>,
}

impl Runners {
    /// Java's `run(b)`: a new runner, its steps up to the first wait or
    /// multi-tick action run right away.
    pub fn run(&mut self, steps: &[String], owner: u64, env: &mut dyn Env) {
        self.start(steps.to_vec(), 0, owner, env);
    }

    fn start(&mut self, steps: Vec<String>, depth: u32, owner: u64, env: &mut dyn Env) {
        let jumps = blocks(&steps).0;
        let r = Runner {
            steps,
            jumps,
            depth,
            owner,
            loops: Vec::new(),
            index: 0,
            wait: 0,
            action: None,
        };
        self.advance(r, env);
    }

    pub fn running(&self, owner: u64) -> bool {
        self.list.iter().any(|r| r.owner == owner)
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Takes the runners started meanwhile (by a nested `/ot run`) over.
    pub fn absorb(&mut self, other: Runners) {
        self.list.extend(other.list);
    }

    /// Stops every run of the owner, also those its `run` steps started.
    pub fn stop(&mut self, owner: u64, env: &mut dyn Env) {
        let mut keep = Vec::new();
        for mut r in std::mem::take(&mut self.list) {
            if r.owner != owner {
                keep.push(r);
            } else if let (Some(a), Some(input)) = (r.action.as_mut(), env.input()) {
                a.stop(input);
            }
        }
        self.list = keep;
    }

    pub fn stop_all(&mut self, env: &mut dyn Env) {
        for mut r in std::mem::take(&mut self.list) {
            if let (Some(a), Some(input)) = (r.action.as_mut(), env.input()) {
                a.stop(input);
            }
        }
    }

    /// Once per client tick.
    pub fn tick(&mut self, env: &mut dyn Env) {
        for mut r in std::mem::take(&mut self.list) {
            if let Some(mut a) = r.action.take() {
                // input actions pause while a screen is open
                if env.screen_open() && !a.in_screen() {
                    r.action = Some(a);
                    self.list.push(r);
                    continue;
                }
                let done = match env.input() {
                    Some(input) => {
                        let done = a.tick(input);
                        if done {
                            a.stop(input);
                        }
                        done
                    }
                    // the player is gone
                    None => true,
                };
                if done {
                    self.advance(r, env);
                } else {
                    r.action = Some(a);
                    self.list.push(r);
                }
            } else if r.wait > 1 {
                r.wait -= 1;
                self.list.push(r);
            } else {
                r.wait = 0;
                self.advance(r, env);
            }
        }
    }

    fn advance(&mut self, mut r: Runner, env: &mut dyn Env) {
        let n = r.steps.len();
        let (mut i, mut budget, mut loop_backs) = (r.index, STEPS_PER_TICK, 0);
        let (beta, curseforge) = (env.beta(), env.curseforge());
        let after = |r: &Runner, i: usize| n.min((r.jumps[i] + 1).max(0) as usize);
        while i <= n {
            let out = budget <= 0;
            budget -= 1;
            if out || loop_backs >= LOOPS_PER_TICK {
                // continue on the next tick
                r.index = i;
                self.list.push(r);
                return;
            }
            if i == n {
                // the end of the list closes unclosed repeat blocks
                let Some(top) = r.loops.last_mut() else {
                    return;
                };
                if r.jumps[top.0] != n as i32 {
                    return;
                }
                top.1 -= 1;
                if top.1 > 0 {
                    i = top.0 + 1;
                    loop_backs += 1;
                } else {
                    r.loops.pop();
                }
                continue;
            }
            let s = r.steps[i].clone();
            let ty = step_of(&s);
            if ty.is_some_and(|t| !t.available(beta, curseforge)) {
                i += 1;
                continue;
            }
            let a = arg(&s);
            match ty.map(|t| t.id) {
                Some("wait") => {
                    let t = ticks(seconds(a, 1.0));
                    if t > 0 {
                        r.index = i + 1;
                        r.wait = t;
                        self.list.push(r);
                        return;
                    }
                    i += 1;
                    continue;
                }
                Some("if") => {
                    i = if env.condition(a) {
                        i + 1
                    } else {
                        after(&r, i)
                    };
                    continue;
                }
                Some("else") => {
                    // reached from the then branch: skip the else branch
                    i = if r.jumps[i] < 0 { i + 1 } else { after(&r, i) };
                    continue;
                }
                Some("repeat") => {
                    let count = repeat_count(a);
                    if count <= 0 {
                        i = after(&r, i);
                    } else {
                        r.loops.push((i, count));
                        i += 1;
                    }
                    continue;
                }
                Some("end") => {
                    let opener = r.jumps[i];
                    if let Some(top) = r.loops.last_mut()
                        && opener >= 0
                        && top.0 == opener as usize
                    {
                        top.1 -= 1;
                        if top.1 > 0 {
                            i = opener as usize + 1;
                            loop_backs += 1;
                            continue;
                        }
                        r.loops.pop();
                    }
                    i += 1;
                    continue;
                }
                Some("stop") => return,
                _ => {}
            }
            if let Some(t) = ty {
                if t.input && env.safe_blocking() {
                    env.trace(&s, true);
                    env.action_bar("Omnitool: movement steps are blocked by Safe Mode");
                    i += 1;
                    continue;
                }
                env.trace(&s, false);
            }
            let screen = env.screen_open();
            let id = ty.map_or("", |t| t.id);
            if let Some(input) = env.input()
                && let Some(mut act) = action(id, a, input)
            {
                // first tick right away, so input steps follow without a gap
                if (!screen || act.in_screen()) && act.tick(input) {
                    act.stop(input);
                    i += 1;
                    continue;
                }
                r.index = i + 1;
                r.action = Some(act);
                self.list.push(r);
                return;
            }
            self.step(&s, r.owner, r.depth, env);
            i += 1;
        }
    }

    /// Java's `step(raw, false, owner, depth)`.
    fn step(&mut self, raw: &str, owner: u64, depth: u32, env: &mut dyn Env) {
        let Some(t) = step_of(raw) else { return };
        let a = arg(raw);
        match t.id {
            "run" => {
                if let Some(steps) = env.workflow(a)
                    && depth < MAX_DEPTH
                {
                    self.start(steps, depth + 1, owner, env);
                }
            }
            "look" | "turn" | "attack" | "use" | "close" | "hotbar" => {
                if let Some(input) = env.input() {
                    input_step(t.id, a, input);
                }
            }
            _ => env.step(raw, false),
        }
    }
}

/// The immediate input steps; only with a player.
fn input_step(id: &str, a: &str, input: &mut dyn Input) {
    match id {
        "look" => look(input, a, false),
        "turn" => look(input, a, true),
        "attack" | "use" => input.click(id == "attack"),
        "close" => input.close_screen(),
        "hotbar" => {
            let slot = parse(a, 1.0) as i64 - 1;
            if (0..9).contains(&slot) {
                input.select_slot(slot as usize);
            }
        }
        _ => {}
    }
}

/// Undo of a HOLD workflow on release: every step backwards, revert mode
/// (only toggle, enable, disable, safemode and freecam_control act).
pub fn revert(steps: &[String], env: &mut dyn Env) {
    for s in steps.iter().rev() {
        if step_of(s).is_some() {
            env.step(s, true);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::omnitool_conditions::{self as conditions, tests as ctests};
    use serde_json::Value;

    pub fn fixtures() -> Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/workflows.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn s(v: &Value) -> &str {
        v.as_str().unwrap()
    }

    fn close(a: f64, b: &Value, tol: f64) -> bool {
        match b.as_f64() {
            Some(b) => (a - b).abs() < tol,
            None => a.is_nan(),
        }
    }

    #[test]
    fn step_parse_and_types() {
        let f = fixtures();
        for c in f["step_parse"].as_array().unwrap() {
            let raw = s(&c["raw"]);
            assert_eq!(verb(raw), s(&c["verb"]), "{raw}");
            assert_eq!(arg(raw), s(&c["arg"]), "{raw}");
            assert_eq!(step_of(raw).map(|t| t.id), c["step"].as_str(), "{raw}");
        }
        let types = f["step_types"].as_array().unwrap();
        assert_eq!(types.len(), STEPS.len());
        for (c, t) in types.iter().zip(&STEPS) {
            assert_eq!(t.id, s(&c["id"]));
            assert_eq!(t.arg.name(), s(&c["arg"]), "{}", t.id);
            assert_eq!(t.input, c["input"].as_bool().unwrap(), "{}", t.id);
            assert_eq!(t.beta, c["beta"].as_bool().unwrap(), "{}", t.id);
        }
        assert_eq!(
            STEPS.iter().filter(|t| t.available(false, false)).count(),
            18
        );
    }

    #[test]
    fn durations_and_repeat_counts() {
        let f = fixtures();
        for c in f["durations"].as_array().unwrap() {
            let secs = seconds(s(&c["raw"]), c["fallback"].as_f64().unwrap());
            assert!(close(secs, &c["seconds"], 1e-6), "{c}");
            assert_eq!(ticks(secs) as i64, c["ticks"].as_i64().unwrap(), "{c}");
        }
        for c in f["repeat_count"].as_array().unwrap() {
            assert_eq!(
                repeat_count(s(&c["arg"])) as i64,
                c["count"].as_i64().unwrap(),
                "{c}"
            );
        }
    }

    #[test]
    fn input_parsers() {
        let f = fixtures();
        for c in f["walk"].as_array().unwrap() {
            let w = parse_walk(s(&c["arg"]));
            assert_eq!(w.dir, s(&c["dir"]), "{c}");
            assert!(close(w.amount, &c["amount"], 1e-6), "{c}");
            assert_eq!(w.seconds, c["seconds"].as_bool().unwrap(), "{c}");
        }
        for c in f["hold"].as_array().unwrap() {
            let h = parse_hold(s(&c["arg"]));
            assert_eq!(h.key, s(&c["key"]), "{c}");
            assert!(close(h.seconds, &c["seconds"], 1e-6), "{c}");
        }
        for c in f["move"].as_array().unwrap() {
            let m = parse_move(s(&c["arg"]));
            let keys: Vec<&str> = c["keys"].as_array().unwrap().iter().map(s).collect();
            assert_eq!(m.keys, keys, "{c}");
            assert!(close(m.seconds, &c["seconds"], 1e-6), "{c}");
        }
        for c in f["look"].as_array().unwrap() {
            let eye = c["eye"].as_array().map(|e| {
                [
                    e[0].as_f64().unwrap(),
                    e[1].as_f64().unwrap(),
                    e[2].as_f64().unwrap(),
                ]
            });
            let (y, p) = parse_look(
                s(&c["arg"]),
                c["relative"].as_bool().unwrap(),
                c["yaw"].as_f64().unwrap() as f32,
                c["pitch"].as_f64().unwrap() as f32,
                eye,
            );
            assert!(close(y.into(), &c["result"][0], 1e-4), "{c} -> {y}");
            assert!(close(p.into(), &c["result"][1], 1e-4), "{c} -> {p}");
        }
        for c in f["craft_parse"].as_array().unwrap() {
            let k = parse_craft(s(&c["arg"]));
            assert_eq!(k.item, s(&c["item"]), "{c}");
            assert_eq!(k.amount as i64, c["amount"].as_i64().unwrap(), "{c}");
            assert_eq!(k.all, c["all"].as_bool().unwrap(), "{c}");
            assert_eq!(k.drop, c["drop"].as_bool().unwrap(), "{c}");
        }
        for c in f["item_match"].as_array().unwrap() {
            let got = item_matches(s(&c["query"]), s(&c["id"]), s(&c["shown"]));
            assert_eq!(got, c["match"].as_bool().unwrap(), "{c}");
        }
        for c in f["drop_option"]["with_drop"].as_array().unwrap() {
            assert_eq!(
                with_drop(s(&c["arg"]), c["drop"].as_bool().unwrap()),
                s(&c["result"]),
                "{c}"
            );
        }
        for c in f["drop_option"]["has_drop"].as_array().unwrap() {
            assert_eq!(
                has_drop(s(&c["arg"])),
                c["result"].as_bool().unwrap(),
                "{c}"
            );
        }
    }

    #[test]
    fn block_structure() {
        for c in fixtures()["blocks"].as_array().unwrap() {
            let steps: Vec<String> = c["steps"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| s(v).to_owned())
                .collect();
            let (jump, level) = blocks(&steps);
            let want = |k: &str| -> Vec<i32> {
                c[k].as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_i64().unwrap() as i32)
                    .collect()
            };
            assert_eq!(jump, want("jump"), "{c}");
            assert_eq!(level, want("level"), "{c}");
        }
    }

    #[test]
    fn binding_migrate() {
        for c in fixtures()["binding_migrate"].as_array().unwrap() {
            let mut input = c["in"].clone();
            let index = input["index"].as_u64().unwrap() as usize;
            input.as_object_mut().unwrap().remove("index");
            let j = Json::parse(&input.to_string()).unwrap();
            let b = Binding::from_json(&j, index).unwrap();
            let out = &c["out"];
            let steps: Vec<String> = out["steps"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| s(v).to_owned())
                .collect();
            assert_eq!(b.steps, steps, "{c}");
            assert_eq!(b.name, s(&out["name"]), "{c}");
            assert_eq!(b.trigger.name(), s(&out["trigger"]), "{c}");
            assert_eq!(b.trigger_arg, s(&out["triggerArg"]), "{c}");
            // `actions` is never written again
            assert!(!b.to_json().as_obj().unwrap().has("actions"));
        }
    }

    #[test]
    fn binding_json_roundtrip() {
        let j = Json::parse(
            r#"{"name":"Bauen","key":66,"mode":"HOLD","steps":["enable:x","wait:2"],"trigger":"CHAT","triggerArg":"hi"}"#,
        )
        .unwrap();
        let b = Binding::from_json(&j, 0).unwrap();
        assert!(b.hold() && b.key == 66 && b.trigger == Trigger::Chat);
        assert_eq!(b.to_json(), j);
        // an unknown mode is not written (Gson: null), acts as PRESS
        let odd =
            Binding::from_json(&Json::parse(r#"{"name":"a","mode":"X"}"#).unwrap(), 0).unwrap();
        assert!(!odd.hold() && !odd.to_json().as_obj().unwrap().has("mode"));
        // wrong types break the section (Gson throws)
        assert!(Binding::from_json(&Json::parse(r#"{"key":"x"}"#).unwrap(), 0).is_none());
        assert!(Binding::from_json(&Json::parse(r#"{"steps":{}}"#).unwrap(), 0).is_none());
    }

    /// An `Env` in memory: the fixture world, modules, a log of the steps.
    pub struct Sim {
        pub beta: bool,
        pub curseforge: bool,
        pub safe: bool,
        pub world: Option<conditions::World>,
        pub modules: Vec<(String, bool)>,
        pub workflows: Vec<(String, Vec<String>)>,
        pub tick: usize,
        pub log: Vec<(usize, String)>,
        pub bar: Vec<String>,
        pub input: Rec,
    }

    /// Input that records what it was asked.
    #[derive(Default)]
    pub struct Rec {
        pub calls: Vec<String>,
        pub pos: [f64; 3],
    }

    impl Input for Rec {
        fn set_key(&mut self, key: &str, down: bool) {
            self.calls.push(format!("{key}={down}"));
        }
        fn click(&mut self, attack: bool) {
            self.calls
                .push(if attack { "click:attack" } else { "click:use" }.into());
        }
        fn position(&self) -> [f64; 3] {
            self.pos
        }
    }

    impl Sim {
        pub fn new(options: &Value) -> Sim {
            let f = fixtures();
            let (world, modules) = ctests::world(&f["conditions_eval"]["world"], &Value::Null);
            Sim {
                beta: options["beta"].as_bool().unwrap_or(true),
                curseforge: options["curseforge"].as_bool().unwrap_or(false),
                safe: options["safe"].as_bool().unwrap_or(false),
                world,
                modules,
                workflows: Vec::new(),
                tick: 0,
                log: Vec::new(),
                bar: Vec::new(),
                input: Rec::default(),
            }
        }

        fn set(&mut self, id: &str, f: impl FnOnce(bool) -> bool) {
            if let Some(m) = self.modules.iter_mut().find(|(m, _)| m == id) {
                m.1 = f(m.1);
            }
        }
    }

    impl Env for Sim {
        fn beta(&self) -> bool {
            self.beta
        }
        fn curseforge(&self) -> bool {
            self.curseforge
        }
        fn safe_blocking(&mut self) -> bool {
            self.safe
        }
        fn screen_open(&mut self) -> bool {
            false
        }
        fn condition(&mut self, raw: &str) -> bool {
            let modules = self.modules.clone();
            conditions::parse(raw).is_some_and(|c| {
                conditions::test(&c, self.world.as_ref(), &|id| {
                    ctests::module_lookup(&modules, id)
                })
            })
        }
        fn action_bar(&mut self, text: &str) {
            self.bar.push(text.to_owned());
        }
        fn step(&mut self, raw: &str, revert: bool) {
            let a = arg(raw).to_owned();
            match verb(raw).as_str() {
                "toggle" => self.set(&a, |on| !on),
                "enable" => self.set(&a, |_| !revert),
                "disable" => self.set(&a, |_| revert),
                _ => {}
            }
        }
        fn workflow(&mut self, name: &str) -> Option<Vec<String>> {
            let name = name.trim().to_lowercase();
            self.workflows
                .iter()
                .find(|(n, _)| n.to_lowercase() == name)
                .map(|(_, s)| s.clone())
        }
        fn input(&mut self) -> Option<&mut dyn Input> {
            Some(&mut self.input)
        }
        fn trace(&mut self, step: &str, blocked: bool) {
            let line = if blocked {
                format!("!blocked {step}")
            } else {
                step.to_owned()
            };
            self.log.push((self.tick, line));
        }
    }

    fn strings(v: &Value) -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|x| s(x).to_owned())
            .collect()
    }

    #[test]
    fn flow_sim() {
        for c in fixtures()["flow_sim"].as_array().unwrap() {
            let mut sim = Sim::new(&c["options"]);
            sim.modules = c["modules"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.as_bool().unwrap()))
                .collect();
            let mut r = Runners::default();
            // run() inside tick 0, before Workflows.tick (a key or trigger start)
            r.run(&strings(&c["steps"]), 1, &mut sim);
            for t in 0..c["ticks"].as_u64().unwrap() as usize {
                sim.tick = t;
                r.tick(&mut sim);
            }
            let want: Vec<(usize, String)> = c["log"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| (l[0].as_u64().unwrap() as usize, s(&l[1]).to_owned()))
                .collect();
            assert_eq!(sim.log, want, "{}", c["name"]);
            for (k, v) in c["modules"].as_object().unwrap() {
                assert_eq!(
                    ctests::module_lookup(&sim.modules, k),
                    v.as_bool(),
                    "{} {k}",
                    c["name"]
                );
            }
        }
    }

    #[test]
    fn hold_revert() {
        for c in fixtures()["hold_revert"].as_array().unwrap() {
            let mut sim = Sim::new(&Value::Null);
            let init = c["initial"].as_object().unwrap();
            sim.modules = init
                .iter()
                .map(|(k, v)| (k.clone(), v.as_bool().unwrap()))
                .collect();
            let steps = strings(&c["steps"]);
            let mut r = Runners::default();
            r.run(&steps, 1, &mut sim);
            let check = |sim: &Sim, k: &str| {
                for (m, v) in c[k].as_object().unwrap() {
                    assert_eq!(
                        ctests::module_lookup(&sim.modules, m),
                        v.as_bool(),
                        "{} {k} {m}",
                        c["name"]
                    );
                }
            };
            check(&sim, "after_run");
            revert(&steps, &mut sim);
            check(&sim, "after_revert");
        }
    }

    #[test]
    fn run_depth_owner_and_stop() {
        let mut sim = Sim::new(&Value::Null);
        sim.workflows
            .push(("Self".into(), vec!["chat:x".into(), "run: self".into()]));
        let mut r = Runners::default();
        r.run(&["run:SELF".into()], 7, &mut sim);
        // the top level (depth 0) starts depth 1..8: 8 runs of "chat:x"
        assert_eq!(sim.log.iter().filter(|l| l.1 == "chat:x").count(), 8);
        sim.workflows
            .push(("Wait".into(), vec!["wait:1".into(), "chat:late".into()]));
        r.run(&["run:wait".into()], 9, &mut sim);
        assert!(r.running(9) && !r.running(7));
        r.stop(9, &mut sim);
        assert!(r.is_empty());
    }

    #[test]
    fn hold_key_and_jump_timing() {
        let mut sim = Sim::new(&Value::Null);
        let mut r = Runners::default();
        r.run(&["jump".into(), "hold:attack 0.1".into()], 1, &mut sim);
        // jump: 2 ticks held plus the first call (no use click: deliberate fix)
        assert_eq!(sim.input.calls, ["jump=true"]);
        r.tick(&mut sim);
        r.tick(&mut sim);
        assert_eq!(
            sim.input.calls,
            [
                "jump=true",
                "jump=true",
                "jump=true",
                "jump=false",
                "click:attack",
                "attack=true"
            ]
        );
        r.tick(&mut sim);
        r.tick(&mut sim);
        assert!(r.is_empty());
        assert_eq!(sim.input.calls.last().unwrap(), "attack=false");
    }

    #[test]
    fn walk_blocks_stops_at_distance() {
        let mut sim = Sim::new(&Value::Null);
        let mut r = Runners::default();
        r.run(&["walk:forward 1".into(), "chat:done".into()], 1, &mut sim);
        assert_eq!(sim.input.calls, ["forward=true"]);
        for z in [0.3, 0.6] {
            sim.input.pos = [0.0, 0.0, z];
            r.tick(&mut sim);
        }
        // 0.6 + 0.3 * 1.2 >= 0.95
        assert_eq!(sim.input.calls.last().unwrap(), "forward=false");
        assert_eq!(sim.log.last().unwrap().1, "chat:done");
    }
}
