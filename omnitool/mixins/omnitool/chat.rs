// The chat backend (docs/spec/omnitool/chat.md, section 6 and 4): sizes with
// units, log segment rotation, filter rules, the spam stacker, log lines and
// the log store. Pure logic; tests/fixtures/chat-pack.json checks it. Java
// writes on a background thread; here the store writes at once (a chat line
// is one small append). Added to the game as `game::omnitool_chat`; also a
// module of this crate.

use super::omnitool_chat_regex::Regex;
use super::omnitool_json::{Json, Obj};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

// ------------------------------------------------------------ ByteSize

pub const KB: u64 = 1024;
pub const MB: u64 = KB * 1024;
pub const MIN: u64 = 64 * KB;

/// `ByteSize.parse`: bytes, or -1 when the text is no size.
pub fn parse_size(text: &str) -> i64 {
    let t = text.trim();
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return -1;
    }
    let mut end = digits;
    let b = t.as_bytes();
    if end + 1 < b.len() && matches!(b[end], b'.' | b',') && b[end + 1].is_ascii_digit() {
        end += 1;
        while end < b.len() && b[end].is_ascii_digit() {
            end += 1;
        }
    }
    let Ok(n) = t[..end].replace(',', ".").parse::<f64>() else {
        return -1;
    };
    let rest = t[end..].trim_start().to_lowercase();
    let (unit, rest) = match rest.chars().next() {
        Some('k') => (KB, &rest[1..]),
        Some('m') => (MB, &rest[1..]),
        Some('g') => (MB * KB, &rest[1..]),
        Some('t') => (MB * MB, &rest[1..]),
        _ => (if rest.contains('b') { 1 } else { MB }, &rest[..]),
    };
    let rest = rest.strip_prefix('i').unwrap_or(rest);
    let rest = rest.strip_prefix('b').unwrap_or(rest);
    if !rest.trim().is_empty() {
        return -1;
    }
    let v = n * unit as f64;
    if v >= i64::MAX as f64 {
        i64::MAX
    } else {
        v as i64
    }
}

/// `ByteSize.parseCap`: at least 64 KB; `fallback` when unreadable.
pub fn parse_cap(text: &str, fallback: u64) -> u64 {
    let v = parse_size(text);
    let v = if v < 0 { fallback } else { v as u64 };
    v.max(MIN)
}

/// `ByteSize.format`: `100 MB`, `1.5 GB`, `0 B`.
pub fn format_size(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < units.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if v == v.floor() {
        format!("{} {}", v as u64, units[u])
    } else {
        format!("{v:.1} {}", units[u])
    }
}

// ------------------------------------------------------------ LogRotation

pub const MAX_SEGMENT: u64 = 64 * MB;
pub const MIN_SEGMENT: u64 = 4 * KB;

pub fn segment_limit(cap: u64) -> u64 {
    (cap / 10).clamp(MIN_SEGMENT, MAX_SEGMENT)
}

pub fn segment_name(day: &str, n: u64) -> String {
    format!("chat-{day}-{n:03}.jsonl")
}

/// `(day, number)` of a segment file name.
fn segment_parts(name: &str) -> Option<(&str, u64)> {
    let rest = name.strip_prefix("chat-")?.strip_suffix(".jsonl")?;
    if rest.len() < 14 || !rest.is_char_boundary(10) {
        return None;
    }
    let (day, num) = (&rest[..10], rest[10..].strip_prefix('-')?);
    let d = day.as_bytes();
    let date_ok = d.iter().enumerate().all(|(i, c)| match i {
        4 | 7 => *c == b'-',
        _ => c.is_ascii_digit(),
    });
    let num_ok = (3..=9).contains(&num.len()) && num.bytes().all(|c| c.is_ascii_digit());
    (date_ok && num_ok).then(|| (day, num.parse().unwrap_or(0)))
}

pub fn is_segment(name: &str) -> bool {
    segment_parts(name).is_some()
}

