// Omni Overlay entries (docs/spec/omnitool/omni-overlay.md, section 3.3):
// the game data an entry reads (`OverlayInfo`, filled by the host; a field
// the host can't fill stays None/empty and its entry shows nothing) and the
// exact text of all 32 entries. Pure logic, checked against
// tests/fixtures/omni-overlay.json. Added to the game as
// `game::omnitool_overlay_entries`; also a module of this crate.

use super::omnitool_core::Player;
use super::omnitool_lang as lang;

/// Server TPS (Beta entry `tps`).
#[derive(Clone, Debug, PartialEq)]
pub struct TpsInfo {
    pub tps: f64,
    pub lagging: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WaypointInfo {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// Full dimension id (`minecraft:overworld`); `None` entries are skipped.
    pub dim: Option<String>,
}

/// What `spawn_here` needs from the world around the player's block.
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnInfo {
    /// Valid spawn floor below and an empty spawn block (the zombie test).
    pub spot: bool,
    pub block_light: i32,
    pub sky_light: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionInfo {
    pub play_seconds: i64,
    pub mined: i64,
    pub placed: i64,
    pub walked: f64,
    pub flown: f64,
    pub deaths: i64,
    pub kills: i64,
}

/// One blockstate property.
#[derive(Clone, Debug, PartialEq)]
pub struct Prop {
    pub name: String,
    /// The serialized value (`true`, `north`, `7`).
    pub value: String,
    /// Int properties: the value and the largest possible value.
    pub int: Option<(i32, i32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockInfo {
    pub pos: [i32; 3],
    /// Registry id with namespace.
    pub id: String,
    /// In `state.getProperties()` order.
    pub props: Vec<Prop>,
    /// `getBestNeighborSignal` at the block.
    pub neighbor_signal: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntityInfo {
    /// Registry id with namespace.
    pub type_id: String,
    /// Health and max health of a LivingEntity.
    pub health: Option<(f32, f32)>,
    pub movement_speed: f64,
    pub jump_strength: f64,
    /// Villager and zombie villager: profession id and level.
    pub villager: Option<(String, i32)>,
}

/// The per-frame snapshot the host gives (Java's `Ctx` reads these from the
/// game). `Default` = nothing known.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OverlayInfo {
    pub fps: Option<u32>,
    /// Used and max memory in MB.
    pub memory_mb: Option<(u64, u64)>,
    /// None in single player.
    pub ping: Option<i32>,
    /// None while the `server_tps` module is off or the TPS unknown.
    pub tps: Option<TpsInfo>,
    /// `ServerTps.warning()`.
    pub lag_warning: Option<String>,
    pub waypoints: Vec<WaypointInfo>,
    pub biome: Option<String>,
    /// Block and sky light at the player's block.
    pub light: Option<(i32, i32)>,
    pub day_time: Option<i64>,
    pub slime_seed: Option<i64>,
    pub spawn: Option<SpawnInfo>,
    pub session: Option<SessionInfo>,
    pub creative_or_spectator: bool,
    pub block: Option<BlockInfo>,
    pub entity: Option<EntityInfo>,
    /// The ridden entity.
    pub vehicle: Option<EntityInfo>,
    /// The block of the bed the player sleeps in.
    pub sleeping_pos: Option<[i32; 3]>,
    /// Vanilla's F3 screen is open.
    pub vanilla_debug: bool,
}

pub use super::omnitool_overlay_layouts::Bed;

/// Java's `Ctx`.
pub struct Ctx<'a> {
    pub info: &'a OverlayInfo,
    pub player: &'a Player,
    pub german: bool,
    pub preview: bool,
    /// `OmniOverlay.speed()`.
    pub speed: f64,
    /// `restTicks()`, -1 = unknown.
    pub rest: i64,
    pub bed: Option<&'a Bed>,
}

/// Alias, so the module can name the text function type.
#[allow(dead_code)]
pub type TextFn = fn(&Ctx) -> Option<String>;

impl Ctx<'_> {
    pub fn tr(&self, s: &str) -> String {
        lang::tr(self.german, s).to_owned()
    }
    fn label(&self, s: &str) -> String {
        format!("{}: ", self.tr(s))
    }
    /// `pos` = `player.blockPosition()`.
    pub fn pos(&self) -> [i32; 3] {
        [
            self.player.x.floor() as i32,
            self.player.y.floor() as i32,
            self.player.z.floor() as i32,
        ]
    }
    /// The short dimension path.
    pub fn dim(&self) -> String {
        dim_short(&self.player.dimension)
    }
    pub fn overworld(&self) -> bool {
        self.dim() == "overworld"
    }
}

/// Java's `Ctx.dim`: everything after the last `:`, without `]`.
pub fn dim_short(full: &str) -> String {
    let s = full.rsplit(':').next().unwrap_or(full);
    s.replace(']', "").trim().to_owned()
}

/// `duration(s)`.
pub fn duration(s: i64) -> String {
    let (h, m, sec) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}

/// `distance(b)`.
pub fn distance(b: f64) -> String {
    if b >= 1000.0 {
        format!("{:.2} km", b / 1000.0)
    } else {
        format!("{} m", b as i64)
    }
}

const MOON: [&str; 8] = [
    "Full moon",
    "Waning gibbous",
    "Third quarter",
    "Waning crescent",
    "New moon",
    "Waxing crescent",
    "First quarter",
    "Waxing gibbous",
];

pub fn time_text(c: &Ctx, day_time: i64) -> String {
    let ticks = day_time % 24000;
    let hh = (ticks / 1000 + 6) % 24;
    let mm = ticks % 1000 * 60 / 1000;
    format!(
        "{}{hh:02}:{mm:02} ({} {})",
        c.label("Time"),
        c.tr("day"),
        day_time / 24000 + 1
    )
}

pub fn moon_phase(day_time: i64) -> &'static str {
    MOON[(day_time / 24000).rem_euclid(8) as usize]
}

pub fn facing_text(c: &Ctx, dir: &str, yaw: f32, pitch: f32) -> String {
    let axis = match dir {
        "north" => "-Z",
        "south" => "+Z",
        "east" => "+X",
        _ => "-X",
    };
    let yaw = ((yaw % 360.0) + 360.0) % 360.0;
    format!(
        "{}{} ({axis}) ({yaw:.1} / {pitch:.1})",
        c.label("Facing"),
        c.tr(dir)
    )
}

/// `Direction.fromYRot`: 0 south, 90 west, 180 north, 270 east.
pub fn direction(yaw: f32) -> &'static str {
    match ((yaw / 90.0 + 0.5).floor() as i64).rem_euclid(4) {
        0 => "south",
        1 => "west",
        2 => "north",
        _ => "east",
    }
}

