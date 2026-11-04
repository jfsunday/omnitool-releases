// The Schematics module's core (docs/spec/omnitool/schematics.md): NBT and
// `.litematic` reading and writing, the packed palette arrays, the material
// list and `/ot schem load|list|materials`. Placing and rendering come later.
// Added to the game as `game::omnitool_schematics`; also a module of this
// crate, checked against tests/fixtures/schematics-*.json.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_share as share;
use std::cell::RefCell;
use std::path::PathBuf;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "schematics",
    name: "Schematics",
    category: Category::World,
    description: "Load .litematic schematics and list their materials: /ot schem load|list|materials.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "Litematica",
    key: super::omnitool_setting::NONE,
    hold: false,
    settings: &[],
    command: None,
    hud: None,
    tick: None,
    on_key: None,
    hud_right: None,
    on_action: None,
};

// ------------------------------------------------------------------ NBT

#[derive(Clone, Debug, PartialEq)]
pub enum Nbt {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    Str(String),
    List(Vec<Nbt>),
    Compound(Vec<(String, Nbt)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

const MAX_DEPTH: usize = 512;
/// A file unpacks to at most this much (256 MiB).
const MAX_UNPACKED: usize = 256 << 20;

impl Nbt {
    #[allow(dead_code)]
    fn id(&self) -> u8 {
        match self {
            Nbt::Byte(_) => 1,
            Nbt::Short(_) => 2,
            Nbt::Int(_) => 3,
            Nbt::Long(_) => 4,
            Nbt::Float(_) => 5,
            Nbt::Double(_) => 6,
            Nbt::ByteArray(_) => 7,
            Nbt::Str(_) => 8,
            Nbt::List(_) => 9,
            Nbt::Compound(_) => 10,
            Nbt::IntArray(_) => 11,
            Nbt::LongArray(_) => 12,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Nbt> {
        match self {
            Nbt::Compound(c) => c.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn int(&self, key: &str) -> Option<i32> {
        match self.get(key)? {
            Nbt::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn string(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Nbt::Str(v) => Some(v),
            _ => None,
        }
    }

    pub fn compound(&self, key: &str) -> Option<&Nbt> {
        self.get(key).filter(|v| matches!(v, Nbt::Compound(_)))
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.data.len());
        let end = end.ok_or("truncated NBT")?;
        let s = &self.data[self.at..end];
        self.at = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn len(&mut self) -> Result<usize, String> {
        let n = i32::from_be_bytes(self.take(4)?.try_into().unwrap());
        usize::try_from(n).map_err(|_| "truncated NBT".to_owned())
    }

    fn string(&mut self) -> Result<String, String> {
        let n = u16::from_be_bytes(self.take(2)?.try_into().unwrap());
        Ok(String::from_utf8_lossy(self.take(n as usize)?).into_owned())
    }

    /// An array length that must fit in what is left of the data.
    fn array(&mut self, width: usize) -> Result<usize, String> {
        let n = self.len()?;
        match n.checked_mul(width) {
            Some(bytes) if bytes <= self.data.len() - self.at => Ok(n),
            _ => Err("truncated NBT".to_owned()),
        }
    }

    fn payload(&mut self, id: u8, depth: usize) -> Result<Nbt, String> {
        if depth > MAX_DEPTH {
            return Err("NBT too deep".to_owned());
        }
        Ok(match id {
            1 => Nbt::Byte(self.u8()? as i8),
            2 => Nbt::Short(i16::from_be_bytes(self.take(2)?.try_into().unwrap())),
            3 => Nbt::Int(i32::from_be_bytes(self.take(4)?.try_into().unwrap())),
            4 => Nbt::Long(i64::from_be_bytes(self.take(8)?.try_into().unwrap())),
            5 => Nbt::Float(f32::from_be_bytes(self.take(4)?.try_into().unwrap())),
            6 => Nbt::Double(f64::from_be_bytes(self.take(8)?.try_into().unwrap())),
            7 => {
                let n = self.array(1)?;
                Nbt::ByteArray(self.take(n)?.iter().map(|b| *b as i8).collect())
            }
            8 => Nbt::Str(self.string()?),
            9 => {
                let element = self.u8()?;
                let n = self.len()?;
                if n > 0 && !(1..=12).contains(&element) {
                    return Err(format!("bad tag {element}"));
                }
                let mut list = Vec::new();
                for _ in 0..n {
                    list.push(self.payload(element, depth + 1)?);
                }
                Nbt::List(list)
            }
            10 => {
                let mut entries = Vec::new();
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    if t > 12 {
                        return Err(format!("bad tag {t}"));
                    }
                    let name = self.string()?;
                    entries.push((name, self.payload(t, depth + 1)?));
                }
                Nbt::Compound(entries)
            }
            11 => {
                let n = self.array(4)?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(i32::from_be_bytes(self.take(4)?.try_into().unwrap()));
                }
                Nbt::IntArray(v)
            }
            12 => {
                let n = self.array(8)?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(i64::from_be_bytes(self.take(8)?.try_into().unwrap()));
                }
                Nbt::LongArray(v)
            }
            _ => return Err(format!("bad tag {id}")),
        })
    }
}

/// A root tag (name and compound) from uncompressed NBT bytes.
pub fn parse_nbt(data: &[u8]) -> Result<(String, Nbt), String> {
    let mut r = Reader { data, at: 0 };
    let id = r.u8()?;
    if id != 10 {
        return Err("root is not a compound".to_owned());
    }
    let name = r.string()?;
    Ok((name, r.payload(10, 0)?))
}

#[allow(dead_code)]
fn put_string(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    out.extend((b.len().min(u16::MAX as usize) as u16).to_be_bytes());
    out.extend(&b[..b.len().min(u16::MAX as usize)]);
}

#[allow(dead_code)]
fn put_payload(out: &mut Vec<u8>, v: &Nbt) {
    match v {
        Nbt::Byte(n) => out.push(*n as u8),
        Nbt::Short(n) => out.extend(n.to_be_bytes()),
        Nbt::Int(n) => out.extend(n.to_be_bytes()),
        Nbt::Long(n) => out.extend(n.to_be_bytes()),
        Nbt::Float(n) => out.extend(n.to_be_bytes()),
        Nbt::Double(n) => out.extend(n.to_be_bytes()),
        Nbt::ByteArray(a) => {
            out.extend((a.len() as i32).to_be_bytes());
            out.extend(a.iter().map(|b| *b as u8));
        }
        Nbt::Str(s) => put_string(out, s),
        Nbt::List(l) => {
            out.push(l.first().map_or(0, Nbt::id));
            out.extend((l.len() as i32).to_be_bytes());
            for e in l {
                put_payload(out, e);
            }
        }
        Nbt::Compound(c) => {
            for (k, e) in c {
                out.push(e.id());
                put_string(out, k);
                put_payload(out, e);
            }
            out.push(0);
        }
        Nbt::IntArray(a) => {
            out.extend((a.len() as i32).to_be_bytes());
            for n in a {
                out.extend(n.to_be_bytes());
            }
        }
        Nbt::LongArray(a) => {
            out.extend((a.len() as i32).to_be_bytes());
            for n in a {
                out.extend(n.to_be_bytes());
            }
        }
    }
}

/// Uncompressed NBT bytes of a root compound.
#[allow(dead_code)]
pub fn write_nbt(name: &str, root: &Nbt) -> Vec<u8> {
    let mut out = vec![10];
    put_string(&mut out, name);
    put_payload(&mut out, root);
    out
}

// ------------------------------------------------------- bit packing

/// Bits per palette entry (`max(2, 32 - leading_zeros(size - 1))`).
pub fn palette_bits(palette_size: usize) -> u32 {
    let top = palette_size.saturating_sub(1) as u32;
    (32 - top.leading_zeros()).max(2)
}

/// Number of longs for `count` entries of `bits` bits.
pub fn packed_len(bits: u32, count: usize) -> usize {
    (bits as usize * count).div_ceil(64)
}

/// Entries one after the other, no padding at long borders; entry 0 starts
/// at the lowest bit of the first long.
#[allow(dead_code)]
pub fn pack(values: &[u32], bits: u32) -> Vec<i64> {
    let mut longs = vec![0u64; packed_len(bits, values.len())];
    let mask = (1u64 << bits) - 1;
    for (i, v) in values.iter().enumerate() {
        let start = i * bits as usize;
        let (at, shift) = (start / 64, start % 64);
        let v = u64::from(*v) & mask;
        longs[at] |= v << shift;
        if shift + bits as usize > 64 {
            longs[at + 1] |= v >> (64 - shift);
        }
    }
    longs.into_iter().map(|l| l as i64).collect()
}

/// `count` entries out of the longs; None when the array is too short.
pub fn unpack(longs: &[i64], bits: u32, count: usize) -> Option<Vec<u32>> {
    if longs.len() < packed_len(bits, count) {
        return None;
    }
    let mask = (1u64 << bits) - 1;
    Some(
        (0..count)
            .map(|i| {
                let start = i * bits as usize;
                let (at, shift) = (start / 64, start % 64);
                let mut v = (longs[at] as u64) >> shift;
                if shift + bits as usize > 64 {
                    v |= (longs[at + 1] as u64) << (64 - shift);
                }
                (v & mask) as u32
            })
            .collect(),
    )
}

// -------------------------------------------------------- litematic

#[derive(Clone, Debug, PartialEq)]
pub struct BlockState {
    /// `minecraft:oak_stairs`
    pub name: String,
    pub properties: Vec<(String, String)>,
}

impl BlockState {
    #[allow(dead_code)]
    pub fn new(name: &str) -> BlockState {
        BlockState {
            name: name.to_owned(),
            properties: Vec::new(),
        }
    }

    pub fn property(&self, key: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn is_air(&self) -> bool {
        matches!(
            self.name.strip_prefix("minecraft:").unwrap_or(&self.name),
            "air" | "cave_air" | "void_air"
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub name: String,
    pub position: [i32; 3],
    /// Signed: a negative axis extends from `position` in the negative direction.
    pub size: [i32; 3],
    pub palette: Vec<BlockState>,
    /// Palette index per block, x fastest, then z, then y.
    pub blocks: Vec<u32>,
    /// TileEntities, Entities, ticks ...: kept as read.
    pub extra: Vec<(String, Nbt)>,
}

impl Region {
    #[allow(dead_code)]
    pub fn volume(&self) -> usize {
        self.size
            .iter()
            .map(|s| s.unsigned_abs() as usize)
            .product()
    }

    /// The block at region coordinates `x,y,z` (from the minimum corner);
    /// an index outside the palette is air.
    #[allow(dead_code)]
    pub fn state(&self, x: usize, y: usize, z: usize) -> Option<&BlockState> {
        let (sx, sy, sz) = (
            self.size[0].unsigned_abs() as usize,
            self.size[1].unsigned_abs() as usize,
            self.size[2].unsigned_abs() as usize,
        );
        if x >= sx || y >= sy || z >= sz {
            return None;
        }
        let i = self.blocks.get(block_index(self.size, x, y, z))?;
        self.palette.get(*i as usize)
    }

    /// A region from block states in index order; the palette is built in
    /// order of first appearance with air as id 0.
    #[allow(dead_code)]
    pub fn from_states(
        name: &str,
        position: [i32; 3],
        size: [i32; 3],
        states: &[BlockState],
    ) -> Region {
        let mut palette = vec![BlockState::new("minecraft:air")];
        let mut blocks = Vec::with_capacity(states.len());
        for s in states {
            let i = palette.iter().position(|p| p == s).unwrap_or_else(|| {
                palette.push(s.clone());
                palette.len() - 1
            });
            blocks.push(i as u32);
        }
        Region {
            name: name.to_owned(),
            position,
            size,
            palette,
            blocks,
            extra: Vec::new(),
        }
    }

    #[allow(dead_code)]
    fn air_count(&self) -> usize {
        self.blocks
            .iter()
            .filter(|i| {
                self.palette
                    .get(**i as usize)
                    .is_none_or(BlockState::is_air)
            })
            .count()
    }
}

/// `y · |sx| · |sz| + z · |sx| + x`
#[allow(dead_code)]
pub fn block_index(size: [i32; 3], x: usize, y: usize, z: usize) -> usize {
    let (sx, sz) = (
        size[0].unsigned_abs() as usize,
        size[2].unsigned_abs() as usize,
    );
    y * sx * sz + z * sx + x
}

/// The end corner relative to the region origin: `size - 1`, or `size + 1`
/// for a negative axis (a zero axis gives -1).
#[allow(dead_code)]
pub fn relative_end(size: [i32; 3]) -> [i32; 3] {
    size.map(|s| if s >= 0 { s - 1 } else { s + 1 })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Metadata {
    pub name: String,
    pub author: String,
    pub description: String,
    pub region_count: i32,
    pub total_volume: i32,
    pub total_blocks: i32,
    pub time_created: i64,
    pub time_modified: i64,
    pub enclosing: [i32; 3],
    pub preview: Option<Vec<i32>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Litematic {
    pub version: i32,
    pub sub_version: i32,
    pub data_version: i32,
    pub metadata: Metadata,
    pub regions: Vec<Region>,
}

fn vec3(n: Option<&Nbt>) -> [i32; 3] {
    let get = |k| n.and_then(|n| n.int(k)).unwrap_or(0);
    [get("x"), get("y"), get("z")]
}

#[allow(dead_code)]
fn put_vec3(v: [i32; 3]) -> Nbt {
    Nbt::Compound(vec![
        ("x".into(), Nbt::Int(v[0])),
        ("y".into(), Nbt::Int(v[1])),
        ("z".into(), Nbt::Int(v[2])),
    ])
}

fn read_palette(list: Option<&Nbt>) -> Vec<BlockState> {
    let Some(Nbt::List(items)) = list else {
        return Vec::new();
    };
    items
        .iter()
        .map(|e| {
            let name = e
                .string("Name")
                .or_else(|| e.string("id"))
                .unwrap_or("minecraft:air");
            let props = e
                .compound("Properties")
                .or_else(|| e.compound("properties"));
            let properties = match props {
                Some(Nbt::Compound(c)) => c
                    .iter()
                    .filter_map(|(k, v)| match v {
                        Nbt::Str(s) => Some((k.clone(), s.clone())),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            BlockState {
                name: name.to_owned(),
                properties,
            }
        })
        .collect()
}

#[allow(dead_code)]
fn write_palette(palette: &[BlockState]) -> Nbt {
    Nbt::List(
        palette
            .iter()
            .map(|s| {
                let mut e = vec![("Name".to_owned(), Nbt::Str(s.name.clone()))];
                if !s.properties.is_empty() {
                    let p = s.properties.iter();
                    let p = p.map(|(k, v)| (k.clone(), Nbt::Str(v.clone())));
                    e.push(("Properties".to_owned(), Nbt::Compound(p.collect())));
                }
                Nbt::Compound(e)
            })
            .collect(),
    )
}

fn read_region(name: &str, n: &Nbt) -> Result<Region, String> {
    let position = vec3(n.compound("Position"));
    let size = vec3(n.compound("Size"));
    if size.contains(&0) {
        return Err(format!("region '{name}' has an empty size"));
    }
    let palette = read_palette(n.get("BlockStatePalette"));
    let count = size
        .iter()
        .map(|s| s.unsigned_abs() as usize)
        .product::<usize>();
    let longs: &[i64] = match n.get("BlockStates") {
        Some(Nbt::LongArray(l)) => l,
        _ => &[],
    };
    let short = || format!("region '{name}' BlockStates too short");
    let blocks = unpack(longs, palette_bits(palette.len()), count).ok_or_else(short)?;
    let known = ["Position", "Size", "BlockStatePalette", "BlockStates"];
    let extra = match n {
        Nbt::Compound(c) => c
            .iter()
            .filter(|(k, _)| !known.contains(&k.as_str()))
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    Ok(Region {
        name: name.to_owned(),
        position,
        size,
        palette,
        blocks,
        extra,
    })
}

/// A schematic from uncompressed NBT bytes.
pub fn parse_litematic_nbt(data: &[u8]) -> Result<Litematic, String> {
    let (_, root) = parse_nbt(data)?;
    let version = root.int("Version").ok_or("missing Version")?;
    let Some(Nbt::Compound(regions)) = root.get("Regions") else {
        return Err("missing Regions".to_owned());
    };
    if version < 5 {
        return Err(format!("unsupported version {version} (numeric block ids)"));
    }
    let meta = root.compound("Metadata");
    let text = |k: &str, d: &str| meta.and_then(|m| m.string(k)).unwrap_or(d).to_owned();
    let long = |k: &str| match meta.and_then(|m| m.get(k)) {
        Some(Nbt::Long(v)) => *v,
        _ => -1,
    };
    let int = |k: &str| meta.and_then(|m| m.int(k)).unwrap_or(0);
    let metadata = Metadata {
        name: text("Name", "?"),
        author: text("Author", "?"),
        description: text("Description", ""),
        region_count: int("RegionCount"),
        total_volume: int("TotalVolume"),
        total_blocks: int("TotalBlocks"),
        time_created: long("TimeCreated"),
        time_modified: long("TimeModified"),
        enclosing: vec3(meta.and_then(|m| m.compound("EnclosingSize"))),
        preview: match meta.and_then(|m| m.get("PreviewImageData")) {
            Some(Nbt::IntArray(a)) => Some(a.clone()),
            _ => None,
        },
    };
    let regions = regions
        .iter()
        .map(|(name, r)| read_region(name, r))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Litematic {
        version,
        sub_version: root.int("SubVersion").unwrap_or(0),
        data_version: root.int("MinecraftDataVersion").unwrap_or(1139),
        metadata,
        regions,
    })
}

/// A schematic from the bytes of a `.litematic` file (gzip).
pub fn read_litematic(file: &[u8]) -> Result<Litematic, String> {
    let raw = share::gunzip(file, MAX_UNPACKED).ok_or("not gzip data")?;
    parse_litematic_nbt(&raw)
}

impl Litematic {
    /// A version 7 schematic of these regions with fresh metadata.
    #[allow(dead_code)]
    pub fn new(
        name: &str,
        author: &str,
        data_version: i32,
        regions: Vec<Region>,
        now_ms: i64,
    ) -> Litematic {
        let mut l = Litematic {
            version: 7,
            sub_version: 1,
            data_version,
            metadata: Metadata {
                name: name.to_owned(),
                author: author.to_owned(),
                description: String::new(),
                region_count: 0,
                total_volume: 0,
                total_blocks: 0,
                time_created: now_ms,
                time_modified: now_ms,
                enclosing: [0; 3],
                preview: None,
            },
            regions,
        };
        l.update_metadata();
        l
    }

    /// RegionCount, TotalVolume, TotalBlocks and EnclosingSize from the regions.
    #[allow(dead_code)]
    pub fn update_metadata(&mut self) {
        let m = &mut self.metadata;
        m.region_count = self.regions.len() as i32;
        m.total_volume = self.regions.iter().map(|r| r.volume() as i32).sum();
        m.total_blocks = self
            .regions
            .iter()
            .map(|r| (r.volume() - r.air_count()) as i32)
            .sum();
        let mut lo = [i32::MAX; 3];
        let mut hi = [i32::MIN; 3];
        for r in &self.regions {
            let end = relative_end(r.size);
            for a in 0..3 {
                let (p, e) = (r.position[a], r.position[a] + end[a]);
                lo[a] = lo[a].min(p.min(e));
                hi[a] = hi[a].max(p.max(e));
            }
        }
        m.enclosing = if self.regions.is_empty() {
            [0; 3]
        } else {
            [0, 1, 2].map(|a| hi[a] - lo[a] + 1)
        };
    }

    #[allow(dead_code)]
    pub fn to_nbt(&self) -> Nbt {
        let m = &self.metadata;
        let mut meta = vec![
            ("Name".to_owned(), Nbt::Str(m.name.clone())),
            ("Author".to_owned(), Nbt::Str(m.author.clone())),
            ("Description".to_owned(), Nbt::Str(m.description.clone())),
            ("RegionCount".to_owned(), Nbt::Int(m.region_count)),
            ("TotalVolume".to_owned(), Nbt::Int(m.total_volume)),
            ("TotalBlocks".to_owned(), Nbt::Int(m.total_blocks)),
            ("TimeCreated".to_owned(), Nbt::Long(m.time_created)),
            ("TimeModified".to_owned(), Nbt::Long(m.time_modified)),
            ("EnclosingSize".to_owned(), put_vec3(m.enclosing)),
        ];
        if let Some(p) = &m.preview {
            meta.push(("PreviewImageData".to_owned(), Nbt::IntArray(p.clone())));
        }
        let regions = self.regions.iter().map(|r| {
            let longs = pack(&r.blocks, palette_bits(r.palette.len()));
            let mut c = vec![
                ("Position".to_owned(), put_vec3(r.position)),
                ("Size".to_owned(), put_vec3(r.size)),
                ("BlockStatePalette".to_owned(), write_palette(&r.palette)),
                ("BlockStates".to_owned(), Nbt::LongArray(longs)),
            ];
            c.extend(r.extra.iter().cloned());
            (r.name.clone(), Nbt::Compound(c))
        });
        Nbt::Compound(vec![
            (
                "MinecraftDataVersion".to_owned(),
                Nbt::Int(self.data_version),
            ),
            ("Version".to_owned(), Nbt::Int(self.version)),
            ("SubVersion".to_owned(), Nbt::Int(self.sub_version)),
            ("Metadata".to_owned(), Nbt::Compound(meta)),
            ("Regions".to_owned(), Nbt::Compound(regions.collect())),
        ])
    }

    /// The bytes of a `.litematic` file (gzip).
    #[allow(dead_code)]
    pub fn write(&self) -> Vec<u8> {
        share::gzip(&write_nbt("", &self.to_nbt()))
    }
}

// --------------------------------------------------- material list

/// The item for a block and how many of it one block needs (None: nothing).
pub fn block_item(state: &BlockState) -> Option<(String, u64)> {
    if state.is_air() {
        return None;
    }
    let name = state.name.strip_prefix("minecraft:").unwrap_or(&state.name);
    if state.property("half") == Some("upper") || state.property("part") == Some("head") {
        return None;
    }
    let number = |key: &str| state.property(key).and_then(|v| v.parse::<u64>().ok());
    let mut count = 1;
    let item: String = match name {
        "fire" | "soul_fire" | "nether_portal" | "end_portal" | "end_gateway" | "moving_piston"
        | "piston_head" | "bubble_column" => return None,
        "water" | "lava" => {
            if state.property("level").is_some_and(|l| l != "0") {
                return None;
            }
            format!("{name}_bucket")
        }
        "powder_snow" => "powder_snow_bucket".into(),
        "wall_torch" => "torch".into(),
        "soul_wall_torch" => "soul_torch".into(),
        "redstone_wall_torch" => "redstone_torch".into(),
        "redstone_wire" => "redstone".into(),
        "tripwire" => "string".into(),
        "wheat" => "wheat_seeds".into(),
        "beetroots" => "beetroot_seeds".into(),
        "melon_stem" | "attached_melon_stem" => "melon_seeds".into(),
        "pumpkin_stem" | "attached_pumpkin_stem" => "pumpkin_seeds".into(),
        "potatoes" => "potato".into(),
        "carrots" => "carrot".into(),
        "sweet_berry_bush" => "sweet_berries".into(),
        "cocoa" => "cocoa_beans".into(),
        "bamboo_sapling" => "bamboo".into(),
        "kelp_plant" => "kelp".into(),
        "tall_seagrass" => "seagrass".into(),
        "big_dripleaf_stem" => "big_dripleaf".into(),
        "cave_vines" | "cave_vines_plant" => "glow_berries".into(),
        "twisting_vines_plant" => "twisting_vines".into(),
        "weeping_vines_plant" => "weeping_vines".into(),
        "frosted_ice" => "ice".into(),
        _ if name.starts_with("potted_") => "flower_pot".into(),
        _ => {
            let walls = [
                "_wall_sign",
                "_wall_hanging_sign",
                "_wall_banner",
                "_wall_head",
                "_wall_skull",
                "_wall_fan",
            ];
            let plain = walls.iter().find_map(|w| {
                let prefix = name.strip_suffix(w)?;
                Some(format!("{prefix}{}", w.replacen("_wall", "", 1)))
            });
            plain.unwrap_or_else(|| name.to_owned())
        }
    };
    if state.property("type") == Some("double") {
        count = 2;
    }
    let multi = match name {
        n if n == "candle" || n.ends_with("_candle") => "candles",
        "sea_pickle" => "pickles",
        "turtle_egg" => "eggs",
        "pink_petals" | "wildflowers" => "flower_amount",
        "leaf_litter" => "segment_amount",
        "snow" => "layers",
        _ => "",
    };
    if !multi.is_empty() {
        count = number(multi).unwrap_or(1);
    }
    Some((format!("minecraft:{item}"), count))
}

/// Items needed for the whole schematic, in order of first appearance.
pub fn material_totals(l: &Litematic) -> Vec<(String, u64)> {
    let mut out: Vec<(String, u64)> = Vec::new();
    for r in &l.regions {
        for i in &r.blocks {
            let Some((item, n)) = r.palette.get(*i as usize).and_then(block_item) else {
                continue;
            };
            match out.iter_mut().find(|(k, _)| *k == item) {
                Some(e) => e.1 = e.1.saturating_add(n),
                None => out.push((item, n)),
            }
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct MaterialRow {
    /// `minecraft:stone`
    pub item: String,
    pub total: u64,
    pub missing: u64,
    pub available: u64,
}

fn namespaced(item: &str) -> String {
    if item.contains(':') {
        item.to_owned()
    } else {
        format!("minecraft:{item}")
    }
}

/// Total, Missing and Available per item (the totals times `multiplier`
/// against the inventory slots), most needed first, ties by name.
pub fn material_list(
    totals: &[(String, u64)],
    multiplier: u64,
    inventory: &[(String, u32)],
) -> Vec<MaterialRow> {
    let mut rows: Vec<MaterialRow> = totals
        .iter()
        .map(|(item, n)| {
            let available = inventory
                .iter()
                .filter(|(k, _)| namespaced(k) == *item)
                .map(|(_, c)| u64::from(*c))
                .sum::<u64>();
            let total = n.saturating_mul(multiplier);
            MaterialRow {
                item: item.clone(),
                total,
                missing: total.saturating_sub(available),
                available,
            }
        })
        .collect();
    rows.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.item.cmp(&b.item)));
    rows
}

// ------------------------------------------------------------ /ot schem

const USAGE: &str = "Usage: /ot schem list | load <name> | materials [multiplier] [all]";
const LIST_LIMIT: usize = 40;

struct Loaded {
    file: String,
    schematic: Litematic,
}

thread_local! {
    static LOADED: RefCell<Option<Loaded>> = const { RefCell::new(None) };
}

fn tr_de(german: bool, english: &str) -> String {
    let de = match english {
        "Schematics module is disabled in Omnitool." => {
            "Das Schematics-Modul ist in Omnitool deaktiviert."
        }
        "No schematic loaded (use /ot schem load <name>)" => {
            "Keine Schematic geladen (nutze /ot schem load <name>)"
        }
        "Nothing missing" => "Nichts fehlt",
        "Schematic not found: " => "Schematic nicht gefunden: ",
        "Invalid file name" => "Ungültiger Dateiname",
        USAGE => "Nutzung: /ot schem list | load <name> | materials [Multiplikator] [all]",
        _ => english,
    };
    if german { de } else { english }.to_owned()
}

fn dir(host: &dyn Host) -> Option<PathBuf> {
    host.data_dir().map(|d| d.join("schematics"))
}

fn german(host: &dyn Host) -> bool {
    core::read(host, |s| s.german(host))
}

/// `/ot schem …` (the rest after `schem`).
pub fn ot(host: &mut dyn Host, args: &str) {
    let de = german(host);
    if !core::active(host, &MODULE) {
        let msg = tr_de(de, "Schematics module is disabled in Omnitool.");
        return core::error(host, &msg);
    }
    let args = args.trim();
    let (sub, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
    let rest = rest.trim();
    match sub {
        "list" => list(host, de),
        "load" if !rest.is_empty() => load(host, de, rest),
        "materials" => materials(host, de, rest),
        _ => core::fail(host, &tr_de(de, USAGE)),
    }
}

fn list(host: &mut dyn Host, _de: bool) {
    let Some(dir) = dir(host) else {
        return core::fail(host, "No data folder");
    };
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".litematic").map(str::to_owned))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    if names.is_empty() {
        return core::ok(host, &format!("No schematics in {}", dir.display()));
    }
    for (i, n) in names.iter().enumerate() {
        host.chat(&format!("§7{}. §f{n}", i + 1));
    }
}

fn load(host: &mut dyn Host, de: bool, name: &str) {
    let bad = name.contains(['/', '\\', '\0']) || name.contains("..");
    let Some(dir) = dir(host).filter(|_| !bad) else {
        return core::fail(host, &tr_de(de, "Invalid file name"));
    };
    let file = if name.ends_with(".litematic") {
        name.to_owned()
    } else {
        format!("{name}.litematic")
    };
    let Ok(bytes) = std::fs::read(dir.join(&file)) else {
        let msg = format!("{}{name}", tr_de(de, "Schematic not found: "));
        return core::fail(host, &msg);
    };
    match read_litematic(&bytes) {
        Ok(schematic) => {
            let m = &schematic.metadata;
            let e = m.enclosing;
            let msg = format!(
                "Loaded '{file}' ({}) by {}: {} region(s), {}x{}x{}, {} blocks",
                m.name,
                m.author,
                schematic.regions.len(),
                e[0],
                e[1],
                e[2],
                m.total_blocks
            );
            core::ok(host, &msg);
            LOADED.with(|l| *l.borrow_mut() = Some(Loaded { file, schematic }));
        }
        Err(e) => core::fail(host, &format!("Not a litematic file: {e}")),
    }
}

fn materials(host: &mut dyn Host, de: bool, args: &str) {
    let mut multiplier = 1u64;
    let mut all = false;
    for word in args.split_whitespace() {
        match word {
            "all" => all = true,
            w => match w.parse::<u64>() {
                Ok(n) if (1..=1_000_000).contains(&n) => multiplier = n,
                _ => return core::fail(host, &tr_de(de, USAGE)),
            },
        }
    }
    let inventory = host.inventory().slots;
    let made = LOADED.with(|l| {
        l.borrow().as_ref().map(|l| {
            let rows = material_list(&material_totals(&l.schematic), multiplier, &inventory);
            (l.file.clone(), rows)
        })
    });
    let Some((file, rows)) = made else {
        let msg = tr_de(de, "No schematic loaded (use /ot schem load <name>)");
        return core::fail(host, &msg);
    };
    host.chat(&format!("§6Materials for '{file}' x{multiplier}"));
    let total: u64 = rows.iter().map(|r| r.total).sum();
    let shown: Vec<&MaterialRow> = rows.iter().filter(|r| all || r.missing > 0).collect();
    if shown.is_empty() {
        host.chat(&format!("§a{}", tr_de(de, "Nothing missing")));
    }
    for r in shown.iter().take(LIST_LIMIT) {
        let item = r.item.strip_prefix("minecraft:").unwrap_or(&r.item);
        host.chat(&format!(
            "§f{item} §7total §f{} §7missing §c{} §7have §a{}",
            r.total, r.missing, r.available
        ));
    }
    if shown.len() > LIST_LIMIT {
        host.chat(&format!("§7... and {} more", shown.len() - LIST_LIMIT));
    }
    host.chat(&format!("§7Total: {total} items, {} kinds", rows.len()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::Fake;
    use serde_json::Value;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/../tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }

    fn state(name: &str, props: &Value) -> BlockState {
        BlockState {
            name: name.to_owned(),
            properties: props
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
                .collect(),
        }
    }

    fn ints(v: &Value) -> Vec<i64> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_i64().unwrap())
            .collect()
    }

    #[test]
    fn bits_and_lengths() {
        let f = fixture("schematics-format.json");
        for c in f["bits"].as_array().unwrap() {
            let size = c["palette_size"].as_u64().unwrap() as usize;
            assert_eq!(palette_bits(size) as u64, c["bits"], "{size}");
        }
        for c in f["lengths"].as_array().unwrap() {
            let (b, n) = (
                c["bits"].as_u64().unwrap() as u32,
                c["count"].as_u64().unwrap() as usize,
            );
            assert_eq!(packed_len(b, n) as u64, c["longs"]);
        }
    }

    #[test]
    fn packing_matches_fixtures() {
        let f = fixture("schematics-format.json");
        for c in f["pack"].as_array().unwrap() {
            let bits = c["bits"].as_u64().unwrap() as u32;
            let values: Vec<u32> = ints(&c["values"]).iter().map(|v| *v as u32).collect();
            let longs = ints(&c["longs"]);
            assert_eq!(
                pack(&values, bits),
                longs,
                "pack {bits} bits x{}",
                values.len()
            );
            assert_eq!(
                unpack(&longs, bits, values.len()),
                Some(values),
                "unpack {bits}"
            );
        }
        assert_eq!(unpack(&[0], 5, 13), None);
    }

    #[test]
    fn litematic_fixtures_read_and_round_trip() {
        let f = fixture("schematics-format.json");
        for c in f["litematic"].as_array().unwrap() {
            let name = c["name"].as_str().unwrap();
            let raw = unhex(c["nbt_hex"].as_str().unwrap());
            let l = parse_litematic_nbt(&raw).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(l.metadata.name, c["meta_name"].as_str().unwrap(), "{name}");
            assert_eq!(
                l.data_version as i64,
                c["data_version"].as_i64().unwrap(),
                "{name}"
            );
            let regions = c["regions"].as_array().unwrap();
            assert_eq!(l.regions.len(), regions.len(), "{name}");
            for (r, want) in l.regions.iter().zip(regions) {
                assert_eq!(r.name, want["name"].as_str().unwrap());
                assert_eq!(
                    r.position.map(i64::from).to_vec(),
                    ints(&want["position"]),
                    "{name}"
                );
                assert_eq!(
                    r.size.map(i64::from).to_vec(),
                    ints(&want["size"]),
                    "{name}"
                );
                let blocks: Vec<i64> = r.blocks.iter().map(|b| i64::from(*b)).collect();
                assert_eq!(blocks, ints(&want["blocks"]), "{name}");
                let palette: Vec<BlockState> = want["palette"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| state(p["name"].as_str().unwrap(), &p["properties"]))
                    .collect();
                assert_eq!(r.palette, palette, "{name}");
                let longs = pack(&r.blocks, palette_bits(r.palette.len()));
                assert_eq!(longs.to_vec(), ints(&want["longs"]), "{name}");
            }
            // writing what was read gives the same schematic back
            let mut l2 = l.clone();
            l2.update_metadata();
            assert_eq!(
                l2.metadata.total_blocks as i64,
                c["total_blocks"].as_i64().unwrap(),
                "{name}"
            );
            assert_eq!(
                l2.metadata.total_volume as i64,
                c["total_volume"].as_i64().unwrap(),
                "{name}"
            );
            let again = read_litematic(&l2.write()).unwrap();
            assert_eq!(again, l2, "{name}");
            // the NBT itself is byte-identical
            let (_, root) = parse_nbt(&raw).unwrap();
            assert_eq!(write_nbt("", &root), raw, "{name}");
        }
    }

    #[test]
    fn litematic_errors() {
        let f = fixture("schematics-format.json");
        for c in f["errors"].as_array().unwrap() {
            let raw = unhex(c["nbt_hex"].as_str().unwrap());
            assert_eq!(
                parse_litematic_nbt(&raw).map(|_| ()),
                Err(c["error"].as_str().unwrap().to_owned()),
                "{}",
                c["name"]
            );
        }
        assert_eq!(
            read_litematic(b"hello world, not gzip at all").map(|_| ()),
            Err("not gzip data".into())
        );
    }

    #[test]
    fn index_and_end() {
        let f = fixture("litematica.json");
        for c in f["block_index"].as_array().unwrap() {
            let s = ints(&c["size"]);
            let size = [s[0] as i32, s[1] as i32, s[2] as i32];
            let at = |k: &str| c[k].as_u64().unwrap() as usize;
            assert_eq!(
                block_index(size, at("x"), at("y"), at("z")) as u64,
                c["index"]
            );
        }
        for c in f["relative_end"].as_array().unwrap() {
            let s = ints(&c["size"]);
            let end = relative_end([s[0] as i32, s[1] as i32, s[2] as i32]);
            assert_eq!(end.map(i64::from).to_vec(), ints(&c["relative_end"]));
        }
    }

    #[test]
    fn from_states_builds_palette_and_enclosing_size() {
        let stone = BlockState::new("minecraft:stone");
        let air = BlockState::new("minecraft:air");
        let r = Region::from_states(
            "r",
            [1, 2, 3],
            [2, 1, -2],
            &[stone.clone(), air.clone(), air, stone],
        );
        assert_eq!(r.palette.len(), 2);
        assert_eq!(r.blocks, [1, 0, 0, 1]);
        let l = Litematic::new("n", "a", 4325, vec![r], 5);
        assert_eq!(l.metadata.enclosing, [2, 1, 2]);
        assert_eq!((l.metadata.total_volume, l.metadata.total_blocks), (4, 2));
        let r = &l.regions[0];
        assert_eq!(
            r.state(1, 0, 1).map(|s| s.name.as_str()),
            Some("minecraft:stone")
        );
        assert_eq!(r.state(2, 0, 0), None);
    }

    #[test]
    fn material_fixtures() {
        let f = fixture("schematics-materials.json");
        for c in f["cases"].as_array().unwrap() {
            let name = c["name"].as_str().unwrap();
            let mut states = Vec::new();
            for b in c["blocks"].as_array().unwrap() {
                let s = state(b["name"].as_str().unwrap(), &b["properties"]);
                for _ in 0..b["count"].as_u64().unwrap() {
                    states.push(s.clone());
                }
            }
            let n = states.len() as i32;
            let region = Region::from_states("r", [0; 3], [n, 1, 1], &states);
            let l = Litematic::new("t", "t", 1, vec![region], 0);
            let inv: Vec<(String, u32)> = c["inventory"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    (
                        e[0].as_str().unwrap().to_owned(),
                        e[1].as_u64().unwrap() as u32,
                    )
                })
                .collect();
            let rows = material_list(
                &material_totals(&l),
                c["multiplier"].as_u64().unwrap(),
                &inv,
            );
            let got: Vec<(String, u64, u64, u64)> = rows
                .iter()
                .map(|r| {
                    (
                        r.item.trim_start_matches("minecraft:").to_owned(),
                        r.total,
                        r.missing,
                        r.available,
                    )
                })
                .collect();
            let want: Vec<(String, u64, u64, u64)> = c["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    let n = |i: usize| r[i].as_u64().unwrap();
                    (r[0].as_str().unwrap().to_owned(), n(1), n(2), n(3))
                })
                .collect();
            assert_eq!(got, want, "{name}");
        }
    }

    fn enabled_host(dir: PathBuf) -> Fake {
        let mut h = Fake {
            dir: Some(dir),
            ..Fake::default()
        };
        core::with(&mut h, |s, _| s.state_mut(&MODULE).enabled = true);
        h
    }

    #[test]
    fn commands_load_list_materials() {
        let dir = crate::omnitool_config::tests::temp_dir();
        std::fs::create_dir_all(dir.join("schematics")).unwrap();
        let stone = BlockState::new("minecraft:stone");
        let region = Region::from_states(
            "r",
            [0; 3],
            [3, 1, 1],
            &[stone.clone(), stone.clone(), stone],
        );
        let l = Litematic::new("House", "me", 4325, vec![region], 0);
        std::fs::write(dir.join("schematics/House.litematic"), l.write()).unwrap();
        std::fs::write(dir.join("schematics/bad.litematic"), b"nope").unwrap();

        let mut h = enabled_host(dir.clone());
        h.inventory.slots = vec![("minecraft:stone".to_owned(), 1)];
        ot(&mut h, "materials");
        assert_eq!(
            h.lines.pop().unwrap(),
            "§c[Omnitool] No schematic loaded (use /ot schem load <name>)"
        );
        ot(&mut h, "list");
        assert_eq!(h.lines.split_off(0), ["§71. §fbad", "§72. §fHouse"]);
        ot(&mut h, "load ../x");
        assert_eq!(h.lines.pop().unwrap(), "§c[Omnitool] Invalid file name");
        ot(&mut h, "load nothing");
        assert_eq!(
            h.lines.pop().unwrap(),
            "§c[Omnitool] Schematic not found: nothing"
        );
        ot(&mut h, "load bad");
        assert_eq!(
            h.lines.pop().unwrap(),
            "§c[Omnitool] Not a litematic file: not gzip data"
        );
        h.lines.clear();
        ot(&mut h, "load House");
        assert_eq!(
            h.lines.pop().unwrap(),
            "§6[Omnitool] §fLoaded 'House.litematic' (House) by me: 1 region(s), 3x1x1, 3 blocks"
        );
        ot(&mut h, "materials 2");
        assert_eq!(
            h.lines.split_off(0),
            [
                "§6Materials for 'House.litematic' x2",
                "§fstone §7total §f6 §7missing §c5 §7have §a1",
                "§7Total: 6 items, 1 kinds"
            ]
        );
        ot(&mut h, "materials 0");
        assert_eq!(h.lines.pop().unwrap(), format!("§c[Omnitool] {USAGE}"));
        h.inventory.slots = vec![("stone".to_owned(), 64)];
        h.lines.clear();
        ot(&mut h, "materials");
        assert_eq!(h.lines[1], "§aNothing missing");
        ot(&mut h, "bogus");
        assert_eq!(h.lines.pop().unwrap(), format!("§c[Omnitool] {USAGE}"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn disabled_module_and_german() {
        let mut h = Fake::default();
        ot(&mut h, "list");
        assert_eq!(
            h.lines.pop().unwrap(),
            "§cSchematics module is disabled in Omnitool."
        );
        h.language = Some("de_de".to_owned());
        ot(&mut h, "list");
        assert_eq!(
            h.lines.pop().unwrap(),
            "§cDas Schematics-Modul ist in Omnitool deaktiviert."
        );
    }
}
