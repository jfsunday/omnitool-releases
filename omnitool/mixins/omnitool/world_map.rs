// World Map, phase 1 (docs/spec/omnitool/world-map.md): remembers the colour,
// height and water depth of every column of the chunks the player has seen,
// in region files per world and dimension. No screen yet. The column scan,
// shading, region index and file format are pure logic with tests; the game
// scans chunks through `Host::map_loaded` / `Host::map_scan`. Added to the
// game as `game::omnitool_world_map`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_safe::Server;
use super::omnitool_setting::{Kind, NONE, SettingDef};
use super::omnitool_share as share;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "world_map",
    name: "World Map",
    category: Category::World,
    description: "Remembers every chunk you visit (colour and height per column) for the world map.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "Xaero's World Map",
    key: NONE,
    hold: false,
    settings: &[
        SettingDef::new("update_chunks", "Update known chunks", Kind::Bool(true)),
        SettingDef::new("open_key", "Open map key", Kind::Key(77)),
        SettingDef::new(
            "writing_distance",
            "Writing distance (chunks, -1 = unlimited)",
            Kind::Number {
                default: -1.0,
                min: -1.0,
                max: 32.0,
                step: 1.0,
            },
        ),
        SettingDef::new(
            "max_loaded_regions",
            "Regions kept in memory",
            Kind::Number {
                default: 300.0,
                min: 4.0,
                max: 1000.0,
                step: 4.0,
            },
        ),
    ],
    hud: None,
    tick: Some(tick),
    command: None,
    on_key: Some(on_key),
    hud_right: None,
    on_action: None,
};

/// The lowest block Y; the scan stops above it.
pub const MIN_Y: i32 = -64;
/// Map colour ids of water and lava (`MapColor.WATER`, `FIRE`).
pub const WATER: u8 = 12;
pub const LAVA: u8 = 4;
/// Marks a column the map cuts through (unused in phase 1).
#[allow(dead_code)]
pub const WALL: u8 = 0x80;

/// Block flags the scan reads: 1 water, 2 lava (`FLUID` mask 3), 4 air,
/// 8 sturdy on top.
pub const FLUID: u8 = 3;
pub const STURDY_TOP: u8 = 8;

/// Ticks between writes of dirty regions (30 s).
const FLUSH_TICKS: u64 = 600;
/// A rescan of a changed chunk waits at least this long.
const RESCAN_MS: u64 = 1000;
/// CPU time the scans may take per tick.
const BUDGET_US: u128 = 300;
/// Ticks between looks for due chunks.
const QUEUE_TICKS: u64 = 10;
/// Most bytes a region file may inflate to.
const MAX_REGION_BYTES: usize = 8 << 20;

// ------------------------------------------------------------ columns

/// One chunk: per column (`z * 16 + x`) the map colour, the height of the
/// colour block and the water depth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    pub color: [u8; 256],
    pub height: [i16; 256],
    pub depth: [u8; 256],
}

impl Tile {
    pub fn empty() -> Tile {
        Tile {
            color: [0; 256],
            height: [MIN_Y as i16; 256],
            depth: [0; 256],
        }
    }
}

/// `MapItem.update` for one column: the first block with a map colour going
/// down from `top`, the fluid's colour over a fluid unless the block is
/// sturdy on top, and the fluid's depth. `block(y)` gives (colour, flags).
pub fn scan_column(block: &dyn Fn(i32) -> (u8, u8), top: i32) -> (u8, i16, u8) {
    let mut y = top;
    while y > MIN_Y {
        let (mut color, flags) = block(y);
        if color == 0 {
            y -= 1;
            continue;
        }
        let mut depth = 0u8;
        let fluid = flags & FLUID;
        if fluid != 0 {
            let mut below = y - 1;
            while below > MIN_Y && block(below).1 & FLUID != 0 {
                depth = depth.saturating_add(1);
                below -= 1;
            }
            depth = depth.saturating_add(1);
            if flags & STURDY_TOP == 0 {
                color = if fluid == 2 { LAVA } else { WATER };
            }
        }
        return (color, y as i16, depth);
    }
    (0, MIN_Y as i16, 0)
}