pub fn nether_text(c: &Ctx, dim: &str, x: i32, z: i32) -> Option<String> {
    match dim {
        "the_nether" => Some(format!("{}{} {}", c.label("Overworld"), x * 8, z * 8)),
        "overworld" => Some(format!(
            "{}{} {}",
            c.label("Nether"),
            x.div_euclid(8),
            z.div_euclid(8)
        )),
        _ => None,
    }
}

pub fn phantoms_text(c: &Ctx, rest: i64) -> Option<String> {
    if rest < 0 {
        return None;
    }
    Some(if rest >= 72000 {
        format!("{}{}", c.label("Phantoms"), c.tr("can spawn (sleep!)"))
    } else {
        format!(
            "{}{} {}",
            c.label("Phantoms"),
            c.tr("in"),
            duration((72000 - rest) / 20)
        )
    })
}

/// `Mobs can spawn here` result.
pub fn spawn_text(c: &Ctx, s: &SpawnInfo) -> String {
    use super::omnitool_light_overlay::{SpawnKind, spawn_kind};
    // the spawn-light rule shared with Light Overlay (world-render.md §1.4)
    let answer = if !s.spot {
        "no"
    } else {
        match spawn_kind(
            s.block_light,
            s.sky_light,
            c.dim() == "the_nether",
            c.overworld(),
        ) {
            SpawnKind::Lit => "no (lit)",
            SpawnKind::Night => "only at night",
            SpawnKind::Spawn => "yes",
        }
    };
    format!("{}{}", c.label("Mobs can spawn here"), c.tr(answer))
}

