// World Map, phase 2 (docs/spec/omnitool/world-map.md §6): the full-screen
// map. Horizon has no screen hook, so the game stays in Screen::Playing with
// the cursor released; the glue feeds window input (`push`) and draws the
// `Op`s this module returns in `native_hud`. The map image lives in 128x128
// texture tiles (atlas slots borrowed from the pack screen, see glue.rs),
// composed from the World Map store and cached per (tile, level). Zoom, pan,
// the player arrow, waypoint markers and the right-click menu (teleport,
// add waypoint) are pure logic here. Added to the game as
// `game::omnitool_world_map_screen`; also a module of this crate.

use super::omnitool_core::{self as core, Host};
use super::omnitool_lang::tr;
use super::omnitool_waypoints::{self as waypoints, Waypoint};
use super::omnitool_world_map::{self as world_map, brightness, shade};
use std::cell::RefCell;

/// GUI pixels per block of the zoom steps.
pub const ZOOMS: [f64; 8] = [0.125, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0];
/// The zoom a fresh map opens with (1 block = 1 GUI pixel).
pub const DEFAULT_ZOOM: usize = 3;
/// Texture tiles (atlas slots) and their side in texels.
pub const SLOTS: usize = 64;
pub const TEX: usize = 128;
/// The coarsest level: blocks per texel.
pub const MAX_BPT: i32 = 8;
/// Composing budget per frame (at least one tile is composed).
const BUDGET_US: u128 = 4000;
/// Pan per arrow/WASD press, in GUI pixels.
const PAN_PX: f64 = 48.0;
/// Pixels within which the cursor is over a marker.
const HOVER_PX: f64 = 5.0;
/// A closed map whose frames stopped (F1 hides the HUD) closes after this.
pub const STALE_MS: u64 = 1000;
/// More touched chunks than this rebuild every tile instead.
pub const TOUCH_LIMIT: usize = 4096;

pub const BACKGROUND: u32 = 0xFF10_1014;
const MENU_BG: u32 = 0xF010_1010;
const MENU_BORDER: u32 = 0xFF60_6060;
const MENU_HOVER: u32 = 0x40FF_FFFF;
const WHITE: u32 = 0xFF_FFFF;
const GRAY: u32 = 0x80_8080;
const ENTRY_H: f64 = 12.0;

/// Window input while the map is open (keys as vanilla names,
/// `key.keyboard.escape`; buttons 0 left, 1 right, 2 middle).
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Key(String),
    Click(i32),
    Release(i32),
    Scroll(f64),
}

/// What the glue draws, in GUI pixels; colours ARGB (`Text`: RGB).
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Fill([f32; 4], u32),
    /// Four corners in order round the shape.
    Quad([[f32; 2]; 4], u32),
    Text {
        text: String,
        x: f32,
        y: f32,
        rgb: u32,
        shadow: bool,
    },
    /// Texture slot `slot` stretched over `[x, y, w, h]`.
    Tex {
        slot: usize,
        rect: [f32; 4],
    },
}

/// One column of the map: colour id, height, water depth.
pub trait MapSource {
    fn column(&mut self, bx: i32, bz: i32) -> Option<(u8, i16, u8)>;
    /// Chunks changed since the last call; None = everything may have.
    fn touched(&mut self) -> Option<Vec<(i32, i32)>>;
}

/// The World Map store as the screen reads it.
pub struct StoreSource<'a> {
    pub store: &'a mut world_map::Store,
    pub now_ms: u64,
}

impl MapSource for StoreSource<'_> {
    fn column(&mut self, bx: i32, bz: i32) -> Option<(u8, i16, u8)> {
        let t = self.store.tile(bx >> 4, bz >> 4, self.now_ms)?;
        let i = ((bz & 15) * 16 + (bx & 15)) as usize;
        Some((t.color[i], t.height[i], t.depth[i]))
    }
    fn touched(&mut self) -> Option<Vec<(i32, i32)>> {
        self.store.take_touched()
    }
}

/// A cached texture tile: `TEX` texels of `bpt` blocks each from the block
/// `(tx, tz) * TEX * bpt`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub tx: i32,
    pub tz: i32,
    pub bpt: i32,
}

impl TileKey {
    /// Blocks a tile spans.
    pub fn span(&self) -> i32 {
        TEX as i32 * self.bpt
    }
    /// Whether chunk `(cx, cz)` lies in this tile.
    pub fn covers(&self, cx: i32, cz: i32) -> bool {
        let s = self.span();
        (cx * 16).div_euclid(s) == self.tx && (cz * 16).div_euclid(s) == self.tz
    }
}

#[derive(Clone, Copy, Debug)]
struct Slot {
    key: TileKey,
    used: u64,
    ready: bool,
}

/// The texture tiles in the atlas slots, least recently shown replaced first.
#[derive(Debug)]
pub struct TexCache {
    slots: Vec<Option<Slot>>,
    clock: u64,
}

impl Default for TexCache {
    fn default() -> Self {
        TexCache {
            slots: vec![None; SLOTS],
            clock: 0,
        }
    }
}

