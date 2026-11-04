// Omni Overlay layouts (docs/spec/omnitool/omni-overlay.md, sections 3.1 and
// 4): the entry catalog, visibility, layouts (normalize, move, presets,
// names, cycle) and overlay.json. Pure logic, checked against
// tests/fixtures/omni-overlay.json. Added to the game as
// `game::omnitool_overlay_layouts`; also a module of this crate.

use super::omnitool_config as config;
use super::omnitool_json::{Json, Obj};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

/// Java's `Visibility`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Always,
    Overlay,
    Never,
}

const VISIBILITIES: [Visibility; 3] = [Visibility::Always, Visibility::Overlay, Visibility::Never];

impl Visibility {
    /// The enum name in overlay.json.
    pub fn name(self) -> &'static str {
        match self {
            Visibility::Always => "ALWAYS",
            Visibility::Overlay => "OVERLAY",
            Visibility::Never => "NEVER",
        }
    }
    /// The English label (translation key) in the layout screen.
    pub fn label(self) -> &'static str {
        match self {
            Visibility::Always => "Always",
            Visibility::Overlay => "In overlay",
            Visibility::Never => "Never",
        }
    }
    #[allow(dead_code)]
    pub fn color(self) -> u32 {
        match self {
            Visibility::Always => 0xFF3F_CF6A,
            Visibility::Overlay => 0xFF3F_A9F5,
            Visibility::Never => 0xFF8A_93A3,
        }
    }
    pub fn shows(self, open: bool) -> bool {
        match self {
            Visibility::Always => true,
            Visibility::Overlay => open,
            Visibility::Never => false,
        }
    }
    /// `Always → In overlay → Never → Always` forwards, reverse for -1.
    pub fn next(self, dir: i32) -> Visibility {
        let i = VISIBILITIES.iter().position(|v| *v == self).unwrap_or(0) as i32;
        VISIBILITIES[(i + dir).rem_euclid(3) as usize]
    }
    /// The enum name, any case; `fallback` otherwise (not the label).
    pub fn parse(s: &str, fallback: Visibility) -> Visibility {
        VISIBILITIES
            .iter()
            .copied()
            .find(|v| v.name().eq_ignore_ascii_case(s))
            .unwrap_or(fallback)
    }
}

// ------------------------------------------------------------ entries

/// One overlay entry as `Entries.ALL` lists it (the text is in
/// omni_overlay.rs).
pub struct EntryInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    /// Default column.
    pub right: bool,
    pub beta: bool,
}

const fn e(id: &'static str, name: &'static str, group: &'static str) -> EntryInfo {
    EntryInfo {
        id,
        name,
        group,
        right: false,
        beta: false,
    }
}

const fn right(mut i: EntryInfo) -> EntryInfo {
    i.right = true;
    i
}

const fn beta(mut i: EntryInfo) -> EntryInfo {
    i.beta = true;
    i
}

/// Java's `Entries.ALL`, with the beta entries.
pub const ENTRIES: &[EntryInfo] = &[
    e("fps", "FPS", "Game"),
    right(e("memory", "Memory", "Game")),
    e("ping", "Ping", "Game"),
    beta(e("tps", "Server TPS", "Game")),
    beta(e("lag", "Lag warning", "Game")),
    e("coords", "Coordinates", "Position"),
    e("block", "Block position", "Position"),
    e("chunk", "Chunk", "Position"),
    e("facing", "Facing", "Position"),
    e("nether", "Nether/overworld coordinates", "Position"),
    e("speed", "Speed", "Position"),
    e("dimension", "Dimension", "Position"),
    e("waypoint", "Nearest waypoint", "Position"),
    e("bed", "Distance to bed", "Position"),
    e("biome", "Biome", "World"),
    e("light", "Light level", "World"),
    e("time", "Time of day", "World"),
    e("moon", "Moon phase", "World"),
    e("phantoms", "Phantom countdown", "World"),
    e("slime", "Slime chunk", "World"),
    e("spawn_here", "Mobs can spawn here", "World"),
    beta(e("session_time", "Session playtime", "Session")),
    beta(e("session_blocks", "Blocks mined / placed", "Session")),
    beta(e("session_distance", "Distance walked / flown", "Session")),
    beta(e("session_combat", "Deaths / mobs killed", "Session")),
    right(e("target_block", "Targeted block", "Target")),
    right(e("target_entity", "Targeted entity", "Target")),
    right(e("horse", "Horse stats", "Target")),
    right(e("villager", "Villager profession", "Target")),
    right(e("crop", "Crop growth", "Target")),
    right(e("fill", "Bee nest / composter level", "Target")),
    right(e("redstone", "Redstone power", "Target")),
];