/// The map item's shading at scale 1 (0 darkest .. 3 wall): brighter facing
/// a lower north neighbour, darker facing a higher one, dithered; water by
/// depth.
pub fn brightness(color: u8, height: i16, north: Option<i16>, depth: u8, parity: i32) -> u8 {
    if color & WALL != 0 {
        return 3;
    }
    let parity = f64::from(parity & 1);
    if color == WATER {
        let diff = f64::from(depth) * 0.1 + parity * 0.2;
        return if diff < 0.5 {
            2
        } else if diff > 0.9 {
            0
        } else {
            1
        };
    }
    let north = north.unwrap_or(height);
    let diff = f64::from(height - north) * 4.0 / 5.0 + (parity - 0.5) * 0.4;
    if diff > 0.6 {
        2
    } else if diff < -0.6 {
        0
    } else {
        1
    }
}

/// The shaded colour: `rgb` (0xRRGGBB) times the brightness's multiplier.
pub fn shade(rgb: u32, brightness: u8) -> [u8; 3] {
    let m = [180u32, 220, 255, 135][usize::from(brightness & 3)];
    let ch = |s: u32| ((rgb >> s & 255) * m / 255) as u8;
    [ch(16), ch(8), ch(0)]
}

// ------------------------------------------------------------ regions

/// The region a chunk is in, and its index within it.
pub fn region_of(cx: i32, cz: i32) -> ((i32, i32), usize) {
    ((cx >> 5, cz >> 5), ((cz & 31) * 32 + (cx & 31)) as usize)
}

/// The chunk at `index` of region `r`.
#[allow(dead_code)]
pub fn chunk_of(r: (i32, i32), index: usize) -> (i32, i32) {
    (
        r.0 * 32 + (index & 31) as i32,
        r.1 * 32 + (index >> 5) as i32,
    )
}

pub fn region_file(r: (i32, i32)) -> String {
    format!("r.{}.{}.bin", r.0, r.1)
}

/// A name safe as a folder: anything but `A-Z a-z 0-9 . _ -` becomes `_`;
/// empty or only dots gives `_`.
pub fn sanitize(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.chars().all(|c| c == '.') {
        "_".to_owned()
    } else {
        out
    }
}

/// The folder name of a world: Waypoints' key, sanitized.
pub fn world_folder(server: &Server, world_name: &str) -> String {
    sanitize(&super::omnitool_waypoints::world_key(server, world_name))
}

/// 32 x 32 tiles.
pub struct Region {
    pub tiles: Vec<Option<Box<Tile>>>,
}

impl Region {
    pub fn new() -> Region {
        Region {
            tiles: (0..1024).map(|_| None).collect(),
        }
    }

    /// Puts a tile in; true when it differs from what was there.
    pub fn put(&mut self, index: usize, tile: Tile) -> bool {
        let slot = &mut self.tiles[index];
        if slot.as_deref() == Some(&tile) {
            return false;
        }
        *slot = Some(Box::new(tile));
        true
    }

    /// The file's bytes (gzip over the layout of the spec).
    pub fn encode(&self) -> Vec<u8> {
        let present: Vec<usize> = (0..1024).filter(|&i| self.tiles[i].is_some()).collect();
        let mut raw = Vec::with_capacity(7 + present.len() * 1026);
        raw.extend(b"OTWM");
        raw.push(1);
        raw.extend((present.len() as u16).to_le_bytes());
        for i in present {
            let t = self.tiles[i].as_ref().expect("present");
            raw.extend((i as u16).to_le_bytes());
            raw.extend(t.color);
            for h in t.height {
                raw.extend(h.to_le_bytes());
            }
            raw.extend(t.depth);
        }
        share::gzip(&raw)
    }

    /// None for anything that isn't a valid file.
    pub fn decode(data: &[u8]) -> Option<Region> {
        let raw = share::gunzip(data, MAX_REGION_BYTES)?;
        if raw.len() < 7 || &raw[..4] != b"OTWM" || raw[4] != 1 {
            return None;
        }
        let count = usize::from(u16::from_le_bytes([raw[5], raw[6]]));
        let mut at = 7;
        let mut region = Region::new();
        for _ in 0..count {
            let body = raw.get(at..at + 2 + 256 + 512 + 256)?;
            at += body.len();
            let index = usize::from(u16::from_le_bytes([body[0], body[1]]));
            if index >= 1024 || region.tiles[index].is_some() {
                return None;
            }
            let mut t = Tile::empty();
            t.color.copy_from_slice(&body[2..258]);
            for (i, h) in body[258..770].as_chunks::<2>().0.iter().enumerate() {
                t.height[i] = i16::from_le_bytes([h[0], h[1]]);
            }
            t.depth.copy_from_slice(&body[770..1026]);
            region.tiles[index] = Some(Box::new(t));
        }
        (at == raw.len()).then_some(region)
    }
}

