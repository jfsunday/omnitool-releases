// Share codes as Java's core/ShareCode.java writes them: `OMNI1:` +
// base64url without padding of gzip(compact JSON). The bundle has std only,
// so base64, gzip, inflate and a small deflate (LZ77 + fixed Huffman) are
// here. Gzip bytes differ from Java's; both read each other's codes.
// What a code holds (collect, preview, apply) is in core.rs. Added to the
// game as `game::omnitool_share`; also a module of this crate.

use super::omnitool_json::{Json, Obj};

pub const PREFIX: &str = "OMNI1:";
/// Java's `MAX_DECODED` (4 MiB of JSON).
pub const MAX_DECODED: usize = 4 << 20;

/// module id -> setting ids that never leave this computer (Java's `PRIVATE`).
pub const PRIVATE: &[(&str, &[&str])] = &[
    ("chunk_borders", &["seed"]),
    ("chat_heads", &["name_aliases"]),
];

pub fn encode(data: &Json) -> String {
    let json = data.compact();
    format!("{PREFIX}{}", base64url(&gzip(json.as_bytes())))
}

/// Java's `decode`: the object, or the (English) message to show.
pub fn decode(code: Option<&str>) -> Result<Obj, &'static str> {
    let Some(code) = code else {
        return Err("Clipboard is empty");
    };
    let s: String = code.split_whitespace().collect();
    let Some(i) = s.find(PREFIX) else {
        return Err("No Omnitool share code (must start with OMNI1:)");
    };
    const DAMAGED: &str = "Share code is damaged or incomplete";
    let raw = unbase64url(&s[i + PREFIX.len()..]).ok_or(DAMAGED)?;
    let json = match gunzip(&raw, MAX_DECODED + 1) {
        Some(json) if json.len() > MAX_DECODED => return Err("Share code is too big"),
        Some(json) => json,
        None => return Err(DAMAGED),
    };
    let text = String::from_utf8_lossy(&json);
    match Json::parse(&text) {
        Ok(Json::Obj(o)) => Ok(o),
        // Gson reads "" as JsonNull: not an object
        Ok(_) => Err("Share code is damaged"),
        Err(_) if text.trim().is_empty() => Err("Share code is damaged"),
        Err(_) => Err(DAMAGED),
    }
}