impl TexCache {
    /// Every slot must be composed again (the atlas may have been drawn
    /// over by the pack screen while the map was closed).
    pub fn invalidate(&mut self) {
        self.slots.iter_mut().for_each(|s| *s = None);
    }
    /// Tiles that cover a touched chunk are composed again.
    pub fn touch(&mut self, chunks: &[(i32, i32)]) {
        for s in self.slots.iter_mut().flatten() {
            if chunks.iter().any(|&(cx, cz)| s.key.covers(cx, cz)) {
                s.ready = false;
            }
        }
    }
    /// The slot of each visible key (assigning slots of tiles not visible
    /// now, least recently used first); keys beyond the slots get none.
    pub fn assign(&mut self, visible: &[TileKey]) -> Vec<Option<usize>> {
        self.clock += 1;
        let clock = self.clock;
        let mut out = Vec::with_capacity(visible.len());
        for k in visible {
            let found = self
                .slots
                .iter()
                .position(|s| s.is_some_and(|s| s.key == *k));
            let slot = found.or_else(|| {
                let free = self.slots.iter().position(Option::is_none);
                free.or_else(|| {
                    self.slots
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| s.is_some_and(|s| s.used != clock))
                        .min_by_key(|(_, s)| s.map_or(0, |s| s.used))
                        .map(|(i, _)| i)
                })
            });
            if let Some(i) = slot {
                let ready = found.is_some() && self.slots[i].is_some_and(|s| s.ready);
                self.slots[i] = Some(Slot {
                    key: *k,
                    used: clock,
                    ready,
                });
            }
            out.push(slot);
        }
        out
    }
    pub fn ready(&self, slot: usize) -> bool {
        self.slots[slot].is_some_and(|s| s.ready)
    }
    fn set_ready(&mut self, slot: usize) {
        if let Some(s) = self.slots[slot].as_mut() {
            s.ready = true;
        }
    }
}

/// The RGBA pixels of a tile (`TEX` x `TEX`, rows north to south): each
/// texel shows the column at its north-west block, shaded against the
/// block north of it (§2.2); unknown columns are transparent.
pub fn compose(key: TileKey, src: &mut dyn MapSource, colors: &[u32]) -> Vec<u8> {
    let mut out = vec![0u8; TEX * TEX * 4];
    let (ox, oz) = (key.tx * key.span(), key.tz * key.span());
    for v in 0..TEX {
        for u in 0..TEX {
            let (bx, bz) = (ox + u as i32 * key.bpt, oz + v as i32 * key.bpt);
            let Some((color, height, depth)) = src.column(bx, bz) else {
                continue;
            };
            let id = usize::from(color & 0x7F);
            if id == 0 || id >= colors.len() {
                continue;
            }
            let north = src.column(bx, bz - 1).map(|c| c.1);
            let b = brightness(color, height, north, depth, (bx + bz) & 1);
            let [r, g, bl] = shade(colors[id], b);
            let i = (v * TEX + u) * 4;
            out[i..i + 4].copy_from_slice(&[r, g, bl, 255]);
        }
    }
    out
}

/// Blocks per texel at zoom `z` on a `w` x `h` screen: at least one GUI
/// pixel per texel, coarser until the visible tiles fit the slots; None
/// when even `MAX_BPT` does not fit.
pub fn level(z: f64, w: f64, h: f64) -> Option<i32> {
    let mut bpt = 1;
    while (bpt as f64) * z < 1.0 {
        bpt *= 2;
    }
    while bpt <= MAX_BPT {
        let t = TEX as f64 * bpt as f64 * z;
        let n = ((w / t).ceil() + 1.0) * ((h / t).ceil() + 1.0);
        if n <= SLOTS as f64 {
            return Some(bpt);
        }
        bpt *= 2;
    }
    None
}

/// The tiles a view shows, nearest the centre first.
pub fn visible(center: (f64, f64), z: f64, w: f64, h: f64, bpt: i32) -> Vec<TileKey> {
    let span = f64::from(TEX as i32 * bpt);
    let (x0, x1) = (center.0 - w / 2.0 / z, center.0 + w / 2.0 / z);
    let (z0, z1) = (center.1 - h / 2.0 / z, center.1 + h / 2.0 / z);
    let mut out = Vec::new();
    for tz in (z0 / span).floor() as i32..=(z1 / span).floor() as i32 {
        for tx in (x0 / span).floor() as i32..=(x1 / span).floor() as i32 {
            out.push(TileKey { tx, tz, bpt });
        }
    }
    let d = |k: &TileKey| {
        let cx = (f64::from(k.tx) + 0.5) * span - center.0;
        let cz = (f64::from(k.tz) + 0.5) * span - center.1;
        cx * cx + cz * cz
    };
    out.sort_by(|a, b| d(a).total_cmp(&d(b)));
    out
}

/// The zoom label: `2x`, `1x`, `1/4x`.
pub fn zoom_label(z: f64) -> String {
    if z >= 1.0 {
        format!("{}x", z as i32)
    } else {
        format!("1/{}x", (1.0 / z).round() as i32)
    }
}

/// The player arrow's corners round it: tip, right, notch, left (yaw in
/// degrees, 0 = south, 90 = west as in Minecraft).
pub fn arrow(x: f64, y: f64, yaw: f32, size: f64) -> [[f32; 2]; 4] {
    let a = f64::from(yaw).to_radians();
    let (dx, dy) = (-a.sin(), a.cos());
    let (px, py) = (-dy, dx);
    let p = |f: f64, s: f64| [(x + dx * f + px * s) as f32, (y + dy * f + py * s) as f32];
    [
        p(size, 0.0),
        p(-size * 0.7, size * 0.7),
        p(-size * 0.3, 0.0),
        p(-size * 0.7, -size * 0.7),
    ]
}