impl Default for Region {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------ store

struct Entry {
    region: Region,
    dirty: bool,
    used: u64,
}

/// The regions of one world and dimension, in memory and on disk.
pub struct Store {
    dir: Option<PathBuf>,
    regions: HashMap<(i32, i32), Entry>,
    clock: u64,
    /// Chunks changed since the screen last asked (`take_touched`); None
    /// after more than the screen's limit (it then redraws everything).
    touched: Option<Vec<(i32, i32)>>,
}

impl Store {
    /// `dir` is the dimension's folder; None keeps nothing on disk.
    pub fn new(dir: Option<PathBuf>) -> Store {
        Store {
            dir,
            regions: HashMap::new(),
            clock: 0,
            touched: Some(Vec::new()),
        }
    }

    fn file(&self, r: (i32, i32)) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(region_file(r)))
    }

    fn load(&self, r: (i32, i32), now_ms: u64) -> Region {
        let Some(file) = self.file(r) else {
            return Region::new();
        };
        let Ok(data) = fs::read(&file) else {
            return Region::new();
        };
        Region::decode(&data).unwrap_or_else(|| {
            let mut broken = file.clone().into_os_string();
            broken.push(format!(".broken-{now_ms}"));
            let _ = fs::rename(&file, broken);
            Region::new()
        })
    }

    fn entry(&mut self, r: (i32, i32), now_ms: u64) -> &mut Entry {
        self.clock += 1;
        if !self.regions.contains_key(&r) {
            let region = self.load(r, now_ms);
            self.regions.insert(
                r,
                Entry {
                    region,
                    dirty: false,
                    used: 0,
                },
            );
        }
        let e = self.regions.get_mut(&r).expect("inserted");
        e.used = self.clock;
        e
    }

    /// The tile of a chunk, if known.
    pub fn tile(&mut self, cx: i32, cz: i32, now_ms: u64) -> Option<&Tile> {
        let (r, i) = region_of(cx, cz);
        self.entry(r, now_ms).region.tiles[i].as_deref()
    }

    /// Merges a scanned tile in (the new one wins); true if it changed.
    pub fn put(&mut self, cx: i32, cz: i32, tile: Tile, now_ms: u64) -> bool {
        let (r, i) = region_of(cx, cz);
        let e = self.entry(r, now_ms);
        let changed = e.region.put(i, tile);
        e.dirty |= changed;
        if changed && let Some(t) = self.touched.as_mut() {
            if t.len() >= super::omnitool_world_map_screen::TOUCH_LIMIT {
                self.touched = None;
            } else {
                t.push((cx, cz));
            }
        }
        changed
    }

    /// The chunks changed since the last call (None: too many to list).
    pub fn take_touched(&mut self) -> Option<Vec<(i32, i32)>> {
        self.touched.replace(Vec::new())
    }

    /// Writes the dirty regions (`.tmp`, then rename); failures leave them
    /// dirty.
    pub fn flush(&mut self) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        if self.regions.values().any(|e| e.dirty) {
            let _ = fs::create_dir_all(&dir);
        }
        for (r, e) in self.regions.iter_mut().filter(|(_, e)| e.dirty) {
            let file = dir.join(region_file(*r));
            let tmp = dir.join(format!("{}.tmp", region_file(*r)));
            if fs::write(&tmp, e.region.encode()).is_ok() && fs::rename(&tmp, &file).is_ok() {
                e.dirty = false;
            }
        }
    }

    /// Drops the least recently used clean regions above `max`.
    pub fn trim(&mut self, max: usize) {
        while self.regions.len() > max {
            let Some(r) = self
                .regions
                .iter()
                .filter(|(_, e)| !e.dirty)
                .min_by_key(|(_, e)| e.used)
                .map(|(r, _)| *r)
            else {
                return;
            };
            self.regions.remove(&r);
        }
    }

    #[allow(dead_code)]
    pub fn loaded(&self) -> usize {
        self.regions.len()
    }
}