/// The entries of this build (`Entries.ALL`).
pub fn entries(beta: bool) -> impl Iterator<Item = &'static EntryInfo> {
    ENTRIES.iter().filter(move |e| beta || !e.beta)
}

/// Java's `Entries.byId`.
pub fn entry(id: &str, beta: bool) -> Option<&'static EntryInfo> {
    entries(beta).find(|e| e.id == id)
}

// ------------------------------------------------------------ layout

/// Java's `Layouts.Layout`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub order: Vec<String>,
    pub vis: HashMap<String, Visibility>,
    pub right: BTreeSet<String>,
    /// HUD element id of another module → visibility (missing = Always).
    pub hud: BTreeMap<String, Visibility>,
    /// Entry objects this build doesn't know, written back at the end.
    pub unknown: Vec<Json>,
}

impl Layout {
    pub fn vis(&self, id: &str) -> Visibility {
        self.vis.get(id).copied().unwrap_or(Visibility::Overlay)
    }

    pub fn hud(&self, id: &str) -> Visibility {
        self.hud.get(id).copied().unwrap_or(Visibility::Always)
    }

    /// Java's `normalize`: drops unknown ids from `order`, appends the
    /// missing entries (In overlay, default column).
    pub fn normalize(&mut self, beta: bool) {
        self.order.retain(|id| entry(id, beta).is_some());
        for info in entries(beta) {
            if !self.order.iter().any(|id| id == info.id) {
                self.order.push(info.id.to_owned());
                self.vis
                    .entry(info.id.to_owned())
                    .or_insert(Visibility::Overlay);
                if info.right {
                    self.right.insert(info.id.to_owned());
                }
            }
        }
    }

    /// Java's `Layouts.preset`.
    pub fn preset(name: &str, beta: bool) -> Layout {
        let mut l = Layout::default();
        l.normalize(beta);
        let always: &[&str] = match name {
            "Build" => &["coords", "facing", "light", "target_block", "redstone"],
            "Farm" => &["time", "spawn_here", "crop", "fill", "villager", "horse"],
            _ => &[],
        };
        for id in always {
            l.vis.insert((*id).to_owned(), Visibility::Always);
        }
        l
    }

    /// Swaps `id` with the next entry of its column in direction `dir`.
    pub fn move_entry(&mut self, id: &str, dir: i32) {
        let Some(i) = self.order.iter().position(|x| x == id) else {
            return;
        };
        let col = self.right.contains(id);
        let mut j = i as i32 + dir;
        while j >= 0 && (j as usize) < self.order.len() {
            if self.right.contains(&self.order[j as usize]) == col {
                self.order.swap(i, j as usize);
                return;
            }
            j += dir;
        }
    }

    /// The column switch of the layout screen.
    pub fn switch_column(&mut self, id: &str) {
        if !self.right.remove(id) {
            self.right.insert(id.to_owned());
        }
    }

    /// Java's `parse`: Err where Java throws (an entry without `id`, wrong
    /// shapes), which makes the whole file count as broken.
    pub fn from_json(json: &Json, beta: bool) -> Result<Layout, String> {
        let obj = json.as_obj().ok_or("layout is not an object")?;
        let mut l = Layout::default();
        if let Some(entries) = obj.get("entries") {
            let Json::Arr(entries) = entries else {
                return Err("entries is not an array".into());
            };
            for raw in entries {
                let o = raw.as_obj().ok_or("entry is not an object")?;
                let id = o
                    .get("id")
                    .and_then(Json::as_string)
                    .ok_or("entry without id")?;
                if entry(&id, beta).is_none() {
                    l.unknown.push(raw.clone());
                    continue;
                }
                if l.order.contains(&id) {
                    continue;
                }
                let vis = o
                    .get("vis")
                    .and_then(Json::as_string)
                    .map_or(Visibility::Overlay, |s| {
                        Visibility::parse(&s, Visibility::Overlay)
                    });
                l.vis.insert(id.clone(), vis);
                if o.get("right").and_then(Json::as_bool) == Some(true) {
                    l.right.insert(id.clone());
                }
                l.order.push(id);
            }
        }
        if let Some(hud) = obj.get("hud") {
            let hud = hud.as_obj().ok_or("hud is not an object")?;
            for (k, v) in hud.iter() {
                let vis = v.as_string().map_or(Visibility::Always, |s| {
                    Visibility::parse(&s, Visibility::Always)
                });
                l.hud.insert(k.to_owned(), vis);
            }
        }
        Ok(l)
    }