/// The segments among `names`, oldest first.
pub fn sorted(names: &[String]) -> Vec<String> {
    let mut v: Vec<&String> = names.iter().filter(|n| is_segment(n)).collect();
    v.sort_by_key(|n| segment_parts(n).map(|(d, n)| (d.to_owned(), n)));
    v.into_iter().cloned().collect()
}

/// The name of the next segment for `today`.
pub fn next_segment(existing: &[String], today: &str) -> String {
    let top = existing
        .iter()
        .filter_map(|n| segment_parts(n))
        .filter(|(d, _)| *d == today)
        .map(|(_, n)| n)
        .max()
        .unwrap_or(0);
    segment_name(today, top + 1)
}

pub fn should_rotate(
    current: Option<&str>,
    size: u64,
    next_bytes: u64,
    limit: u64,
    today: &str,
) -> bool {
    let Some(c) = current else { return true };
    match segment_parts(c) {
        Some((d, _)) if d == today => size > 0 && size + next_bytes > limit,
        _ => true,
    }
}

/// The oldest segments to delete so that the closed ones stay in budget.
pub fn to_delete(segments: &[(String, u64)], cap: u64, current: Option<&str>) -> Vec<String> {
    let mut closed: u64 = segments
        .iter()
        .filter(|(n, _)| Some(n.as_str()) != current)
        .map(|s| s.1)
        .sum();
    let budget = cap.saturating_sub(segment_limit(cap));
    let mut out = Vec::new();
    for (name, size) in segments {
        if closed <= budget {
            break;
        }
        if Some(name.as_str()) == current {
            continue;
        }
        out.push(name.clone());
        closed -= size;
    }
    out
}

// ------------------------------------------------------------ ChatFilter

/// `ChatFilter.split`: rules separated by `;` (`\;` is a literal one).
pub fn split(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut it = text.chars().peekable();
    let mut push = |cur: &mut String| {
        if !cur.trim().is_empty() {
            out.push(cur.trim().to_owned());
        }
        cur.clear();
    };
    while let Some(c) = it.next() {
        match c {
            '\\' if it.peek() == Some(&';') => {
                it.next();
                cur.push(';');
            }
            ';' => push(&mut cur),
            c => cur.push(c),
        }
    }
    push(&mut cur);
    out
}

pub fn join(rules: &[String]) -> String {
    rules
        .iter()
        .map(|r| r.trim().replace(';', "\\;"))
        .collect::<Vec<_>>()
        .join("; ")
}

#[derive(Debug)]
pub struct Rule {
    #[allow(dead_code)]
    pub source: String,
    server: Option<String>,
    plain: String,
    regex: Option<Regex>,
}

/// `ChatFilter.rule`: None = empty rule, Err = the regex error.
pub fn rule(raw: &str) -> Result<Option<Rule>, String> {
    let mut s = raw.trim();
    let mut server = None;
    if let Some(rest) = s.strip_prefix('@') {
        let Some((srv, tail)) = rest.split_once(' ') else {
            return Ok(None);
        };
        server = Some(srv.trim().to_lowercase());
        s = tail.trim();
    }
    if s.is_empty() {
        return Ok(None);
    }
    let mut regex = None;
    if s.len() >= 3 && s.is_char_boundary(3) && s[..3].eq_ignore_ascii_case("re:") {
        let re = s[3..].trim();
        if re.is_empty() {
            return Ok(None);
        }
        regex = Some(Regex::new(re)?);
    }
    Ok(Some(Rule {
        source: raw.trim().to_owned(),
        server,
        plain: s.to_lowercase(),
        regex,
    }))
}

/// `ChatFilter.serverMatches`.
pub fn server_matches(rule: Option<&str>, key: Option<&str>) -> bool {
    let (Some(rule), Some(key)) = (rule, key) else {
        return false;
    };
    let k = key.to_lowercase();
    let mut host = k.as_str();
    if !k.starts_with("singleplayer:")
        && let Some(i) = k.rfind(':')
        && i > 0
        && k[i + 1..].bytes().all(|c| c.is_ascii_digit())
        && i + 1 < k.len()
    {
        host = &k[..i];
    }
    k == rule || host == rule || host.ends_with(&format!(".{rule}"))
}