pub fn crop_text(c: &Ctx, age: i32, max: i32) -> Option<String> {
    if max == 0 {
        return None;
    }
    Some(format!(
        "{}{}% ({age}/{max})",
        c.label("Growth"),
        age * 100 / max
    ))
}

pub fn fill_text(c: &Ctx, block_id: &str, p: &Prop) -> Option<String> {
    let (a, max) = p.int?;
    let counts = p.name == "honey_level"
        || (p.name == "level" && (block_id.contains("composter") || block_id.contains("cauldron")));
    if !counts {
        return None;
    }
    let ready = if a >= max {
        format!("  {}", c.tr("(ready)"))
    } else {
        String::new()
    };
    Some(format!("{}{a}/{max}{ready}", c.label("Fill level")))
}

pub fn horse_text(c: &Ctx, e: &EntityInfo) -> String {
    let s = e.jump_strength;
    let jump = -0.1817584952 * s * s * s + 3.689713992 * s * s + 2.128599134 * s - 0.343930367;
    let max = e.health.map_or(0.0, |h| h.1);
    format!(
        "{}{:.2} b/s\n{}{:.2} blocks\n{}{:.0} ({:.1} hearts)",
        c.label("Horse speed"),
        e.movement_speed * 42.16,
        c.label("Horse jump"),
        jump,
        c.label("Horse health"),
        max,
        max / 2.0
    )
}

pub fn is_horse_type(e: &EntityInfo) -> bool {
    e.health.is_some()
        && ["horse", "donkey", "mule", "llama", "camel"]
            .iter()
            .any(|s| e.type_id.ends_with(s))
}

fn bare(id: &str) -> String {
    id.strip_prefix("minecraft:").unwrap_or(id).to_owned()
}