    pub fn to_json(&self) -> Json {
        let mut list = Vec::new();
        for id in &self.order {
            let mut o = Obj::new();
            o.insert("id", Json::str(id));
            o.insert("vis", Json::str(self.vis(id).name()));
            o.insert("right", Json::Bool(self.right.contains(id)));
            list.push(Json::Obj(o));
        }
        list.extend(self.unknown.iter().cloned());
        let mut hud = Obj::new();
        for (k, v) in &self.hud {
            hud.insert(k, Json::str(v.name()));
        }
        let mut o = Obj::new();
        o.insert("entries", Json::Arr(list));
        o.insert("hud", Json::Obj(hud));
        Json::Obj(o)
    }
}

/// A remembered bed (`dim` = short dimension path).
#[derive(Clone, Debug, PartialEq)]
pub struct Bed {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub dim: String,
}

/// Java's `Layouts`: the named layouts in insertion order, the active name
/// (the setting `layout` is the source; kept here only as a lookup key by the
/// callers) and the beds per world.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layouts {
    pub list: Vec<(String, Layout)>,
    pub beds: BTreeMap<String, Bed>,
    pub beta: bool,
}

pub const DEFAULT: &str = "Default";

impl Layouts {
    /// `Default`, `Build`, `Farm` from the presets.
    pub fn presets(beta: bool) -> Layouts {
        Layouts {
            list: [DEFAULT, "Build", "Farm"]
                .iter()
                .map(|n| ((*n).to_owned(), Layout::preset(n, beta)))
                .collect(),
            beds: BTreeMap::new(),
            beta,
        }
    }

    pub fn names(&self) -> Vec<&str> {
        self.list.iter().map(|(n, _)| n.as_str()).collect()
    }

    fn index(&self, name: &str) -> Option<usize> {
        self.list.iter().position(|(n, _)| n == name)
    }

    /// `currentName()`: `name` if it exists, else the first layout.
    pub fn current_name(&self, name: &str) -> String {
        if self.index(name).is_some() {
            name.to_owned()
        } else {
            self.list
                .first()
                .map_or(DEFAULT.to_owned(), |l| l.0.clone())
        }
    }

    pub fn current(&self, name: &str) -> Option<&Layout> {
        let i = self
            .index(name)
            .or(if self.list.is_empty() { None } else { Some(0) })?;
        Some(&self.list[i].1)
    }

    pub fn current_mut(&mut self, name: &str) -> Option<&mut Layout> {
        let i = self
            .index(name)
            .or(if self.list.is_empty() { None } else { Some(0) })?;
        Some(&mut self.list[i].1)
    }

    /// `next()`: the name after the current one, wrapping.
    pub fn next(&self, name: &str) -> String {
        let cur = self.current_name(name);
        let i = self.index(&cur).unwrap_or(0);
        self.list[(i + 1) % self.list.len()].0.clone()
    }

    /// `create`: a copy of the current layout under a new name; the stored name.
    pub fn create(&mut self, current: &str, name: &str) -> Option<String> {
        let name = name.trim();
        if name.is_empty() || self.index(name).is_some() {
            return None;
        }
        let copy = self.current(current)?.clone();
        self.list.push((name.to_owned(), copy));
        Some(name.to_owned())
    }