impl Rule {
    pub fn matches(&self, text: &str, server: Option<&str>) -> bool {
        if self.server.is_some() && !server_matches(self.server.as_deref(), server) {
            return false;
        }
        match &self.regex {
            Some(r) => r.find(text),
            None => text.to_lowercase().contains(&self.plain),
        }
    }
}

/// A parsed filter text: the usable rules and the errors (`rule: why`).
#[derive(Debug, Default)]
pub struct Filter {
    pub rules: Vec<Rule>,
    pub errors: Vec<String>,
}

impl Filter {
    pub fn parse(text: &str) -> Filter {
        let mut f = Filter::default();
        for raw in split(text) {
            match rule(&raw) {
                Ok(Some(r)) => f.rules.push(r),
                Ok(None) => {}
                Err(e) => f
                    .errors
                    .push(format!("{raw}: {}", e.split(" near").next().unwrap_or(&e))),
            }
        }
        f
    }

    /// The first rule that matches.
    pub fn find(&self, text: &str, server: Option<&str>) -> Option<&Rule> {
        self.rules.iter().find(|r| r.matches(text, server))
    }
}

/// `ChatFilter.isJoinLeave`.
pub fn is_join_leave(key: Option<&str>, text: Option<&str>) -> bool {
    if let Some(k) = key
        && k.starts_with("multiplayer.player.")
        && (k.contains("joined") || k.contains("left"))
    {
        return true;
    }
    let Some(t) = text else { return false };
    let t = t.trim();
    let Some((name, rest)) = t.split_once(' ') else {
        return false;
    };
    let n = name.chars().count();
    if !(1..=40).contains(&n) || name.contains(char::is_whitespace) {
        return false;
    }
    let mut rest = rest;
    if let Some(r) = rest.strip_prefix("(formerly known as ") {
        match r.split_once(") ") {
            Some((old, tail)) if !old.is_empty() && !old.contains(char::is_whitespace) => {
                rest = tail
            }
            _ => return false,
        }
    }
    rest == "joined the game" || rest == "left the game"
}

// ------------------------------------------------------------ SpamStacker

#[derive(Debug, Default)]
pub struct SpamStacker {
    last: Option<String>,
    count: u32,
}

impl SpamStacker {
    pub fn offer(&mut self, text: Option<&str>) -> u32 {
        if text.is_some() && self.last.as_deref() == text {
            self.count += 1;
            return self.count;
        }
        self.last = text.map(str::to_owned);
        self.count = 1;
        1
    }

    pub fn reset(&mut self) {
        self.last = None;
        self.count = 0;
    }
}

pub fn suffix(count: u32) -> String {
    if count > 1 {
        format!(" (x{count})")
    } else {
        String::new()
    }
}

// ------------------------------------------------------------ ChatLine

/// One log line (spec section 4).
#[derive(Clone, Debug, PartialEq)]
pub struct ChatLine {
    pub time: String,
    pub server: Option<String>,
    pub kind: Option<String>,
    pub sender: Option<String>,
    pub text: String,
    pub filtered: bool,
    pub extra: Option<Obj>,
}