/// Java's `Entries.text` for entry `id`.
pub fn text(id: &str, c: &Ctx) -> Option<String> {
    let i = c.info;
    let [x, y, z] = c.pos();
    let p = c.player;
    match id {
        "fps" => Some(format!("FPS: {}", i.fps?)),
        "memory" => {
            let (used, max) = i.memory_mb?;
            let pct = (used * 100).checked_div(max).unwrap_or(0);
            Some(format!("{}{pct}% {used}/{max} MB", c.label("Memory")))
        }
        "ping" => Some(format!("{}{} ms", c.label("Ping"), i.ping?)),
        "tps" => {
            let t = i.tps.as_ref().filter(|t| t.tps >= 0.0);
            let (tps, lagging) = match t {
                Some(t) => (t.tps, t.lagging),
                None if c.preview => (20.0, false),
                None => return None,
            };
            let lag = if lagging {
                format!("  {}", c.tr("(lag!)"))
            } else {
                String::new()
            };
            Some(format!("{}{tps:.1}{lag}", c.label("TPS")))
        }
        "lag" => match &i.lag_warning {
            Some(w) => Some(w.clone()),
            None if c.preview => Some(format!("{} (12.0 TPS)", c.tr("Server lagging"))),
            None => None,
        },
        "coords" => Some(format!("XYZ: {:.3} / {:.3} / {:.3}", p.x, p.y, p.z)),
        "block" => Some(format!("{}{x} {y} {z}", c.label("Block"))),
        "chunk" => Some(format!(
            "{}{} {} {} {} {} {} {}",
            c.label("Chunk"),
            x & 15,
            y & 15,
            z & 15,
            c.tr("in"),
            x >> 4,
            y >> 4,
            z >> 4
        )),
        "facing" => Some(facing_text(c, direction(p.yaw), p.yaw, p.pitch)),
        "nether" => nether_text(c, &c.dim(), x, z),
        "speed" => Some(format!("{}{:.1} b/s", c.label("Speed"), c.speed)),
        "dimension" => Some(format!("{}{}", c.label("Dimension"), c.dim())),
        "waypoint" => {
            // Both sides are compared as short paths; waypoints without a
            // dimension are skipped.
            let dim = c.dim();
            let (cx, cz) = (p.x, p.z);
            i.waypoints
                .iter()
                .filter(|w| w.dim.as_deref().is_some_and(|d| dim_short(d) == dim))
                .map(|w| {
                    let d = ((w.x as f64 + 0.5 - cx).powi(2)
                        + (w.y as f64 - p.y).powi(2)
                        + (w.z as f64 + 0.5 - cz).powi(2))
                    .sqrt();
                    (d, w)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(d, w)| format!("{}{}  {} m", c.label("Waypoint"), w.name, d as i64))
        }
        "bed" => {
            let b = c.bed.filter(|b| b.dim == c.dim())?;
            let d = ((b.x as f64 + 0.5 - p.x).powi(2)
                + (b.y as f64 - p.y).powi(2)
                + (b.z as f64 + 0.5 - p.z).powi(2))
            .sqrt();
            Some(format!(
                "{}{} m  ({} {} {})",
                c.label("Bed"),
                d as i64,
                b.x,
                b.y,
                b.z
            ))
        }
        "biome" => Some(format!("{}{}", c.label("Biome"), bare(i.biome.as_deref()?))),
        "light" => {
            let (b, s) = i.light?;
            Some(format!(
                "{}{b} {}, {s} {}",
                c.label("Light"),
                c.tr("block"),
                c.tr("sky")
            ))
        }
        "time" => Some(time_text(c, i.day_time?)),
        "moon" => {
            let t = i.day_time?;
            c.overworld()
                .then(|| format!("{}{}", c.label("Moon"), c.tr(moon_phase(t))))
        }
        "phantoms" => {
            if !c.overworld() || i.creative_or_spectator {
                return None;
            }
            phantoms_text(c, c.rest)
        }
        "slime" => {
            let seed = i.slime_seed?;
            if !c.overworld() {
                return None;
            }
            let yes = is_slime_chunk(seed, x >> 4, z >> 4);
            Some(format!(
                "{}{}",
                c.label("Slime chunk"),
                c.tr(if yes { "yes" } else { "no" })
            ))
        }
        "spawn_here" => Some(spawn_text(c, i.spawn.as_ref()?)),
        "session_time" => {
            let s = i.session.as_ref()?;
            Some(format!(
                "{}{}",
                c.label("Playtime"),
                duration(s.play_seconds)
            ))
        }
        "session_blocks" => {
            let s = i.session.as_ref()?;
            Some(format!(
                "{}{}  {}: {}",
                c.label("Blocks mined"),
                s.mined,
                c.tr("placed"),
                s.placed
            ))
        }
        "session_distance" => {
            let s = i.session.as_ref()?;
            Some(format!(
                "{}{}  {}: {}",
                c.label("Walked"),
                distance(s.walked),
                c.tr("flown"),
                distance(s.flown)
            ))
        }
        "session_combat" => {
            let s = i.session.as_ref()?;
            Some(format!(
                "{}{}  {}: {}",
                c.label("Deaths"),
                s.deaths,
                c.tr("mobs killed"),
                s.kills
            ))
        }
        "target_block" => {
            let b = i.block.as_ref()?;
            let mut out = format!(
                "{}{}, {}, {}\n{}",
                c.label("Targeted block"),
                b.pos[0],
                b.pos[1],
                b.pos[2],
                b.id
            );
            for pr in &b.props {
                out.push_str(&format!("\n{}: {}", pr.name, pr.value));
            }
            Some(out)
        }
        "target_entity" => {
            let e = i.entity.as_ref()?;
            let mut out = format!("{}{}", c.label("Targeted entity"), e.type_id);
            if let Some((hp, max)) = e.health {
                out.push_str(&format!("\n{}{hp:.1} / {max:.1}", c.label("Health")));
            }
            Some(out)
        }
        "horse" => {
            let e = i
                .vehicle
                .as_ref()
                .filter(|e| is_horse_type(e))
                .or(i.entity.as_ref().filter(|e| is_horse_type(e)))?;
            Some(horse_text(c, e))
        }
        "villager" => {
            let (prof, level) = i.entity.as_ref()?.villager.as_ref()?;
            Some(format!(
                "{}{} ({} {level})",
                c.label("Villager"),
                prof.replace("minecraft:", ""),
                c.tr("level")
            ))
        }
        "crop" => {
            let age = i.block.as_ref()?.props.iter().find(|p| p.name == "age")?;
            let (a, max) = age.int?;
            crop_text(c, a, max)
        }
        "fill" => {
            let b = i.block.as_ref()?;
            let prop = b
                .props
                .iter()
                .find(|p| p.name == "honey_level")
                .or_else(|| b.props.iter().find(|p| p.name == "level"))?;
            fill_text(c, &b.id, prop)
        }
        "redstone" => {
            let b = i.block.as_ref()?;
            if let Some(pw) = b
                .props
                .iter()
                .find(|p| p.name == "power")
                .and_then(|p| p.int)
            {
                return Some(format!("{}{}", c.label("Redstone power"), pw.0));
            }
            (b.neighbor_signal > 0)
                .then(|| format!("{}{}", c.label("Redstone input"), b.neighbor_signal))
        }
        _ => None,
    }
}

// ------------------------------------------------------------ host helpers

/// An int property's value and largest value (Java's `IntegerProperty`):
/// `value` and every possible value (`values`) are integers; else None
/// (booleans, enums).
pub fn int_prop<'a>(value: &str, values: impl IntoIterator<Item = &'a str>) -> Option<(i32, i32)> {
    let v: i32 = value.parse().ok()?;
    let mut max = v;
    for other in values {
        max = max.max(other.parse().ok()?);
    }
    Some((v, max))
}