    /// `rename`: the current layout keeps its position.
    pub fn rename(&mut self, current: &str, new: &str) -> Option<String> {
        let to = new.trim();
        if to.is_empty() || self.index(to).is_some() {
            return None;
        }
        let i = self.index(&self.current_name(current))?;
        self.list[i].0 = to.to_owned();
        Some(to.to_owned())
    }

    /// `delete`: the current layout, unless it is the only one; the name to
    /// select (the first).
    pub fn delete(&mut self, current: &str) -> Option<String> {
        if self.list.len() <= 1 {
            return None;
        }
        let i = self.index(&self.current_name(current))?;
        self.list.remove(i);
        Some(self.list[0].0.clone())
    }

    /// `reset`: the current layout back to its preset.
    pub fn reset(&mut self, current: &str) {
        let name = self.current_name(current);
        if let Some(i) = self.index(&name) {
            self.list[i].1 = Layout::preset(&name, self.beta);
        }
    }

    /// Java's `serializeLayouts` (no beds).
    pub fn layouts_json(&self) -> Obj {
        let mut o = Obj::new();
        for (n, l) in &self.list {
            o.insert(n, l.to_json());
        }
        o
    }

    /// Java's `importLayouts`: replaces the same names, keeps the others.
    pub fn import(&mut self, json: &Obj) -> Result<(), String> {
        let mut incoming = Vec::new();
        for (name, l) in json.iter() {
            let mut layout = Layout::from_json(l, self.beta)?;
            layout.normalize(self.beta);
            incoming.push((name.to_owned(), layout));
        }
        for (name, layout) in incoming {
            match self.index(&name) {
                Some(i) => self.list[i].1 = layout,
                None => self.list.push((name, layout)),
            }
        }
        Ok(())
    }

    /// Java's `Layouts.load` without the Info HUD migration: Err = the file
    /// was broken (the caller sets it aside).
    pub fn from_json(root: &Obj, beta: bool) -> Result<Layouts, String> {
        let mut out = Layouts {
            list: Vec::new(),
            beds: BTreeMap::new(),
            beta,
        };
        if let Some(layouts) = root.get("layouts") {
            let layouts = layouts.as_obj().ok_or("layouts is not an object")?;
            for (name, l) in layouts.iter() {
                out.list
                    .push((name.to_owned(), Layout::from_json(l, beta)?));
            }
        }
        if let Some(beds) = root.get("beds") {
            let beds = beds.as_obj().ok_or("beds is not an object")?;
            for (key, b) in beds.iter() {
                let b = b.as_obj().ok_or("bed is not an object")?;
                let n = |k: &str| b.get(k).and_then(Json::as_int).unwrap_or(0);
                out.beds.insert(
                    key.to_owned(),
                    Bed {
                        x: n("x"),
                        y: n("y"),
                        z: n("z"),
                        dim: b.get("dim").and_then(Json::as_string).unwrap_or_default(),
                    },
                );
            }
        }
        Ok(out)
    }

    pub fn to_json(&self) -> Obj {
        let mut beds = Obj::new();
        for (k, b) in &self.beds {
            let mut o = Obj::new();
            o.insert("x", Json::Int(b.x.into()));
            o.insert("y", Json::Int(b.y.into()));
            o.insert("z", Json::Int(b.z.into()));
            o.insert("dim", Json::str(&b.dim));
            beds.insert(k, Json::Obj(o));
        }
        let mut root = Obj::new();
        root.insert("layouts", Json::Obj(self.layouts_json()));
        root.insert("beds", Json::Obj(beds));
        root
    }

    /// Java's `Layouts.load`: the file, or the presets when it is missing,
    /// broken (set aside) or has no layouts; every layout normalized.
    pub fn load(file: Option<&Path>, beta: bool) -> Layouts {
        let root = file.and_then(config::read);
        let mut layouts = match root.as_ref().map(|r| Layouts::from_json(r, beta)) {
            Some(Ok(l)) => l,
            Some(Err(e)) => {
                let moved = file.and_then(|f| config::set_aside(f, false));
                config::log(&format!(
                    "[Omnitool] Broken overlay.json, using default layouts (old file kept as {moved:?}): {e}"
                ));
                Layouts::presets(beta)
            }
            None => Layouts::presets(beta),
        };
        if layouts.list.is_empty() {
            layouts.list = Layouts::presets(beta).list;
        }
        for (_, l) in &mut layouts.list {
            l.normalize(beta);
        }
        if root.is_none() || layouts.list.is_empty() {
            layouts.save(file);
        }
        layouts
    }