/// A JSON string without HTML escaping.
fn quote(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '\u{8}' => o.push_str("\\b"),
            '\u{c}' => o.push_str("\\f"),
            c if c < ' ' || c == '\u{2028}' || c == '\u{2029}' => {
                o.push_str(&format!("\\u{:04x}", c as u32))
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

const CORE: [&str; 6] = ["time", "server", "type", "sender", "text", "filtered"];

impl ChatLine {
    pub fn new(
        time: &str,
        server: Option<&str>,
        kind: &str,
        sender: Option<&str>,
        text: &str,
        filtered: bool,
    ) -> ChatLine {
        ChatLine {
            time: time.to_owned(),
            server: server.map(str::to_owned),
            kind: Some(kind.to_owned()),
            sender: sender.map(str::to_owned),
            text: text.to_owned(),
            filtered,
            extra: None,
        }
    }

    pub fn to_json(&self) -> String {
        let mut o = Obj::new();
        o.insert("time", Json::str(&self.time));
        if let Some(s) = &self.server {
            o.insert("server", Json::str(s));
        }
        if let Some(t) = &self.kind {
            o.insert("type", Json::str(t));
        }
        if let Some(s) = self.sender.as_deref().filter(|s| !s.is_empty()) {
            o.insert("sender", Json::str(s));
        }
        o.insert("text", Json::str(&self.text));
        if self.filtered {
            o.insert("filtered", Json::Bool(true));
        }
        if let Some(e) = &self.extra {
            for (k, v) in e.iter() {
                if !CORE.contains(&k) {
                    o.insert(k, v.clone());
                }
            }
        }
        // Gson here doesn't escape `<`, `>`, `&`, `'` or `=` (compact() does)
        let mut s = String::from("{");
        for (i, (k, v)) in o.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&quote(k));
            s.push(':');
            match v {
                Json::Str(x) => s.push_str(&quote(x)),
                v => s.push_str(&v.compact()),
            }
        }
        s.push('}');
        s
    }

    pub fn parse(line: &str) -> Option<ChatLine> {
        if line.trim().is_empty() {
            return None;
        }
        let Ok(Json::Obj(o)) = Json::parse(line) else {
            return None;
        };
        let get = |k: &str| match o.get(k) {
            Some(Json::Null) | None => None,
            Some(v) => v.as_string(),
        };
        let (text, time) = (get("text")?, get("time")?);
        let mut extra = Obj::new();
        for (k, v) in o.iter() {
            if !CORE.contains(&k) {
                extra.insert(k, v.clone());
            }
        }
        Some(ChatLine {
            time,
            server: get("server"),
            kind: get("type"),
            sender: get("sender"),
            text,
            filtered: o.get("filtered").and_then(Json::as_bool).unwrap_or(false),
            extra: (!extra.is_empty()).then_some(extra),
        })
    }
}