/// A right-click menu entry.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    TeleportHere { x: i32, y: i32, z: i32 },
    AddWaypoint { x: i32, y: i32, z: i32 },
    TeleportWaypoint(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub label: String,
    pub action: Option<Action>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub at: (f64, f64),
    pub width: f64,
    pub entries: Vec<Entry>,
}

impl Menu {
    /// The entry under `(mx, my)`.
    pub fn hit(&self, mx: f64, my: f64) -> Option<usize> {
        let (x, y) = self.at;
        if mx < x || mx >= x + self.width || my < y + 2.0 {
            return None;
        }
        let i = ((my - y - 2.0) / ENTRY_H).floor() as usize;
        (i < self.entries.len()).then_some(i)
    }
    fn height(&self) -> f64 {
        self.entries.len() as f64 * ENTRY_H + 4.0
    }
}

/// The map screen's state (one per game thread).
#[derive(Debug)]
pub struct Screen {
    pub open: bool,
    request: bool,
    pub center: (f64, f64),
    pub zoom: usize,
    /// Mouse and centre when the left button went down.
    drag: Option<((f64, f64), (f64, f64))>,
    pub menu: Option<Menu>,
    scroll: f64,
    queue: Vec<Input>,
    pub tex: TexCache,
    /// When the last frame ran (`STALE_MS`).
    pub last_frame_ms: u64,
    /// The dimension the cached tiles show.
    dim: String,
}

impl Default for Screen {
    fn default() -> Self {
        Screen {
            open: false,
            request: false,
            center: (0.0, 0.0),
            zoom: DEFAULT_ZOOM,
            drag: None,
            menu: None,
            scroll: 0.0,
            queue: Vec::new(),
            tex: TexCache::default(),
            last_frame_ms: 0,
            dim: String::new(),
        }
    }
}

/// What a frame needs from the game.
pub struct FrameIn<'a> {
    pub w: f64,
    pub h: f64,
    pub mouse: (f64, f64),
    pub now_ms: u64,
    pub colors: &'a [u32],
    /// The open-map key's vanilla name (pressing it again closes).
    pub open_key: Option<String>,
}

/// What a frame gives back.
#[derive(Debug, Default)]
pub struct FrameOut {
    pub ops: Vec<Op>,
    /// Tile pixels to upload: (slot, RGBA `TEX` x `TEX`).
    pub uploads: Vec<(usize, Vec<u8>)>,
    /// The map closed this frame (the glue captures the cursor again).
    pub close: bool,
}

impl Screen {
    pub fn z(&self) -> f64 {
        ZOOMS[self.zoom]
    }

    /// Opens centred on the player; every tile is composed again.
    pub fn open_at(&mut self, x: f64, z: f64, dim: &str) {
        self.open = true;
        self.request = false;
        self.center = (x, z);
        self.drag = None;
        self.menu = None;
        self.scroll = 0.0;
        self.queue.clear();
        self.tex.invalidate();
        self.dim = dim.to_owned();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.request = false;
        self.drag = None;
        self.menu = None;
        self.queue.clear();
    }

    /// Block coordinates under a screen point.
    pub fn to_world(&self, sx: f64, sy: f64, w: f64, h: f64) -> (f64, f64) {
        let z = self.z();
        (
            self.center.0 + (sx - w / 2.0) / z,
            self.center.1 + (sy - h / 2.0) / z,
        )
    }

    /// The screen point of a world position.
    pub fn to_screen(&self, wx: f64, wz: f64, w: f64, h: f64) -> (f64, f64) {
        let z = self.z();
        (
            w / 2.0 + (wx - self.center.0) * z,
            h / 2.0 + (wz - self.center.1) * z,
        )
    }

    /// One zoom step in (`dir > 0`) or out about the screen point
    /// `(sx, sy)`, which keeps showing the same block; steps the screen
    /// cannot fit (`level`) are skipped.
    pub fn zoom_by(&mut self, dir: i32, sx: f64, sy: f64, w: f64, h: f64) {
        let next = self.zoom as i32 + dir.signum();
        if next < 0 || next >= ZOOMS.len() as i32 {
            return;
        }
        let next = next as usize;
        if level(ZOOMS[next], w, h).is_none() {
            return;
        }
        let (wx, wz) = self.to_world(sx, sy, w, h);
        self.zoom = next;
        let z = self.z();
        self.center = (wx - (sx - w / 2.0) / z, wz - (sy - h / 2.0) / z);
    }

    /// The smallest zoom that fits, if the current one does not (a
    /// smaller window or GUI scale since).
    fn fit_zoom(&mut self, w: f64, h: f64) {
        while level(self.z(), w, h).is_none() && self.zoom + 1 < ZOOMS.len() {
            self.zoom += 1;
        }
    }
}

thread_local! {
    static SCREEN: RefCell<Screen> = RefCell::new(Screen::default());
}

pub fn with<R>(f: impl FnOnce(&mut Screen) -> R) -> R {
    SCREEN.with_borrow_mut(f)
}

pub fn is_open() -> bool {
    with(|s| s.open)
}

/// The open key was pressed while playing.
pub fn request() {
    with(|s| s.request = true);
}

pub fn take_request() -> bool {
    with(|s| std::mem::take(&mut s.request))
}

/// Input from the window while open.
pub fn push(input: Input) {
    with(|s| {
        if s.open {
            s.queue.push(input);
        }
    });
}