/// A `Key:   123 kB` line of /proc/self/status or /proc/meminfo, in kB.
pub fn proc_kb(text: &str, key: &str) -> Option<u64> {
    text.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Entry `memory` without a JVM (deviation, omni-overlay.md §9.6): used =
/// the process's resident memory (`VmRSS`), max = the machine's memory
/// (`MemTotal`), both in MB (`>> 10` of kB, as Java's `>> 20`).
pub fn memory_mb(status: &str, meminfo: &str) -> Option<(u64, u64)> {
    Some((
        proc_kb(status, "VmRSS")? >> 10,
        proc_kb(meminfo, "MemTotal")? >> 10,
    ))
}

// ------------------------------------------------------------ slime chunks

struct JavaRandom(i64);

impl JavaRandom {
    fn new(seed: i64) -> JavaRandom {
        JavaRandom((seed ^ 0x5DEECE66D) & ((1 << 48) - 1))
    }
    fn next(&mut self, bits: u32) -> i32 {
        self.0 = (self.0.wrapping_mul(0x5DEECE66D).wrapping_add(0xB)) & ((1 << 48) - 1);
        (self.0 >> (48 - bits)) as i32
    }
    fn next_int(&mut self, bound: i32) -> i32 {
        if bound & (bound - 1) == 0 {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % bound;
            if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
                return val;
            }
        }
    }
}