// ------------------------------------------------------------ capture

/// What the game has loaded: the world's revision and its chunks.
pub struct Loaded {
    pub revision: u64,
    pub center: (i32, i32),
    pub chunks: Vec<(i32, i32)>,
}

/// Whether a chunk needs a scan: never scanned, or (when updating) changed
/// since and not scanned in the last second.
pub fn due(last: Option<(u64, u64)>, revision: u64, now_ms: u64, update: bool) -> bool {
    match last {
        None => true,
        Some((rev, at)) => update && rev != revision && now_ms.saturating_sub(at) >= RESCAN_MS,
    }
}

/// The due chunks within `distance` (negative = any) of the center, nearest
/// first.
pub fn queue(
    loaded: &Loaded,
    seen: &HashMap<(i32, i32), (u64, u64)>,
    now_ms: u64,
    update: bool,
    distance: i32,
) -> Vec<(i32, i32)> {
    let dist = |c: &(i32, i32)| {
        (c.0 - loaded.center.0)
            .abs()
            .max((c.1 - loaded.center.1).abs())
    };
    let mut out: Vec<(i32, i32)> = loaded
        .chunks
        .iter()
        .copied()
        .filter(|c| distance < 0 || dist(c) <= distance)
        .filter(|c| due(seen.get(c).copied(), loaded.revision, now_ms, update))
        .collect();
    // farthest first, so `pop` takes the nearest
    out.sort_by_key(|c| std::cmp::Reverse((dist(c), c.0, c.1)));
    out
}