    pub fn save(&self, file: Option<&Path>) {
        if let Some(f) = file {
            config::write(f, &self.to_json());
        }
    }
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

    fn vis(name: &str) -> Visibility {
        Visibility::parse(name, Visibility::Overlay)
    }

    fn strings(v: &Value) -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn entry_lists() {
        let f = fixtures();
        let stable: Vec<_> = entries(false).collect();
        let want = f["entries_stable"].as_array().unwrap();
        assert_eq!(stable.len(), want.len());
        for (e, w) in stable.iter().zip(want) {
            assert_eq!(e.id, w["id"].as_str().unwrap());
            assert_eq!(e.right, w["right"].as_bool().unwrap());
        }
        let beta: Vec<_> = ENTRIES.iter().filter(|e| e.beta).map(|e| e.id).collect();
        assert_eq!(beta, strings(&f["entries_beta_only"]));
        assert_eq!(ENTRIES.len(), 32);
    }

    #[test]
    fn visibility() {
        let f = fixtures();
        for c in f["visibility_next"].as_array().unwrap() {
            let i = &c["in"];
            let got = vis(i["vis"].as_str().unwrap()).next(i["dir"].as_i64().unwrap() as i32);
            assert_eq!(got.name(), c["out"].as_str().unwrap());
        }
        for c in f["visibility_shows"].as_array().unwrap() {
            let got = vis(c["vis"].as_str().unwrap()).shows(c["open"].as_bool().unwrap());
            assert_eq!(got, c["out"].as_bool().unwrap());
        }
        for c in f["visibility_parse"].as_array().unwrap() {
            let got = Visibility::parse(
                c["in"].as_str().unwrap(),
                vis(c["fallback"].as_str().unwrap()),
            );
            assert_eq!(got.name(), c["out"].as_str().unwrap());
        }
    }

    #[test]
    fn normalize_fixture() {
        let f = &fixtures()["layout_normalize"];
        let beta = f["beta"].as_bool().unwrap();
        let json = Json::parse(&f["saved"].to_string()).unwrap();
        let mut l = Layout::from_json(&json, beta).unwrap();
        assert_eq!(l.unknown.len(), 1);
        l.normalize(beta);
        let want = strings(&f["out_order_prefix"]);
        assert_eq!(l.order[..want.len()], want[..]);
        assert_eq!(l.order.len(), f["out_order_len"].as_u64().unwrap() as usize);
        assert_eq!(
            l.right.iter().cloned().collect::<Vec<_>>(),
            strings(&f["out_right"])
        );
        for (id, v) in f["out_vis"].as_object().unwrap() {
            assert_eq!(l.vis(id).name(), v.as_str().unwrap(), "{id}");
        }
        // unknown entries come back at the end
        let Json::Arr(out) = l
            .to_json()
            .as_obj()
            .unwrap()
            .get("entries")
            .unwrap()
            .clone()
        else {
            panic!()
        };
        assert_eq!(out.len(), 27);
        assert_eq!(
            out.last()
                .unwrap()
                .as_obj()
                .unwrap()
                .get("id")
                .unwrap()
                .as_string()
                .unwrap(),
            "gone"
        );
    }

    #[test]
    fn move_fixture() {
        for c in fixtures()["layout_move"].as_array().unwrap() {
            let i = &c["in"];
            let mut l = Layout {
                order: strings(&i["order"]),
                right: strings(&i["right"]).into_iter().collect(),
                ..Layout::default()
            };
            l.move_entry(i["id"].as_str().unwrap(), i["dir"].as_i64().unwrap() as i32);
            assert_eq!(l.order, strings(&c["out"]), "{i}");
        }
    }