/// The vanilla slime chunk test (Java `Random` seeded from the world seed
/// and the chunk).
pub fn is_slime_chunk(seed: i64, cx: i32, cz: i32) -> bool {
    let s = seed
        .wrapping_add(cx.wrapping_mul(cx).wrapping_mul(0x4c1906) as i64)
        .wrapping_add(cx.wrapping_mul(0x5ac0db) as i64)
        .wrapping_add(cz.wrapping_mul(cz).wrapping_mul(0x4307a7) as i64)
        .wrapping_add(cz.wrapping_mul(0x5f24f) as i64)
        ^ 0x3ad8025f;
    JavaRandom::new(s).next_int(10) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixtures() -> Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/omni-overlay.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn player(dim: &str) -> Player {
        Player {
            x: 10.5,
            y: 64.0,
            z: -3.25,
            yaw: 0.0,
            pitch: 0.0,
            dimension: format!("minecraft:{dim}"),
        }
    }

    fn with_ctx<R>(dim: &str, info: &OverlayInfo, f: impl FnOnce(&Ctx) -> R) -> R {
        let p = player(dim);
        f(&Ctx {
            info,
            player: &p,
            german: false,
            preview: false,
            speed: 0.0,
            rest: -1,
            bed: None,
        })
    }

    fn out(v: &Value) -> Option<String> {
        v.as_str().map(str::to_owned)
    }

    #[test]
    fn time_and_moon() {
        let f = fixtures();
        let info = OverlayInfo::default();
        for c in f["time"].as_array().unwrap() {
            let t = c["in"]["day_time"].as_i64().unwrap();
            assert_eq!(
                Some(with_ctx("overworld", &info, |x| time_text(x, t))),
                out(&c["out"])
            );
        }
        for c in f["moon"].as_array().unwrap() {
            let t = c["in"]["day_time"].as_i64().unwrap();
            assert_eq!(Some(moon_phase(t).to_owned()), out(&c["out"]));
        }
        let i = OverlayInfo {
            day_time: Some(24000),
            ..Default::default()
        };
        let t = with_ctx("overworld", &i, |c| text("moon", c));
        assert_eq!(t.as_deref(), Some("Moon: Waning gibbous"));
        assert_eq!(with_ctx("the_nether", &i, |c| text("moon", c)), None);
    }

    #[test]
    fn duration_and_distance() {
        let f = fixtures();
        for c in f["duration"].as_array().unwrap() {
            assert_eq!(Some(duration(c["in"].as_i64().unwrap())), out(&c["out"]));
        }
        for c in f["distance"].as_array().unwrap() {
            assert_eq!(Some(distance(c["in"].as_f64().unwrap())), out(&c["out"]));
        }
    }

    #[test]
    fn nether_facing() {
        let f = fixtures();
        let info = OverlayInfo::default();
        for c in f["nether"].as_array().unwrap() {
            let i = &c["in"];
            let got = with_ctx("overworld", &info, |x| {
                nether_text(
                    x,
                    i["dim"].as_str().unwrap(),
                    i["x"].as_i64().unwrap() as i32,
                    i["z"].as_i64().unwrap() as i32,
                )
            });
            assert_eq!(got, out(&c["out"]));
        }
        for c in f["facing"].as_array().unwrap() {
            let i = &c["in"];
            let got = with_ctx("overworld", &info, |x| {
                facing_text(
                    x,
                    i["dir"].as_str().unwrap(),
                    i["yaw"].as_f64().unwrap() as f32,
                    i["pitch"].as_f64().unwrap() as f32,
                )
            });
            assert_eq!(Some(got), out(&c["out"]));
        }
        assert_eq!(direction(0.0), "south");
        assert_eq!(direction(90.0), "west");
        assert_eq!(direction(-90.0), "east");
        assert_eq!(direction(180.0), "north");
    }

    #[test]
    fn phantoms_horse_crop_fill() {
        let f = fixtures();
        let info = OverlayInfo::default();
        for c in f["phantoms"].as_array().unwrap() {
            let r = c["in"]["rest"].as_i64().unwrap();
            assert_eq!(
                with_ctx("overworld", &info, |x| phantoms_text(x, r)),
                out(&c["out"])
            );
        }
        for c in f["horse_jump"].as_array().unwrap() {
            let i = &c["in"];
            let e = EntityInfo {
                type_id: "minecraft:horse".into(),
                health: Some((20.0, i["max_health"].as_f64().unwrap() as f32)),
                movement_speed: i["movement_speed"].as_f64().unwrap(),
                jump_strength: i["jump_strength"].as_f64().unwrap(),
                villager: None,
            };
            assert_eq!(
                Some(with_ctx("overworld", &info, |x| horse_text(x, &e))),
                out(&c["out"])
            );
        }
        for c in f["crop"].as_array().unwrap() {
            let i = &c["in"];
            let got = with_ctx("overworld", &info, |x| {
                crop_text(
                    x,
                    i["age"].as_i64().unwrap() as i32,
                    i["max"].as_i64().unwrap() as i32,
                )
            });
            assert_eq!(got, out(&c["out"]));
        }
        for c in f["fill"].as_array().unwrap() {
            let i = &c["in"];
            let p = Prop {
                name: i["prop"].as_str().unwrap().into(),
                value: String::new(),
                int: Some((
                    i["value"].as_i64().unwrap() as i32,
                    i["max"].as_i64().unwrap() as i32,
                )),
            };
            let got = with_ctx("overworld", &info, |x| {
                fill_text(x, i["block"].as_str().unwrap(), &p)
            });
            assert_eq!(got, out(&c["out"]));
        }
    }

    #[test]
    fn simple_entries_and_missing_data() {
        let info = OverlayInfo {
            fps: Some(60),
            ..Default::default()
        };
        let t = |id: &str| with_ctx("overworld", &info, |c| text(id, c));
        assert_eq!(t("fps").as_deref(), Some("FPS: 60"));
        assert_eq!(
            t("coords").as_deref(),
            Some("XYZ: 10.500 / 64.000 / -3.250")
        );
        assert_eq!(t("block").as_deref(), Some("Block: 10 64 -4"));
        assert_eq!(t("chunk").as_deref(), Some("Chunk: 10 0 12 in 0 4 -1"));
        assert_eq!(t("dimension").as_deref(), Some("Dimension: overworld"));
        assert_eq!(t("speed").as_deref(), Some("Speed: 0.0 b/s"));
        for id in [
            "memory",
            "ping",
            "tps",
            "biome",
            "light",
            "time",
            "slime",
            "spawn_here",
            "target_block",
            "villager",
        ] {
            assert_eq!(t(id), None, "{id}");
        }
        assert_eq!(
            dim_short("ResourceKey[minecraft:dimension / mymod:cave]"),
            "cave"
        );
    }

    #[test]
    fn target_and_waypoint() {
        let info = OverlayInfo {
            block: Some(BlockInfo {
                pos: [1, 2, 3],
                id: "minecraft:wheat".into(),
                props: vec![Prop {
                    name: "age".into(),
                    value: "3".into(),
                    int: Some((3, 7)),
                }],
                neighbor_signal: 4,
            }),
            waypoints: vec![
                WaypointInfo {
                    name: "Home".into(),
                    x: 10,
                    y: 64,
                    z: -4,
                    dim: Some("minecraft:overworld".into()),
                },
                WaypointInfo {
                    name: "Far".into(),
                    x: 500,
                    y: 64,
                    z: 0,
                    dim: Some("minecraft:overworld".into()),
                },
                WaypointInfo {
                    name: "Hell".into(),
                    x: 10,
                    y: 64,
                    z: -4,
                    dim: Some("minecraft:the_nether".into()),
                },
            ],
            ..Default::default()
        };
        let t = |id: &str| with_ctx("overworld", &info, |c| text(id, c));
        assert_eq!(
            t("target_block").as_deref(),
            Some("Targeted block: 1, 2, 3\nminecraft:wheat\nage: 3")
        );
        assert_eq!(t("crop").as_deref(), Some("Growth: 42% (3/7)"));
        assert_eq!(t("redstone").as_deref(), Some("Redstone input: 4"));
        assert_eq!(t("waypoint").as_deref(), Some("Waypoint: Home  0 m"));
    }

    #[test]
    fn slime_chunk_known_values() {
        // Seed 0: chunk (0,0) is not a slime chunk; the formula is stable.
        let hits = (-8..8)
            .flat_map(|x| (-8..8).map(move |z| (x, z)))
            .filter(|&(x, z)| is_slime_chunk(0, x, z))
            .count();
        assert!((10..=45).contains(&hits), "{hits} of 256");
    }

    #[test]
    fn int_props() {
        assert_eq!(int_prop("3", ["0", "1", "7", "3"]), Some((3, 7)));
        assert_eq!(int_prop("true", ["true", "false"]), None);
        assert_eq!(int_prop("2", ["2", "north"]), None);
    }

    #[test]
    fn memory_from_proc() {
        let status = "Name:\thorizon\nVmPeak:\t 9000 kB\nVmRSS:\t 1259520 kB\n";
        let meminfo = "MemTotal:       16315392 kB\nMemFree: 1 kB\n";
        assert_eq!(proc_kb(status, "VmRSS"), Some(1259520));
        assert_eq!(memory_mb(status, meminfo), Some((1230, 15933)));
        assert_eq!(memory_mb("", meminfo), None);
    }
}