/// Opens the map if a request is pending and World Map is active; true
/// when it opened (the glue releases the cursor).
pub fn try_open(host: &mut dyn Host) -> bool {
    if !take_request() || is_open() || !core::active(host, &world_map::MODULE) {
        return false;
    }
    let Some(p) = host.player() else {
        return false;
    };
    with(|s| s.open_at(p.x, p.z, &p.dimension));
    true
}

/// The waypoints shown: the current world's, in the player's dimension.
fn markers(host: &dyn Host, dim: &str) -> Vec<Waypoint> {
    waypoints::all(host)
        .into_iter()
        .filter(|w| w.dim.as_deref() == Some(dim))
        .collect()
}

fn text(ops: &mut Vec<Op>, s: &str, x: f64, y: f64, rgb: u32) {
    ops.push(Op::Text {
        text: s.to_owned(),
        x: x as f32,
        y: y as f32,
        rgb,
        shadow: true,
    });
}

fn fill(ops: &mut Vec<Op>, x: f64, y: f64, w: f64, h: f64, argb: u32) {
    ops.push(Op::Fill([x as f32, y as f32, w as f32, h as f32], argb));
}

/// One frame of the open map: input, tiles, drawing, menu actions.
pub fn frame(host: &mut dyn Host, src: &mut dyn MapSource, input: &FrameIn) -> FrameOut {
    let mut out = FrameOut::default();
    let Some(player) = host.player() else {
        with(Screen::close);
        out.close = true;
        return out;
    };
    let german = core::read(host, |st| st.german(host));
    let marks = markers(host, &player.dimension);
    let (w, h) = (input.w, input.h);
    let (mx, my) = input.mouse;
    let mut actions = Vec::new();
    let closed = with(|s| {
        s.last_frame_ms = input.now_ms;
        if s.dim != player.dimension {
            s.tex.invalidate();
            s.dim.clone_from(&player.dimension);
        }
        s.fit_zoom(w, h);
        // input
        for ev in std::mem::take(&mut s.queue) {
            match ev {
                Input::Key(k) => {
                    let z = s.z();
                    match k.as_str() {
                        "key.keyboard.escape" if s.menu.is_some() => s.menu = None,
                        "key.keyboard.escape" => return true,
                        _ if input.open_key.as_deref() == Some(k.as_str()) => return true,
                        "key.keyboard.left" | "key.keyboard.a" => s.center.0 -= PAN_PX / z,
                        "key.keyboard.right" | "key.keyboard.d" => s.center.0 += PAN_PX / z,
                        "key.keyboard.up" | "key.keyboard.w" => s.center.1 -= PAN_PX / z,
                        "key.keyboard.down" | "key.keyboard.s" => s.center.1 += PAN_PX / z,
                        "key.keyboard.c" | "key.keyboard.space" => {
                            s.center = (player.x, player.z);
                        }
                        "key.keyboard.equal" | "key.keyboard.keypad.add" => {
                            s.zoom_by(1, w / 2.0, h / 2.0, w, h);
                        }
                        "key.keyboard.minus" | "key.keyboard.keypad.subtract" => {
                            s.zoom_by(-1, w / 2.0, h / 2.0, w, h);
                        }
                        _ => {}
                    }
                }
                Input::Scroll(v) => {
                    s.scroll += v;
                    while s.scroll >= 1.0 {
                        s.scroll -= 1.0;
                        s.zoom_by(1, mx, my, w, h);
                    }
                    while s.scroll <= -1.0 {
                        s.scroll += 1.0;
                        s.zoom_by(-1, mx, my, w, h);
                    }
                }
                Input::Click(0) => {
                    if let Some(menu) = s.menu.take() {
                        if let Some(i) = menu.hit(mx, my)
                            && let Some(a) = menu.entries[i].action.clone()
                        {
                            actions.push(a);
                        }
                    } else {
                        s.drag = Some(((mx, my), s.center));
                    }
                }
                Input::Click(1) => {
                    s.drag = None;
                    s.menu = Some(context_menu(host, s, src, &marks, input, german));
                }
                Input::Release(0) => s.drag = None,
                _ => {}
            }
        }
        if let Some(((sx, sy), c)) = s.drag {
            let z = s.z();
            s.center = (c.0 - (mx - sx) / z, c.1 - (my - sy) / z);
        }
        false
    });
    if closed {
        with(Screen::close);
        out.close = true;
        return out;
    }
    // tiles
    with(|s| {
        match src.touched() {
            Some(chunks) => s.tex.touch(&chunks),
            None => s.tex.invalidate(),
        }
        let z = s.z();
        let Some(bpt) = level(z, w, h) else { return };
        let keys = visible(s.center, z, w, h, bpt);
        let slots = s.tex.assign(&keys);
        let started = std::time::Instant::now();
        let mut composed = 0;
        for (k, slot) in keys.iter().zip(&slots) {
            let Some(slot) = *slot else { continue };
            if s.tex.ready(slot) {
                continue;
            }
            if composed > 0 && started.elapsed().as_micros() >= BUDGET_US {
                break;
            }
            out.uploads.push((slot, compose(*k, src, input.colors)));
            s.tex.set_ready(slot);
            composed += 1;
        }
        fill(&mut out.ops, 0.0, 0.0, w, h, BACKGROUND);
        let span = f64::from(TEX as i32 * bpt);
        for (k, slot) in keys.iter().zip(&slots) {
            let Some(slot) = *slot else { continue };
            // a tile composed this frame shows once uploaded, before the HUD draw
            if !s.tex.ready(slot) {
                continue;
            }
            let (x, y) = s.to_screen(f64::from(k.tx) * span, f64::from(k.tz) * span, w, h);
            out.ops.push(Op::Tex {
                slot,
                rect: [x as f32, y as f32, (span * z) as f32, (span * z) as f32],
            });
        }
    });
    // markers, player, texts, menu
    with(|s| {
        let hover = hovered(s, &marks, mx, my, w, h);
        for (i, wp) in marks.iter().enumerate() {
            let (x, y) = s.to_screen(f64::from(wp.x) + 0.5, f64::from(wp.z) + 0.5, w, h);
            if x < -50.0 || y < -20.0 || x > w + 50.0 || y > h + 20.0 {
                continue;
            }
            let big = if hover == Some(i) { 4.0 } else { 3.0 };
            fill(
                &mut out.ops,
                x - big - 1.0,
                y - big - 1.0,
                big * 2.0 + 2.0,
                big * 2.0 + 2.0,
                0xFF00_0000,
            );
            fill(
                &mut out.ops,
                x - big,
                y - big,
                big * 2.0,
                big * 2.0,
                (wp.color as u32) | 0xFF00_0000,
            );
            let label = wp.name();
            if !label.is_empty() {
                let tw = f64::from(host.text_width(label));
                text(&mut out.ops, label, x - tw / 2.0, y - big - 11.0, WHITE);
            }
        }
        let (px, py) = s.to_screen(player.x, player.z, w, h);
        out.ops
            .push(Op::Quad(arrow(px, py, player.yaw, 7.0), 0xFF00_0000));
        out.ops
            .push(Op::Quad(arrow(px, py, player.yaw, 5.0), 0xFFFF_FFFF));
        // texts
        let title = tr(german, "World Map");
        let tw = f64::from(host.text_width(title));
        text(&mut out.ops, title, (w - tw) / 2.0, 4.0, WHITE);
        let (wx, wz) = s.to_world(mx, my, w, h);
        let (bx, bz) = (wx.floor() as i32, wz.floor() as i32);
        let y = src
            .column(bx, bz)
            .filter(|c| c.0 != 0)
            .map_or_else(|| "?".to_owned(), |c| c.1.to_string());
        text(
            &mut out.ops,
            &format!("{bx}, {y}, {bz}"),
            4.0,
            h - 12.0,
            WHITE,
        );
        let zl = format!("{}: {}", tr(german, "Zoom"), zoom_label(s.z()));
        let zw = f64::from(host.text_width(&zl));
        text(&mut out.ops, &zl, w - zw - 4.0, h - 12.0, WHITE);
        let hint = tr(
            german,
            "Drag: move | Scroll: zoom | Right-click: menu | C: center | Esc: close",
        );
        let hw = f64::from(host.text_width(hint));
        text(&mut out.ops, hint, (w - hw) / 2.0, h - 24.0, GRAY);
        if let Some(menu) = &s.menu {
            let (x, y) = menu.at;
            let mh = menu.height();
            fill(
                &mut out.ops,
                x - 1.0,
                y - 1.0,
                menu.width + 2.0,
                mh + 2.0,
                MENU_BORDER,
            );
            fill(&mut out.ops, x, y, menu.width, mh, MENU_BG);
            let over = menu.hit(mx, my);
            for (i, e) in menu.entries.iter().enumerate() {
                let ey = y + 2.0 + i as f64 * ENTRY_H;
                if over == Some(i) && e.action.is_some() {
                    fill(&mut out.ops, x, ey, menu.width, ENTRY_H, MENU_HOVER);
                }
                let rgb = if e.action.is_some() { WHITE } else { GRAY };
                text(&mut out.ops, &e.label, x + 4.0, ey + 2.0, rgb);
            }
        }
    });
    for a in actions {
        if run(host, a, &marks, &player.dimension) {
            with(Screen::close);
            out.close = true;
        }
    }
    out
}