    #[test]
    fn presets_fixture() {
        let f = &fixtures()["presets"];
        for name in ["Default", "Build", "Farm", "Mine"] {
            let want = match name {
                "Build" | "Farm" => strings(&f[name]),
                _ => Vec::new(),
            };
            let l = Layout::preset(name, false);
            let always: Vec<_> = l
                .order
                .iter()
                .filter(|id| l.vis(id) == Visibility::Always)
                .cloned()
                .collect();
            let mut want_sorted = want.clone();
            want_sorted.sort();
            let mut got = always.clone();
            got.sort();
            assert_eq!(got, want_sorted, "{name}");
            assert!(l.hud.is_empty());
        }
    }

    #[test]
    fn cycle_fixture() {
        let f = &fixtures()["layout_cycle"];
        let mut ls = Layouts::presets(false);
        assert_eq!(ls.names(), strings(&f["names"]));
        for c in f["cases"].as_array().unwrap() {
            assert_eq!(
                ls.next(c["current"].as_str().unwrap()),
                c["out"].as_str().unwrap()
            );
        }
        ls.list.truncate(1);
        assert_eq!(ls.next("Default"), "Default");
    }

    #[test]
    fn names_fixture() {
        for c in fixtures()["layout_names"].as_array().unwrap() {
            let mut ls = Layouts::default();
            for n in c["existing"].as_array().unwrap() {
                ls.list
                    .push((n.as_str().unwrap().to_owned(), Layout::default()));
            }
            let name = c["name"].as_str();
            match c["op"].as_str().unwrap() {
                "create" => {
                    let got = ls.create("Default", name.unwrap());
                    assert_eq!(got.is_some(), c["ok"].as_bool().unwrap());
                    if let Some(stored) = c.get("stored") {
                        assert_eq!(got.as_deref(), stored.as_str());
                    }
                }
                "rename" => {
                    let got = ls.rename(c["current"].as_str().unwrap(), name.unwrap());
                    assert_eq!(got.is_some(), c["ok"].as_bool().unwrap());
                    if let Some(order) = c.get("order_after") {
                        assert_eq!(ls.names(), strings(order));
                    }
                }
                _ => {
                    let before = ls.names().len();
                    let got = ls.delete(c["current"].as_str().unwrap());
                    assert_eq!(got.is_some(), c["changed"].as_bool().unwrap());
                    assert_eq!(ls.names().len() != before, got.is_some());
                }
            }
        }
    }

    #[test]
    fn delete_selects_first_and_reset_restores_preset() {
        let mut ls = Layouts::presets(false);
        ls.current_mut("Build")
            .unwrap()
            .vis
            .insert("fps".into(), Visibility::Never);
        ls.reset("Build");
        assert_eq!(ls.current("Build"), Some(&Layout::preset("Build", false)));
        assert_eq!(ls.delete("Farm").as_deref(), Some("Default"));
        assert_eq!(ls.names(), ["Default", "Build"]);
    }

    #[test]
    fn file_round_trip_and_broken_file() {
        let dir = config::tests::temp_dir();
        let file = dir.join("overlay.json");
        let mut ls = Layouts::load(Some(&file), false);
        assert!(file.exists());
        ls.current_mut("Default")
            .unwrap()
            .hud
            .insert("armor".into(), Visibility::Never);
        ls.beds.insert(
            "world:A".into(),
            Bed {
                x: 1,
                y: 64,
                z: 2,
                dim: "overworld".into(),
            },
        );
        ls.save(Some(&file));
        let back = Layouts::load(Some(&file), false);
        assert_eq!(back, ls);
        std::fs::write(
            &file,
            r#"{"layouts":{"Default":{"entries":[{"vis":"ALWAYS"}]}}}"#,
        )
        .unwrap();
        let fresh = Layouts::load(Some(&file), false);
        assert_eq!(fresh.names(), ["Default", "Build", "Farm"]);
    }

    #[test]
    fn import_replaces_same_names() {
        let mut ls = Layouts::presets(false);
        let mut src = Layouts::presets(false);
        src.list.truncate(1);
        src.list[0].1.vis.insert("fps".into(), Visibility::Never);
        src.list.push(("Extra".into(), Layout::default()));
        ls.import(&src.layouts_json()).unwrap();
        assert_eq!(ls.names(), ["Default", "Build", "Farm", "Extra"]);
        assert_eq!(ls.current("Default").unwrap().vis("fps"), Visibility::Never);
        assert_eq!(ls.current("Extra").unwrap().order.len(), 26);
    }
}