/// Removes private settings from a modules object (Java's `stripPrivate`).
pub fn strip_private(modules: &mut Obj) {
    for (module, ids) in PRIVATE {
        if let Some(settings) = modules.obj_mut(module).and_then(|m| m.obj_mut("settings")) {
            for id in *ids {
                settings.remove(id);
            }
        }
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub fn base64url(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// Java's strict URL decoder: only `A-Za-z0-9-_`, padding optional.
pub fn unbase64url(s: &str) -> Option<Vec<u8>> {
    let s = s
        .strip_suffix("==")
        .or_else(|| s.strip_suffix('='))
        .unwrap_or(s);
    if s.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let v = B64.iter().position(|&b| b == c)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

pub fn gzip(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
    out.extend(deflate(data));
    out.extend(crc32(data).to_le_bytes());
    out.extend((data.len() as u32).to_le_bytes());
    out
}

/// The first gzip member, at most `limit` bytes of it (Java's `readNBytes`:
/// at the limit the rest is not checked).
pub fn gunzip(data: &[u8], limit: usize) -> Option<Vec<u8>> {
    gunzip_member(data, limit).ok()
}

fn gunzip_member(data: &[u8], limit: usize) -> Result<Vec<u8>, ()> {
    if data.len() < 18 || data[0] != 0x1f || data[1] != 0x8b || data[2] != 8 {
        return Err(());
    }
    let flags = data[3];
    let mut i = 10;
    if flags & 4 != 0 {
        let n = u16::from_le_bytes([*data.get(i).ok_or(())?, *data.get(i + 1).ok_or(())?]) as usize;
        i += 2 + n;
    }
    for flag in [8, 16] {
        if flags & flag != 0 {
            i += data
                .get(i..)
                .ok_or(())?
                .iter()
                .position(|&b| b == 0)
                .ok_or(())?
                + 1;
        }
    }
    if flags & 2 != 0 {
        i += 2;
    }
    let mut inf = Inflate {
        data: data.get(i..).ok_or(())?,
        pos: 0,
        bit: 0,
        out: Vec::new(),
        limit,
    };
    inf.run()?;
    if inf.out.len() >= limit {
        return Ok(inf.out);
    }
    let end = i + inf.pos + usize::from(inf.bit > 0);
    let trailer = data.get(end..end + 8).ok_or(())?;
    let crc = u32::from_le_bytes(trailer[0..4].try_into().map_err(|_| ())?);
    let size = u32::from_le_bytes(trailer[4..8].try_into().map_err(|_| ())?);
    if crc != crc32(&inf.out) || size != inf.out.len() as u32 {
        return Err(());
    }
    Ok(inf.out)
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// A canonical Huffman table: count per length, symbols by code.
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Huffman, ()> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[l as usize] += 1;
        }
        counts[0] = 0;
        let mut offs = [0u16; 16];
        for l in 1..15 {
            offs[l + 1] = offs[l] + counts[l];
        }
        let mut symbols = vec![0; lengths.len()];
        for (s, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        Ok(Huffman { counts, symbols })
    }
}

struct Inflate<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
    out: Vec<u8>,
    limit: usize,
}

impl Inflate<'_> {
    fn bits(&mut self, n: u32) -> Result<u32, ()> {
        let mut v = 0;
        for i in 0..n {
            let byte = *self.data.get(self.pos).ok_or(())?;
            v |= ((byte >> self.bit) as u32 & 1) << i;
            self.bit += 1;
            if self.bit == 8 {
                self.bit = 0;
                self.pos += 1;
            }
        }
        Ok(v)
    }

    fn decode(&mut self, h: &Huffman) -> Result<u16, ()> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= self.bits(1)? as i32;
            let count = h.counts[len] as i32;
            if code - count < first {
                return h
                    .symbols
                    .get((index + code - first) as usize)
                    .copied()
                    .ok_or(());
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(())
    }

    fn run(&mut self) -> Result<(), ()> {
        loop {
            let last = self.bits(1)?;
            match self.bits(2)? {
                0 => {
                    if self.bit > 0 {
                        self.bit = 0;
                        self.pos += 1;
                    }
                    let h = self.data.get(self.pos..self.pos + 4).ok_or(())?;
                    let len = u16::from_le_bytes([h[0], h[1]]);
                    if len != !u16::from_le_bytes([h[2], h[3]]) {
                        return Err(());
                    }
                    self.pos += 4;
                    let block = self.data.get(self.pos..self.pos + len as usize).ok_or(())?;
                    self.out.extend_from_slice(block);
                    self.pos += len as usize;
                }
                1 => {
                    let mut l = [8u8; 288];
                    l[144..256].fill(9);
                    l[256..280].fill(7);
                    self.codes(&Huffman::new(&l)?, &Huffman::new(&[5; 30])?)?;
                }
                2 => {
                    let (lit, dist) = self.dynamic()?;
                    self.codes(&lit, &dist)?;
                }
                _ => return Err(()),
            }
            if last == 1 || self.out.len() >= self.limit {
                return Ok(());
            }
        }
    }

    fn dynamic(&mut self) -> Result<(Huffman, Huffman), ()> {
        let nlen = self.bits(5)? as usize + 257;
        let ndist = self.bits(5)? as usize + 1;
        let ncode = self.bits(4)? as usize + 4;
        const ORDER: [usize; 19] = [
            16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
        ];
        let mut cl = [0u8; 19];
        for &o in &ORDER[..ncode] {
            cl[o] = self.bits(3)? as u8;
        }
        let ch = Huffman::new(&cl)?;
        let mut lengths = Vec::with_capacity(nlen + ndist);
        while lengths.len() < nlen + ndist {
            let sym = self.decode(&ch)?;
            let (value, n) = match sym {
                0..=15 => (sym as u8, 1),
                16 => (*lengths.last().ok_or(())?, 3 + self.bits(2)?),
                17 => (0, 3 + self.bits(3)?),
                _ => (0, 11 + self.bits(7)?),
            };
            for _ in 0..n {
                lengths.push(value);
            }
        }
        if lengths.len() != nlen + ndist {
            return Err(());
        }
        Ok((
            Huffman::new(&lengths[..nlen])?,
            Huffman::new(&lengths[nlen..])?,
        ))
    }

    fn codes(&mut self, lit: &Huffman, dist: &Huffman) -> Result<(), ()> {
        loop {
            if self.out.len() >= self.limit {
                return Ok(());
            }
            let sym = self.decode(lit)? as usize;
            match sym {
                0..=255 => self.out.push(sym as u8),
                256 => return Ok(()),
                _ => {
                    let s = sym - 257;
                    if s >= 29 {
                        return Err(());
                    }
                    let len = LEN_BASE[s] as usize + self.bits(LEN_EXTRA[s] as u32)? as usize;
                    let d = self.decode(dist)? as usize;
                    if d >= 30 {
                        return Err(());
                    }
                    let back = DIST_BASE[d] as usize + self.bits(DIST_EXTRA[d] as u32)? as usize;
                    if back > self.out.len() {
                        return Err(());
                    }
                    let start = self.out.len() - back;
                    for k in 0..len {
                        self.out.push(self.out[start + k]);
                    }
                }
            }
        }
    }
}

/// LSB-first bit writer.
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn put(&mut self, v: u32, n: u32) {
        self.acc |= (v as u64) << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// A Huffman code: written from its most significant bit.
    fn code(&mut self, code: u32, len: u32) {
        self.put(code.reverse_bits() >> (32 - len), len);
    }

    fn literal(&mut self, sym: u32) {
        match sym {
            0..=143 => self.code(0x30 + sym, 8),
            144..=255 => self.code(0x190 + sym - 144, 9),
            256..=279 => self.code(sym - 256, 7),
            _ => self.code(0xC0 + sym - 280, 8),
        }
    }
}

/// One fixed-Huffman block with greedy LZ77 matches (32 KiB window).
pub fn deflate(data: &[u8]) -> Vec<u8> {
    const WINDOW: usize = 32768;
    let mut b = Bits {
        out: Vec::with_capacity(data.len() / 2 + 16),
        acc: 0,
        n: 0,
    };
    b.put(1, 1);
    b.put(1, 2);
    let mut head = vec![usize::MAX; 1 << 15];
    let mut prev = vec![usize::MAX; WINDOW];
    let hash = |i: usize| -> usize {
        ((data[i] as usize) << 10 ^ (data[i + 1] as usize) << 5 ^ data[i + 2] as usize)
            & ((1 << 15) - 1)
    };
    let mut i = 0;
    while i < data.len() {
        let (mut best, mut best_d) = (0, 0);
        if i + 2 < data.len() {
            let mut cand = head[hash(i)];
            let mut tries = 64;
            while cand != usize::MAX && i - cand <= WINDOW && tries > 0 {
                let max = (data.len() - i).min(258);
                let l = (0..max)
                    .take_while(|&k| data[cand + k] == data[i + k])
                    .count();
                if l > best {
                    best = l;
                    best_d = i - cand;
                    if l == max {
                        break;
                    }
                }
                let p = prev[cand % WINDOW];
                if p == usize::MAX || p >= cand {
                    break;
                }
                cand = p;
                tries -= 1;
            }
        }
        if best >= 3 {
            let s = LEN_BASE
                .iter()
                .rposition(|&l| l as usize <= best)
                .unwrap_or(0);
            b.literal(257 + s as u32);
            b.put((best - LEN_BASE[s] as usize) as u32, LEN_EXTRA[s] as u32);
            let d = DIST_BASE
                .iter()
                .rposition(|&x| x as usize <= best_d)
                .unwrap_or(0);
            b.code(d as u32, 5);
            b.put(
                (best_d - DIST_BASE[d] as usize) as u32,
                DIST_EXTRA[d] as u32,
            );
            for k in i..i + best {
                if k + 2 < data.len() {
                    prev[k % WINDOW] = head[hash(k)];
                    head[hash(k)] = k;
                }
            }
            i += best;
        } else {
            b.literal(data[i] as u32);
            if i + 2 < data.len() {
                prev[i % WINDOW] = head[hash(i)];
                head[hash(i)] = i;
            }
            i += 1;
        }
    }
    b.literal(256);
    if b.n > 0 {
        b.out.push(b.acc as u8);
    }
    b.out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_json::Json;

    fn fixture() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/core-share-code.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn ours(v: &serde_json::Value) -> Json {
        Json::parse(&v.to_string()).unwrap()
    }

    #[test]
    fn decode_fixture_codes() {
        let f = fixture();
        assert_eq!(f["prefix"], PREFIX);
        assert_eq!(f["max_decoded_bytes"], MAX_DECODED);
        for case in f["decode"].as_array().unwrap() {
            let got = decode(case["code"].as_str());
            match case.get("expected") {
                Some(e) => assert_eq!(got.map(Json::Obj), Ok(ours(e)), "{}", case["name"]),
                None => assert_eq!(got.err(), case["error"].as_str(), "{}", case["name"]),
            }
        }
    }

    #[test]
    fn roundtrip_fixture_values() {
        for v in fixture()["roundtrip"].as_array().unwrap() {
            let code = encode(&ours(v));
            assert!(code.starts_with(PREFIX));
            assert_eq!(decode(Some(&code)).map(Json::Obj), Ok(ours(v)));
        }
    }

    #[test]
    fn too_big() {
        let f = fixture();
        let mut big = "{\"a\":\"".to_owned();
        big.push_str(&"x".repeat(MAX_DECODED));
        big.push_str("\"}");
        let code = format!("{PREFIX}{}", base64url(&gzip(big.as_bytes())));
        assert_eq!(decode(Some(&code)).err(), f["too_big"]["error"].as_str());
    }

    #[test]
    fn strip_private_fixture() {
        for case in fixture()["strip_private"].as_array().unwrap() {
            let Json::Obj(mut m) = ours(&case["input"]) else {
                panic!()
            };
            strip_private(&mut m);
            assert_eq!(Json::Obj(m), ours(&case["expected"]));
        }
    }

    #[test]
    fn deflate_roundtrips_varied_data() {
        let mut data = Vec::new();
        for i in 0..70_000u32 {
            data.push((i.wrapping_mul(2_654_435_761) >> 13) as u8 % 7 + b'a');
        }
        data.extend_from_slice(&[0; 1000]);
        for d in [&data[..], b"", b"a", b"abcabcabcabcabc"] {
            assert_eq!(gunzip(&gzip(d), usize::MAX).unwrap(), d);
        }
        assert_eq!(base64url(b"\xff\xfe"), "__4");
        assert_eq!(unbase64url("__4"), Some(vec![0xff, 0xfe]));
        assert_eq!(unbase64url("a"), None);
    }
}