/// The marker under the cursor.
fn hovered(s: &Screen, marks: &[Waypoint], mx: f64, my: f64, w: f64, h: f64) -> Option<usize> {
    marks
        .iter()
        .enumerate()
        .map(|(i, wp)| {
            let (x, y) = s.to_screen(f64::from(wp.x) + 0.5, f64::from(wp.z) + 0.5, w, h);
            (i, (x - mx).abs().max((y - my).abs()))
        })
        .filter(|(_, d)| *d <= HOVER_PX)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The right-click menu at the cursor: teleport to / add a waypoint at the
/// block under it (needs a known column for the height), and teleport to
/// the waypoint under it.
fn context_menu(
    host: &dyn Host,
    s: &Screen,
    src: &mut dyn MapSource,
    marks: &[Waypoint],
    input: &FrameIn,
    german: bool,
) -> Menu {
    let (mx, my) = input.mouse;
    let (wx, wz) = s.to_world(mx, my, input.w, input.h);
    let (bx, bz) = (wx.floor() as i32, wz.floor() as i32);
    let y = src
        .column(bx, bz)
        .filter(|c| c.0 != 0)
        .map(|c| i32::from(c.1) + 1);
    let mut entries = vec![Entry {
        label: format!(
            "{bx}, {}, {bz}",
            y.map_or_else(|| "?".to_owned(), |y| y.to_string())
        ),
        action: None,
    }];
    if let Some(i) = hovered(s, marks, mx, my, input.w, input.h) {
        entries.push(Entry {
            label: format!("{} '{}'", tr(german, "Teleport to"), marks[i].name()),
            action: Some(Action::TeleportWaypoint(i)),
        });
    }
    entries.push(Entry {
        label: tr(german, "Teleport here").to_owned(),
        action: y.map(|y| Action::TeleportHere { x: bx, y, z: bz }),
    });
    entries.push(Entry {
        label: tr(german, "Add waypoint here").to_owned(),
        action: y.map(|y| Action::AddWaypoint { x: bx, y, z: bz }),
    });
    let width = entries
        .iter()
        .map(|e| f64::from(host.text_width(&e.label)))
        .fold(0.0, f64::max)
        + 8.0;
    let height = entries.len() as f64 * ENTRY_H + 4.0;
    let at = (
        mx.min(input.w - width - 1.0).max(1.0),
        my.min(input.h - height - 1.0).max(1.0),
    );
    Menu { at, width, entries }
}

/// Runs a menu action; true when the map closes (teleports).
fn run(host: &mut dyn Host, a: Action, marks: &[Waypoint], dim: &str) -> bool {
    match a {
        Action::TeleportHere { x, y, z } => {
            let w = Waypoint::new(&format!("{x}, {y}, {z}"), x, y, z, dim, 0);
            waypoints::teleport_to(host, &w)
        }
        Action::AddWaypoint { x, y, z } => {
            waypoints::add_at(host, None, x, y, z, dim);
            false
        }
        Action::TeleportWaypoint(i) => marks
            .get(i)
            .is_some_and(|w| waypoints::teleport_to(host, w)),
    }
}

/// Closes the map when its frames stopped (HUD hidden) or the game left
/// the playing state; true when it closed and the cursor should be
/// captured again (`playing`).
pub fn check(now_ms: u64, playing: bool) -> Option<bool> {
    with(|s| {
        if !s.open {
            return None;
        }
        if !playing || now_ms.saturating_sub(s.last_frame_ms) > STALE_MS {
            s.close();
            return Some(playing);
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::Fake;
    use std::collections::HashMap;

    struct Src {
        cols: HashMap<(i32, i32), (u8, i16, u8)>,
        touched: Option<Vec<(i32, i32)>>,
    }

    impl MapSource for Src {
        fn column(&mut self, bx: i32, bz: i32) -> Option<(u8, i16, u8)> {
            self.cols.get(&(bx, bz)).copied()
        }
        fn touched(&mut self) -> Option<Vec<(i32, i32)>> {
            self.touched.take().or(Some(Vec::new()))
        }
    }

    fn src() -> Src {
        Src {
            cols: HashMap::new(),
            touched: None,
        }
    }

    fn colors() -> Vec<u32> {
        let mut c = vec![0u32; 62];
        c[1] = 0x7FB238; // grass
        c[12] = 0x4040FF; // water
        c
    }

    #[test]
    fn level_fits_slots() {
        // 960x540 GUI px: 1 block per texel from zoom 1 up
        assert_eq!(level(1.0, 960.0, 540.0), Some(1));
        assert_eq!(level(16.0, 960.0, 540.0), Some(1));
        assert_eq!(level(0.5, 960.0, 540.0), Some(2));
        assert_eq!(level(0.125, 960.0, 540.0), Some(8));
        // GUI scale 1 on 1920x1080: zoom 1 gets coarser texels, 1/8 does not fit
        assert_eq!(level(1.0, 1920.0, 1080.0), Some(2));
        assert_eq!(level(0.125, 1920.0, 1080.0), None);
        for z in ZOOMS {
            if let Some(bpt) = level(z, 960.0, 540.0) {
                let t = TEX as f64 * f64::from(bpt) * z;
                assert!(((960.0 / t).ceil() + 1.0) * ((540.0 / t).ceil() + 1.0) <= SLOTS as f64);
            }
        }
    }

    #[test]
    fn visible_covers_screen_nearest_first() {
        let keys = visible((0.0, 0.0), 1.0, 300.0, 200.0, 1);
        // x -150..150 -> tiles -2..1, z -100..100 -> -1..0
        assert_eq!(keys.len(), 8);
        assert!(keys[..4].iter().all(|k| (-1..=0).contains(&k.tx)));
        assert!(keys.len() <= SLOTS);
        let neg = visible((-10.0, -10.0), 2.0, 10.0, 10.0, 1);
        assert_eq!(
            neg,
            vec![TileKey {
                tx: -1,
                tz: -1,
                bpt: 1
            }]
        );
    }

    #[test]
    fn tile_covers_chunks() {
        let k = TileKey {
            tx: -1,
            tz: 0,
            bpt: 1,
        };
        assert!(k.covers(-1, 0));
        assert!(k.covers(-8, 7));
        assert!(!k.covers(0, 0));
        assert!(!k.covers(-9, 0));
        let k8 = TileKey {
            tx: 0,
            tz: 0,
            bpt: 8,
        };
        assert!(k8.covers(63, 63));
        assert!(!k8.covers(64, 0));
    }

    #[test]
    fn cache_reuses_and_evicts_unseen() {
        let mut c = TexCache::default();
        let keys: Vec<TileKey> = (0..SLOTS as i32)
            .map(|i| TileKey {
                tx: i,
                tz: 0,
                bpt: 1,
            })
            .collect();
        let s1 = c.assign(&keys);
        assert!(s1.iter().all(Option::is_some));
        c.set_ready(s1[0].unwrap());
        // same key, same slot, still ready
        let again = c.assign(&keys[..1]);
        assert_eq!(again[0], s1[0]);
        assert!(c.ready(again[0].unwrap()));
        // a new key takes the least recently used slot, not the one just shown
        let fresh = c.assign(&[TileKey {
            tx: 99,
            tz: 0,
            bpt: 1,
        }]);
        assert_ne!(fresh[0], s1[0]);
        assert!(!c.ready(fresh[0].unwrap()));
        // more keys than slots in one frame: the rest get none
        let many: Vec<TileKey> = (0..SLOTS as i32 + 2)
            .map(|i| TileKey {
                tx: i,
                tz: 5,
                bpt: 1,
            })
            .collect();
        let s = c.assign(&many);
        assert_eq!(s.iter().filter(|x| x.is_none()).count(), 2);
        // touch and invalidate
        c.set_ready(s[0].unwrap());
        c.touch(&[(0, 5 * 8)]);
        assert!(!c.ready(s[0].unwrap()));
        c.set_ready(s[0].unwrap());
        c.invalidate();
        assert!(!c.ready(s[0].unwrap()));
    }

    #[test]
    fn compose_shades_and_leaves_unknown_clear() {
        let mut s = src();
        s.cols.insert((0, 0), (1, 64, 0));
        s.cols.insert((0, -1), (1, 63, 0)); // lower north -> brighter
        s.cols.insert((1, 0), (12, 62, 1)); // shallow water
        s.cols.insert((2, 0), (0, -64, 0)); // no colour
        let px = compose(
            TileKey {
                tx: 0,
                tz: 0,
                bpt: 1,
            },
            &mut s,
            &colors(),
        );
        assert_eq!(px.len(), TEX * TEX * 4);
        assert_eq!(&px[0..4], &[0x7F, 0xB2, 0x38, 255]); // brightness 2 = 255
        let w = shade(0x4040FF, brightness(12, 62, None, 1, 1));
        assert_eq!(&px[4..8], &[w[0], w[1], w[2], 255]);
        assert_eq!(&px[8..12], &[0, 0, 0, 0]);
        assert_eq!(&px[12..16], &[0, 0, 0, 0]);
        // a coarser level samples every bpt-th block
        let mut s2 = src();
        s2.cols.insert((8, 0), (1, 64, 0));
        let px = compose(
            TileKey {
                tx: 0,
                tz: 0,
                bpt: 8,
            },
            &mut s2,
            &colors(),
        );
        assert_eq!(px[7], 255);
        assert_eq!(px[3], 0);
    }

    #[test]
    fn zoom_keeps_point_under_cursor() {
        let mut s = Screen::default();
        s.open_at(100.0, 200.0, "minecraft:overworld");
        let before = s.to_world(700.0, 100.0, 960.0, 540.0);
        s.zoom_by(1, 700.0, 100.0, 960.0, 540.0);
        assert_eq!(s.z(), 2.0);
        let after = s.to_world(700.0, 100.0, 960.0, 540.0);
        assert!((before.0 - after.0).abs() < 1e-9 && (before.1 - after.1).abs() < 1e-9);
        // clamps at the ends and skips zooms the screen cannot fit
        for _ in 0..20 {
            s.zoom_by(1, 0.0, 0.0, 960.0, 540.0);
        }
        assert_eq!(s.zoom, ZOOMS.len() - 1);
        for _ in 0..20 {
            s.zoom_by(-1, 0.0, 0.0, 1920.0, 1080.0);
        }
        assert_eq!(s.z(), 0.25);
    }

    #[test]
    fn screen_and_world_round_trip() {
        let mut s = Screen::default();
        s.open_at(-37.5, 12.25, "minecraft:overworld");
        s.zoom = 5;
        let (x, y) = s.to_screen(-30.0, 0.0, 640.0, 480.0);
        let (wx, wz) = s.to_world(x, y, 640.0, 480.0);
        assert!((wx + 30.0).abs() < 1e-9 && wz.abs() < 1e-9);
        assert_eq!(s.to_screen(-37.5, 12.25, 640.0, 480.0), (320.0, 240.0));
    }

    #[test]
    fn arrow_points_along_yaw() {
        // yaw 0 = south = +z = screen down
        let a = arrow(10.0, 10.0, 0.0, 5.0);
        assert!((a[0][0] - 10.0).abs() < 1e-5 && (a[0][1] - 15.0).abs() < 1e-5);
        // yaw 90 = west = screen left
        let a = arrow(10.0, 10.0, 90.0, 5.0);
        assert!((a[0][0] - 5.0).abs() < 1e-5 && (a[0][1] - 10.0).abs() < 1e-5);
    }

    #[test]
    fn labels() {
        assert_eq!(zoom_label(1.0), "1x");
        assert_eq!(zoom_label(16.0), "16x");
        assert_eq!(zoom_label(0.125), "1/8x");
    }

    #[test]
    fn menu_hit() {
        let m = Menu {
            at: (10.0, 20.0),
            width: 50.0,
            entries: vec![
                Entry {
                    label: "a".into(),
                    action: None,
                },
                Entry {
                    label: "b".into(),
                    action: None,
                },
            ],
        };
        assert_eq!(m.hit(15.0, 23.0), Some(0));
        assert_eq!(m.hit(15.0, 22.0 + ENTRY_H), Some(1));
        assert_eq!(m.hit(15.0, 22.0 + 2.0 * ENTRY_H), None);
        assert_eq!(m.hit(9.0, 23.0), None);
        assert_eq!(m.hit(60.0, 23.0), None);
    }

    fn fake_player() -> Fake {
        Fake {
            player: Some(core::Player {
                x: 0.5,
                y: 64.0,
                z: 0.5,
                yaw: 0.0,
                pitch: 0.0,
                dimension: "minecraft:overworld".into(),
            }),
            ..Fake::default()
        }
    }

    fn input<'a>(colors: &'a [u32], mouse: (f64, f64)) -> FrameIn<'a> {
        FrameIn {
            w: 320.0,
            h: 240.0,
            mouse,
            now_ms: 1000,
            colors,
            open_key: Some("key.keyboard.m".into()),
        }
    }

    #[test]
    fn frame_draws_uploads_and_closes() {
        let mut h = fake_player();
        let c = colors();
        let mut s = src();
        s.cols.insert((0, 0), (1, 64, 0));
        with(|sc| sc.open_at(0.5, 0.5, "minecraft:overworld"));
        let out = frame(&mut h, &mut s, &input(&c, (160.0, 120.0)));
        assert!(!out.close);
        assert!(!out.uploads.is_empty());
        assert!(matches!(out.ops[0], Op::Fill(_, BACKGROUND)));
        assert!(out.ops.iter().any(|o| matches!(o, Op::Tex { .. })));
        assert!(out.ops.iter().any(|o| matches!(o, Op::Quad(..))));
        assert!(
            out.ops
                .iter()
                .any(|o| matches!(o, Op::Text { text, .. } if text == "0, 64, 0"))
        );
        // the budget spreads composing over frames; then nothing is uploaded again
        let mut frames = 0;
        while !frame(&mut h, &mut s, &input(&c, (160.0, 120.0)))
            .uploads
            .is_empty()
        {
            frames += 1;
            assert!(frames < SLOTS);
        }
        assert!(
            frame(&mut h, &mut s, &input(&c, (160.0, 120.0)))
                .uploads
                .is_empty()
        );
        // drag moves the centre against the mouse
        push(Input::Click(0));
        frame(&mut h, &mut s, &input(&c, (160.0, 120.0)));
        frame(&mut h, &mut s, &input(&c, (170.0, 120.0)));
        assert!((with(|sc| sc.center.0) - (0.5 - 10.0)).abs() < 1e-9);
        push(Input::Release(0));
        frame(&mut h, &mut s, &input(&c, (170.0, 120.0)));
        // the open key closes
        push(Input::Key("key.keyboard.m".into()));
        let out = frame(&mut h, &mut s, &input(&c, (170.0, 120.0)));
        assert!(out.close);
        assert!(!is_open());
    }

    #[test]
    fn escape_closes_menu_first() {
        let mut h = fake_player();
        let c = colors();
        let mut s = src();
        with(|sc| sc.open_at(0.5, 0.5, "minecraft:overworld"));
        push(Input::Click(1));
        frame(&mut h, &mut s, &input(&c, (100.0, 100.0)));
        let menu = with(|sc| sc.menu.clone()).unwrap();
        // unknown column: header + disabled teleport/add
        assert_eq!(menu.entries.len(), 3);
        assert!(menu.entries.iter().all(|e| e.action.is_none()));
        push(Input::Key("key.keyboard.escape".into()));
        let out = frame(&mut h, &mut s, &input(&c, (100.0, 100.0)));
        assert!(!out.close);
        assert!(with(|sc| sc.menu.is_none()));
        push(Input::Key("key.keyboard.escape".into()));
        assert!(frame(&mut h, &mut s, &input(&c, (100.0, 100.0))).close);
    }

    #[test]
    fn menu_teleports_here() {
        let mut h = fake_player();
        let c = colors();
        let mut s = src();
        // the block under the screen centre (160,120) is the centre (0.5, 0.5) -> (0, 0)
        s.cols.insert((0, 0), (1, 70, 0));
        with(|sc| sc.open_at(0.5, 0.5, "minecraft:overworld"));
        push(Input::Click(1));
        frame(&mut h, &mut s, &input(&c, (160.0, 120.0)));
        let menu = with(|sc| sc.menu.clone()).unwrap();
        assert_eq!(menu.entries[0].label, "0, 71, 0");
        let i = menu
            .entries
            .iter()
            .position(|e| matches!(e.action, Some(Action::TeleportHere { .. })))
            .unwrap();
        let (x, y) = (menu.at.0 + 2.0, menu.at.1 + 3.0 + i as f64 * ENTRY_H);
        push(Input::Click(0));
        let out = frame(&mut h, &mut s, &input(&c, (x, y)));
        assert!(out.close);
        assert_eq!(h.sent, vec!["/tp @s 0 71.5 0".to_owned()]);
    }

    #[test]
    fn check_closes_stale_or_left() {
        with(|sc| {
            sc.open_at(0.0, 0.0, "d");
            sc.last_frame_ms = 1000;
        });
        assert_eq!(check(1500, true), None);
        assert_eq!(check(2500, true), Some(true));
        assert!(!is_open());
        with(|sc| {
            sc.open_at(0.0, 0.0, "d");
            sc.last_frame_ms = 1000;
        });
        assert_eq!(check(1000, false), Some(false));
        assert_eq!(check(1000, false), None);
    }
}