/// `ChatLine.plain`: without `§` codes.
pub fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '§' {
            // a lone trailing `§` stays (the regex needs a character after it)
            if it.clone().next().is_none() {
                out.push(c);
            } else {
                it.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

const SECRET: [&str; 12] = [
    "login",
    "l",
    "log",
    "register",
    "reg",
    "changepassword",
    "changepass",
    "changepw",
    "unregister",
    "email",
    "2fa",
    "premium",
];

/// `ChatLine.maskCommand`: passwords in sent commands become `***`.
pub fn mask_command(command: &str) -> String {
    let c = command.strip_prefix('/').unwrap_or(command);
    let Some((first, _)) = c.split_once(' ') else {
        return format!("/{c}");
    };
    let lower = first.to_lowercase();
    let verb = lower.rsplit_once(':').map_or(lower.as_str(), |(_, v)| v);
    if SECRET.contains(&verb) {
        format!("/{first} ***")
    } else {
        format!("/{c}")
    }
}

// ------------------------------------------------------------ store

/// The chat log folder: segments `chat-<day>-<nnn>.jsonl`, appended to
/// through `append`, capped in total size.
#[derive(Default)]
pub struct Store {
    dir: PathBuf,
    out: Option<File>,
    current: Option<String>,
    size: u64,
    failed: bool,
}

fn names_in(dir: &Path) -> Vec<String> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let names: Vec<String> = rd
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    sorted(&names)
}

impl Store {
    pub fn new(dir: PathBuf) -> Store {
        Store {
            dir,
            ..Store::default()
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes one line; rotates and enforces the cap. Errors are kept to
    /// a single report (`false` the first time, then silently).
    pub fn append(&mut self, line: &ChatLine, cap: u64, today: &str) -> Result<(), String> {
        let mut bytes = line.to_json().into_bytes();
        bytes.push(b'\n');
        let limit = segment_limit(cap);
        let res = (|| -> std::io::Result<()> {
            if self.out.is_none()
                || should_rotate(
                    self.current.as_deref(),
                    self.size,
                    bytes.len() as u64,
                    limit,
                    today,
                )
            {
                self.open(today, limit, bytes.len() as u64, cap)?;
            }
            if let Some(f) = self.out.as_mut() {
                f.write_all(&bytes)?;
                self.size += bytes.len() as u64;
            }
            Ok(())
        })();
        if let Err(e) = res {
            self.out = None;
            if !std::mem::replace(&mut self.failed, true) {
                return Err(format!(
                    "Could not write the chat log in {}: {e}",
                    self.dir.display()
                ));
            }
        }
        Ok(())
    }

    fn open(&mut self, today: &str, limit: u64, next: u64, cap: u64) -> std::io::Result<()> {
        self.out = None;
        fs::create_dir_all(&self.dir)?;
        let names = names_in(&self.dir);
        let size_of = |n: &str| fs::metadata(self.dir.join(n)).map_or(0, |m| m.len());
        let name = match names.last() {
            Some(n) if !should_rotate(Some(n), size_of(n), next, limit, today) => n.clone(),
            _ => next_segment(&names, today),
        };
        let path = self.dir.join(&name);
        let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
        let mut size = f.metadata()?.len();
        if size > 0 {
            let mut last = [0u8; 1];
            let mut r = File::open(&path)?;
            r.seek(SeekFrom::End(-1))?;
            r.read_exact(&mut last)?;
            if last[0] != b'\n' {
                f.write_all(b"\n")?;
                size += 1;
            }
        }
        self.out = Some(f);
        self.current = Some(name);
        self.size = size;
        self.enforce_cap(cap);
        Ok(())
    }

    fn enforce_cap(&self, cap: u64) {
        let segs: Vec<(String, u64)> = names_in(&self.dir)
            .into_iter()
            .map(|n| {
                let s = fs::metadata(self.dir.join(&n)).map_or(0, |m| m.len());
                (n, s)
            })
            .collect();
        for n in to_delete(&segs, cap, self.current.as_deref()) {
            let _ = fs::remove_file(self.dir.join(n));
        }
    }

    /// Closes the open file; the next append reopens.
    pub fn close(&mut self) {
        self.out = None;
        self.current = None;
        self.size = 0;
    }

    pub fn total_size(&self) -> u64 {
        names_in(&self.dir)
            .iter()
            .map(|n| fs::metadata(self.dir.join(n)).map_or(0, |m| m.len()))
            .sum()
    }

    /// All lines, newest first, over the segments newest first, within
    /// `max_bytes` of scanned file size.
    fn lines_newest_first(&self, max_bytes: u64, mut f: impl FnMut(&str) -> bool) {
        let mut budget = max_bytes as i128;
        for name in names_in(&self.dir).iter().rev() {
            if budget <= 0 {
                break;
            }
            let path = self.dir.join(name);
            let Ok(bytes) = fs::read(&path) else { continue };
            budget -= bytes.len() as i128;
            let text = String::from_utf8_lossy(&bytes);
            for l in text.rsplit('\n') {
                let l = l.trim();
                if !l.is_empty() && !f(l) {
                    return;
                }
            }
        }
    }

    /// The newest `n` lines accepted by `keep`, oldest first.
    pub fn tail(
        &self,
        n: usize,
        max_bytes: u64,
        keep: impl Fn(&ChatLine) -> bool,
    ) -> Vec<ChatLine> {
        let mut out = Vec::new();
        if n == 0 {
            return out;
        }
        self.lines_newest_first(max_bytes, |raw| {
            if let Some(l) = ChatLine::parse(raw)
                && keep(&l)
            {
                out.push(l);
            }
            out.len() < n
        });
        out.reverse();
        out
    }

    /// Up to `max` lines whose text holds `query`, over all servers,
    /// newest first.
    pub fn search(&self, query: &str, max: usize) -> Vec<ChatLine> {
        let q = query.to_lowercase();
        let q = q.trim();
        let mut out = Vec::new();
        if q.is_empty() || max == 0 {
            return out;
        }
        let prefilter = !q.chars().any(|c| c < ' ' || c == '"' || c == '\\');
        self.lines_newest_first(u64::MAX, |raw| {
            if prefilter && !raw.to_lowercase().contains(q) {
                return true;
            }
            if let Some(l) = ChatLine::parse(raw)
                && l.text.to_lowercase().contains(q)
            {
                out.push(l);
            }
            out.len() < max
        });
        out
    }
}

/// Distinct set helper for the tests of the store.
#[allow(dead_code)]
pub fn distinct<T: std::hash::Hash + Eq>(v: impl IntoIterator<Item = T>) -> usize {
    v.into_iter().collect::<HashSet<T>>().len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fx() -> Value {
        serde_json::from_str(
            &std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../tests/fixtures/chat-pack.json"
            ))
            .unwrap(),
        )
        .unwrap()
    }

    fn s(v: &Value) -> Option<&str> {
        v.as_str()
    }

    #[test]
    fn byte_size() {
        let f = &fx()["byte_size"];
        for c in f["parse"].as_array().unwrap() {
            assert_eq!(parse_size(s(&c[0]).unwrap()), c[1].as_i64().unwrap(), "{c}");
        }
        for c in f["parse_cap"].as_array().unwrap() {
            let (t, fb) = (s(&c["text"]).unwrap(), c["fallback"].as_u64().unwrap());
            assert_eq!(parse_cap(t, fb), c["expect"].as_u64().unwrap(), "{c}");
        }
        for c in f["format"].as_array().unwrap() {
            assert_eq!(format_size(c[0].as_u64().unwrap()), s(&c[1]).unwrap());
        }
    }

    fn strings(v: &Value) -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn rotation() {
        let f = &fx()["log_rotation"];
        for c in f["segment_limit"].as_array().unwrap() {
            assert_eq!(
                segment_limit(c[0].as_u64().unwrap()),
                c[1].as_u64().unwrap()
            );
        }
        for c in f["names"].as_array().unwrap() {
            assert_eq!(
                segment_name(s(&c[0]).unwrap(), c[1].as_u64().unwrap()),
                s(&c[2]).unwrap()
            );
        }
        for c in f["is_segment"].as_array().unwrap() {
            assert_eq!(
                is_segment(s(&c[0]).unwrap()),
                c[1].as_bool().unwrap(),
                "{c}"
            );
        }
        assert_eq!(
            sorted(&strings(&f["sorted"]["in"])),
            strings(&f["sorted"]["out"])
        );
        for c in f["next"].as_array().unwrap() {
            assert_eq!(
                next_segment(&strings(&c["existing"]), s(&c["today"]).unwrap()),
                s(&c["expect"]).unwrap()
            );
        }
        for c in f["should_rotate"].as_array().unwrap() {
            assert_eq!(
                should_rotate(
                    s(&c["current"]),
                    c["size"].as_u64().unwrap(),
                    c["next_bytes"].as_u64().unwrap(),
                    c["limit"].as_u64().unwrap(),
                    s(&c["today"]).unwrap()
                ),
                c["expect"].as_bool().unwrap(),
                "{c}"
            );
        }
        for c in f["to_delete"].as_array().unwrap() {
            let segs: Vec<(String, u64)> = c["segments"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| {
                    (
                        s(&x["name"]).unwrap().to_owned(),
                        x["size"].as_u64().unwrap(),
                    )
                })
                .collect();
            assert_eq!(
                to_delete(&segs, c["cap"].as_u64().unwrap(), s(&c["current"])),
                strings(&c["expect"]),
                "{c}"
            );
        }
    }

    #[test]
    fn filter() {
        let f = &fx()["chat_filter"];
        for c in f["parse"].as_array().unwrap() {
            let p = Filter::parse(s(&c["text"]).unwrap_or(""));
            assert_eq!(p.rules.len() as u64, c["rules"].as_u64().unwrap(), "{c}");
            assert_eq!(p.errors.len() as u64, c["errors"].as_u64().unwrap(), "{c}");
        }
        for c in f["match"].as_array().unwrap() {
            let p = Filter::parse(s(&c["rules"]).unwrap());
            assert_eq!(
                p.find(s(&c["text"]).unwrap(), s(&c["server"])).is_some(),
                c["expect"].as_bool().unwrap(),
                "{c}"
            );
        }
        for c in f["split"].as_array().unwrap() {
            assert_eq!(split(s(&c["in"]).unwrap_or("")), strings(&c["out"]), "{c}");
        }
        for c in f["join"].as_array().unwrap() {
            assert_eq!(join(&strings(&c["in"])), s(&c["out"]).unwrap());
        }
        for c in f["split_join_roundtrip"].as_array().unwrap() {
            let r = strings(c);
            assert_eq!(split(&join(&r)), r);
        }
        for c in f["server_matches"].as_array().unwrap() {
            assert_eq!(
                server_matches(s(&c["rule"]), s(&c["key"])),
                c["expect"].as_bool().unwrap(),
                "{c}"
            );
        }
        for c in f["join_leave"].as_array().unwrap() {
            assert_eq!(
                is_join_leave(s(&c["key"]), s(&c["text"])),
                c["expect"].as_bool().unwrap(),
                "{c}"
            );
        }
    }

    #[test]
    fn stacker() {
        let f = &fx()["spam_stacker"];
        for run in f["runs"].as_array().unwrap() {
            let mut st = SpamStacker::default();
            for op in run["ops"].as_array().unwrap() {
                if op[0] == "reset" {
                    st.reset();
                } else {
                    assert_eq!(u64::from(st.offer(s(&op[1]))), op[2].as_u64().unwrap());
                }
            }
        }
        for c in f["suffix"].as_array().unwrap() {
            assert_eq!(suffix(c[0].as_u64().unwrap() as u32), s(&c[1]).unwrap());
        }
    }

    #[test]
    fn lines() {
        let f = &fx()["chat_line"];
        for c in f["to_json"].as_array().unwrap() {
            let extra = c["extra"].as_object().map(|m| {
                let mut o = Obj::new();
                for (k, v) in m {
                    o.insert(k, Json::parse(&v.to_string()).unwrap());
                }
                o
            });
            let l = ChatLine {
                time: s(&c["time"]).unwrap().into(),
                server: s(&c["server"]).map(Into::into),
                kind: s(&c["type"]).map(Into::into),
                sender: s(&c["sender"]).map(Into::into),
                text: s(&c["text"]).unwrap().into(),
                filtered: c["filtered"].as_bool().unwrap(),
                extra,
            };
            assert_eq!(l.to_json(), s(&c["expect"]).unwrap());
        }
        for c in f["parse"].as_array().unwrap() {
            let got = ChatLine::parse(s(&c["line"]).unwrap());
            match c["expect"].as_object() {
                None => assert!(got.is_none(), "{c}"),
                Some(e) => {
                    let g = got.expect("parsed");
                    assert_eq!(g.time, e["time"].as_str().unwrap());
                    assert_eq!(g.text, e["text"].as_str().unwrap());
                    assert_eq!(g.server.as_deref(), s(&e["server"]));
                    assert_eq!(g.kind.as_deref(), s(&e["type"]));
                    assert_eq!(g.filtered, e["filtered"].as_bool().unwrap());
                    assert_eq!(g.extra.is_some(), e["extra"].is_object());
                }
            }
        }
        for c in f["mask_command"].as_array().unwrap() {
            assert_eq!(mask_command(s(&c[0]).unwrap()), s(&c[1]).unwrap(), "{c}");
        }
        for c in f["plain"].as_array().unwrap() {
            assert_eq!(plain(s(&c[0]).unwrap_or("")), s(&c[1]).unwrap());
        }
    }

    fn pick(spec: &str, i: usize) -> Option<&str> {
        // "even:a.net,odd:b.net" -> the server of line i
        let want = if i.is_multiple_of(2) { "even:" } else { "odd:" };
        spec.split(',').find_map(|p| p.strip_prefix(want))
    }

    #[test]
    fn store_scenarios() {
        use crate::omnitool_config::tests::temp_dir;
        for sc in fx()["chat_store"].as_array().unwrap() {
            let dir = temp_dir();
            let mut st = Store::new(dir.clone());
            let cap = sc["cap"].as_u64().unwrap();
            let mut today = s(&sc["today"]).unwrap().to_owned();
            if today == "now" {
                today = "2026-10-17".into();
            }
            let line = |server: Option<&str>, kind: &str, text: &str, filtered: bool| {
                ChatLine::new(
                    "2026-10-17T14:03:22.123+02:00",
                    server,
                    kind,
                    None,
                    text,
                    filtered,
                )
            };
            for step in sc["steps"].as_array().unwrap() {
                let kind = step["type"].as_str().unwrap_or("chat");
                match step["op"].as_str().unwrap() {
                    "write_n" | "append_n" => {
                        for i in 0..step["count"].as_u64().unwrap() as usize {
                            let text = s(&step["text"]).unwrap().replace("{i}", &i.to_string());
                            let srv = pick(s(&step["server"]).unwrap(), i)
                                .or(s(&step["server"]).filter(|x| !x.contains(':')));
                            let filtered = step["filtered_index"].as_u64() == Some(i as u64);
                            st.append(&line(srv, kind, &text, filtered), cap, &today)
                                .unwrap();
                        }
                    }
                    "write" => {
                        let l = line(s(&step["server"]), kind, s(&step["text"]).unwrap(), false);
                        st.append(&l, cap, &today).unwrap();
                    }
                    "set_today" => today = s(&step["value"]).unwrap().into(),
                    "close" => st.close(),
                    "reopen" => st = Store::new(dir.clone()),
                    "append_raw_to_newest" => {
                        let newest = names_in(&dir).pop().unwrap();
                        let mut f = OpenOptions::new()
                            .append(true)
                            .open(dir.join(newest))
                            .unwrap();
                        f.write_all(s(&step["raw"]).unwrap().as_bytes()).unwrap();
                    }
                    "expect" => {
                        let names = names_in(&dir);
                        if let Some(n) = step["segment_count_greater_than"].as_u64() {
                            assert!(names.len() as u64 > n, "{} segments", names.len());
                        }
                        if let Some(n) = step["segment_count"].as_u64() {
                            assert_eq!(names.len() as u64, n);
                        }
                        if step["total_size_at_most_cap"].as_bool() == Some(true) {
                            assert!(st.total_size() <= cap);
                        }
                        if let Some(p) = s(&step["newest_segment_starts_with"]) {
                            assert!(names.last().unwrap().starts_with(p));
                        }
                    }
                    "tail" => {
                        let srv = s(&step["server"]);
                        let got = st.tail(step["n"].as_u64().unwrap() as usize, u64::MAX, |l| {
                            !l.filtered && srv.is_none_or(|x| l.server.as_deref() == Some(x))
                        });
                        let texts: Vec<&str> = got.iter().map(|l| l.text.as_str()).collect();
                        let want: Vec<&str> = step["expect_texts"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|x| x.as_str().unwrap())
                            .collect();
                        assert_eq!(texts, want);
                    }
                    "search" => {
                        let got = st.search(
                            s(&step["query"]).unwrap(),
                            step["max"].as_u64().unwrap() as usize,
                        );
                        assert_eq!(got.len() as u64, step["expect_count"].as_u64().unwrap());
                        if let Some(x) = s(&step["expect_first_server"]) {
                            assert_eq!(got[0].server.as_deref(), Some(x));
                        }
                    }
                    op => panic!("op {op}"),
                }
            }
            let _ = fs::remove_dir_all(dir);
        }
    }
}