#[derive(Default)]
struct State {
    /// Folder of the open store.
    open: Option<PathBuf>,
    store: Option<Store>,
    seen: HashMap<(i32, i32), (u64, u64)>,
    pending: Vec<(i32, i32)>,
    /// The world revision the pending scans were queued at.
    revision: u64,
    ticks: u64,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Writes and closes the open store.
fn close(s: &mut State) {
    if let Some(store) = s.store.as_mut() {
        store.flush();
    }
    *s = State::default();
}

/// The open store (None while World Map is off or there is no world).
pub fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> Option<R> {
    STATE.with_borrow_mut(|s| s.store.as_mut().map(f))
}

/// The open-map key (phase 2): asks the screen to open.
fn on_key(host: &mut dyn Host, key: &str, pressed: bool) {
    if !pressed || !core::active(host, &MODULE) {
        return;
    }
    let code = core::value(host, &MODULE, "open_key").key();
    if core::vanilla_key_name(code).as_deref() == Some(key) {
        super::omnitool_world_map_screen::request();
    }
}

fn tick(host: &mut dyn Host) {
    STATE.with_borrow_mut(|s| step(s, host));
}

fn step(s: &mut State, host: &mut dyn Host) {
    let player = host.player();
    let Some(p) = player.filter(|_| core::active(host, &MODULE)) else {
        close(s);
        return;
    };
    let world_name = host.world().map(|w| w.world_name).unwrap_or_default();
    let dir = host.data_dir().map(|d| {
        d.join("worldmap")
            .join(world_folder(&host.server(), &world_name))
            .join(sanitize(&p.dimension))
    });
    if s.store.is_none() || s.open != dir {
        close(s);
        s.open = dir.clone();
        s.store = Some(Store::new(dir));
    }
    let now = host.now_ms();
    s.ticks += 1;
    let update = core::value(host, &MODULE, "update_chunks").on();
    let distance = core::value(host, &MODULE, "writing_distance").num().round() as i32;
    let max_regions = core::value(host, &MODULE, "max_loaded_regions")
        .num()
        .round() as usize;
    if s.pending.is_empty()
        && s.ticks.is_multiple_of(QUEUE_TICKS)
        && let Some(loaded) = host.map_loaded()
    {
        s.pending = queue(&loaded, &s.seen, now, update, distance);
        s.revision = loaded.revision;
        s.seen
            .retain(|c, _| loaded.chunks.contains(c) || s.pending.contains(c));
    }
    let revision = s.revision;
    let started = std::time::Instant::now();
    while let Some((cx, cz)) = s.pending.pop() {
        if let Some(tile) = host.map_scan(cx, cz) {
            if let Some(store) = s.store.as_mut() {
                store.put(cx, cz, tile, now);
            }
            s.seen.insert((cx, cz), (revision, now));
        }
        if started.elapsed().as_micros() >= BUDGET_US {
            break;
        }
    }
    if let Some(store) = s.store.as_mut() {
        if s.ticks.is_multiple_of(FLUSH_TICKS) {
            store.flush();
        }
        store.trim(max_regions);
    }
}

/// The folder a dimension's regions are in, for the screen of phase 2.
#[allow(dead_code)]
pub fn dimension_dir(data_dir: &Path, server: &Server, world_name: &str, dim: &str) -> PathBuf {
    data_dir
        .join("worldmap")
        .join(world_folder(server, world_name))
        .join(sanitize(dim))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ot-wm-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn tile(seed: u8) -> Tile {
        let mut t = Tile::empty();
        for i in 0..256 {
            t.color[i] = seed.wrapping_add(i as u8) % 60;
            t.height[i] = i as i16 - 64 + i16::from(seed);
            t.depth[i] = (i % 7) as u8;
        }
        t
    }

    #[test]
    fn column_scan() {
        // stone (11) up to y=63, water (12 flagged 1) above down 3 deep
        let block = |y: i32| -> (u8, u8) {
            match y {
                64..=66 => (12, 1),
                -63..=63 => (11, 0),
                _ => (0, 4),
            }
        };
        assert_eq!(scan_column(&block, 100), (WATER, 66, 3));
        // air only
        assert_eq!(scan_column(&|_| (0, 4), 100), (0, -64, 0));
        // lava not sturdy on top keeps lava colour; sturdy block keeps own colour
        assert_eq!(scan_column(&|_| (30, 2), 10), (LAVA, 10, 74));
        assert_eq!(scan_column(&|_| (30, 2 | STURDY_TOP), 10), (30, 10, 74));
    }

    #[test]
    fn shading() {
        assert_eq!(brightness(5, 70, Some(70), 0, 0), 1);
        assert_eq!(brightness(5, 70, Some(60), 0, 1), 2);
        assert_eq!(brightness(5, 60, Some(70), 0, 0), 0);
        assert_eq!(brightness(WATER, 62, None, 2, 0), 2);
        assert_eq!(brightness(WATER, 62, None, 6, 1), 1);
        assert_eq!(brightness(WATER, 62, None, 9, 1), 0);
        assert_eq!(brightness(5 | WALL, 0, None, 0, 0), 3);
        assert_eq!(shade(0xFF8040, 2), [255, 128, 64]);
        assert_eq!(shade(0xFFFFFF, 0), [180, 180, 180]);
    }

    #[test]
    fn regions_index() {
        assert_eq!(region_of(0, 0), ((0, 0), 0));
        assert_eq!(region_of(31, 31), ((0, 0), 1023));
        assert_eq!(region_of(-1, -1), ((-1, -1), 1023));
        assert_eq!(region_of(32, -33), ((1, -2), 31 * 32));
        for (cx, cz) in [(5, 9), (-1, 40), (-33, -2)] {
            let (r, i) = region_of(cx, cz);
            assert_eq!(chunk_of(r, i), (cx, cz));
        }
        assert_eq!(region_file((-1, 2)), "r.-1.2.bin");
    }

    #[test]
    fn folder_names() {
        assert_eq!(
            sanitize("server:mc.example.com:25565"),
            "server_mc.example.com_25565"
        );
        assert_eq!(sanitize(".."), "_");
        assert_eq!(sanitize(""), "_");
        assert_eq!(
            world_folder(&Server::Remote("MC.Example.com:25565".into()), ""),
            "server_mc.example.com_25565"
        );
        assert_eq!(world_folder(&Server::Local, "Neue Welt"), "world_Neue_Welt");
        assert_eq!(world_folder(&Server::Local, ""), "world_unknown");
        assert_eq!(sanitize("minecraft:the_nether"), "minecraft_the_nether");
    }

    #[test]
    fn file_roundtrip_and_rejects() {
        let mut r = Region::new();
        assert!(r.put(0, tile(1)));
        assert!(r.put(1023, tile(2)));
        assert!(!r.put(0, tile(1)));
        let bytes = r.encode();
        let back = Region::decode(&bytes).expect("valid");
        assert_eq!(back.tiles[0].as_deref(), Some(&tile(1)));
        assert_eq!(back.tiles[1023].as_deref(), Some(&tile(2)));
        assert!(back.tiles[5].is_none());
        // empty region
        assert!(Region::decode(&Region::new().encode()).is_some());
        // garbage, wrong magic, truncated
        assert!(Region::decode(b"nope").is_none());
        assert!(Region::decode(&share::gzip(b"XXXX\x01\x00\x00")).is_none());
        assert!(Region::decode(&share::gzip(b"OTWM\x02\x00\x00")).is_none());
        assert!(Region::decode(&share::gzip(b"OTWM\x01\x01\x00")).is_none());
        // trailing bytes
        assert!(Region::decode(&share::gzip(b"OTWM\x01\x00\x00\x00")).is_none());
    }

    #[test]
    fn store_saves_merges_and_quarantines() {
        let dir = temp("store");
        let mut s = Store::new(Some(dir.clone()));
        assert!(s.put(3, -4, tile(7), 0));
        assert!(!s.put(3, -4, tile(7), 0));
        assert!(s.put(3, -4, tile(8), 0));
        s.flush();
        let file = dir.join("r.0.-1.bin");
        assert!(file.exists());
        assert!(!dir.join("r.0.-1.bin.tmp").exists());
        // reopened: loaded, merged with a second chunk
        let mut s = Store::new(Some(dir.clone()));
        assert_eq!(s.tile(3, -4, 0), Some(&tile(8)));
        assert!(s.put(4, -4, tile(9), 0));
        s.flush();
        let mut s = Store::new(Some(dir.clone()));
        assert_eq!(s.tile(3, -4, 0), Some(&tile(8)));
        assert_eq!(s.tile(4, -4, 0), Some(&tile(9)));
        // corrupt file: moved aside, region starts empty
        fs::write(&file, b"garbage").unwrap();
        let mut s = Store::new(Some(dir.clone()));
        assert_eq!(s.tile(3, -4, 42), None);
        assert!(dir.join("r.0.-1.bin.broken-42").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn trim_keeps_dirty_and_recent() {
        let mut s = Store::new(None);
        s.put(0, 0, tile(1), 0); // region (0,0), dirty
        s.tile(32, 0, 0); // clean
        s.tile(64, 0, 0); // clean
        s.tile(96, 0, 0); // clean, most recent
        s.trim(2);
        assert_eq!(s.loaded(), 2);
        assert!(s.regions.contains_key(&(0, 0)));
        assert!(s.regions.contains_key(&(3, 0)));
        s.trim(0); // dirty stays
        assert_eq!(s.loaded(), 1);
    }

    #[test]
    fn due_and_queue() {
        assert!(due(None, 5, 0, false));
        assert!(!due(Some((5, 0)), 5, 5000, true));
        assert!(!due(Some((4, 0)), 5, 5000, false));
        assert!(!due(Some((4, 4500)), 5, 5000, true));
        assert!(due(Some((4, 4000)), 5, 5000, true));
        let loaded = Loaded {
            revision: 2,
            center: (0, 0),
            chunks: vec![(5, 0), (0, 1), (-1, -1), (9, 9), (0, 0)],
        };
        let mut seen = HashMap::new();
        seen.insert((0, 0), (2, 0));
        let mut q = queue(&loaded, &seen, 10_000, true, -1);
        // nearest last (popped first)
        assert_eq!(q.pop(), Some((-1, -1)));
        assert_eq!(q.pop(), Some((0, 1)));
        assert_eq!(q.len(), 2);
        let q = queue(&loaded, &seen, 10_000, true, 5);
        assert_eq!(q.len(), 3);
        // changed revision, long ago: the seen chunk is due again
        let loaded = Loaded {
            revision: 3,
            ..loaded
        };
        assert!(queue(&loaded, &seen, 10_000, true, 0).contains(&(0, 0)));
        assert!(!queue(&loaded, &seen, 10_000, false, 0).contains(&(0, 0)));
    }
}
