// A small JSON tree with Gson's behaviour where Omnitool depends on it: keys
// keep their order, numbers compare by value (`10` == `10.0`), output escapes
// like Gson (`<`, `>`, `&`, `=`, `'` as `<` ...), doubles print like
// Java's `Double.toString` (`10.0`), the pretty form indents by 2 spaces.
// The bundle has std only, so no serde. Added to the game as
// `game::omnitool_json`; also a module of this crate.

use std::fmt::Write as _;

#[derive(Clone, Debug)]
pub enum Json {
    Null,
    Bool(bool),
    /// An integral number (Java int/long, or a file's `10`).
    Int(i64),
    /// A double (Java `double`, or a file's `10.5`/`1e3`).
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Obj),
}

/// A JSON object: its entries in insertion order (Gson's `LinkedTreeMap`).
#[derive(Clone, Debug, Default)]
pub struct Obj(pub Vec<(String, Json)>);

impl PartialEq for Json {
    /// Gson's `JsonElement.equals`: numbers by value, everything else exact.
    fn eq(&self, other: &Json) -> bool {
        match (self, other) {
            (Json::Null, Json::Null) => true,
            (Json::Bool(a), Json::Bool(b)) => a == b,
            (Json::Int(a), Json::Int(b)) => a == b,
            (Json::Int(_) | Json::Num(_), Json::Int(_) | Json::Num(_)) => {
                let (a, b) = (self.as_f64().unwrap_or(0.0), other.as_f64().unwrap_or(0.0));
                a == b || (a.is_nan() && b.is_nan())
            }
            (Json::Str(a), Json::Str(b)) => a == b,
            (Json::Arr(a), Json::Arr(b)) => a == b,
            (Json::Obj(a), Json::Obj(b)) => a == b,
            _ => false,
        }
    }
}

impl PartialEq for Obj {
    /// Same entries, order ignored (Gson compares the maps).
    fn eq(&self, other: &Obj) -> bool {
        self.0.len() == other.0.len() && self.0.iter().all(|(k, v)| other.get(k) == Some(v))
    }
}

impl Obj {
    pub fn new() -> Obj {
        Obj(Vec::new())
    }
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
        self.0.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
    /// The value at `key` if it is an object (Java's `obj(o, key)` helpers).
    pub fn obj(&self, key: &str) -> Option<&Obj> {
        match self.get(key) {
            Some(Json::Obj(o)) => Some(o),
            _ => None,
        }
    }
    pub fn obj_mut(&mut self, key: &str) -> Option<&mut Obj> {
        match self.get_mut(key) {
            Some(Json::Obj(o)) => Some(o),
            _ => None,
        }
    }
    /// Gson's `add`: replaces the value in place, or appends.
    pub fn insert(&mut self, key: &str, value: Json) {
        match self.get_mut(key) {
            Some(v) => *v = value,
            None => self.0.push((key.to_owned(), value)),
        }
    }
    pub fn remove(&mut self, key: &str) -> Option<Json> {
        let i = self.0.iter().position(|(k, _)| k == key)?;
        Some(self.0.remove(i).1)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Json)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }
    #[allow(dead_code)] // used by modules and menus still to come
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Json {
    pub fn str(s: &str) -> Json {
        Json::Str(s.to_owned())
    }
    pub fn as_obj(&self) -> Option<&Obj> {
        match self {
            Json::Obj(o) => Some(o),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Json::Int(i) => Some(i as f64),
            Json::Num(n) => Some(n),
            _ => None,
        }
    }
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            Json::Bool(_) | Json::Int(_) | Json::Num(_) | Json::Str(_)
        )
    }

    /// Gson's `getAsString`: a primitive as text, else an error.
    pub fn as_string(&self) -> Option<String> {
        match self {
            Json::Bool(b) => Some(b.to_string()),
            Json::Int(i) => Some(i.to_string()),
            Json::Num(n) => Some(java_double(*n)),
            Json::Str(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// Gson's `getAsBoolean`: a boolean, or a string equal to `true`
    /// (ignoring case); any other primitive is false.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => Some(self.as_string()?.eq_ignore_ascii_case("true")),
        }
    }

    /// Gson's `getAsDouble`: a number, or a string Java's `parseDouble` reads.
    pub fn as_double(&self) -> Option<f64> {
        match self {
            Json::Int(_) | Json::Num(_) => Some(self.as_f64().unwrap_or(0.0)),
            Json::Str(s) => parse_java_double(s),
            _ => None,
        }
    }

    /// Gson's `getAsInt`: a number cut to an int, or a string `parseInt` reads.
    pub fn as_int(&self) -> Option<i32> {
        match *self {
            Json::Int(i) => Some(i as i32),
            Json::Num(n) => Some(n as i64 as i32),
            Json::Str(ref s) => s.parse::<i32>().ok(),
            _ => None,
        }
    }

    /// Gson's compact form (`new Gson().toJson`).
    pub fn compact(&self) -> String {
        let mut out = String::new();
        write(&mut out, self, None, 0);
        out
    }

    /// Gson's pretty form (`setPrettyPrinting`): 2 spaces, `"key": value`.
    pub fn pretty(&self) -> String {
        let mut out = String::new();
        write(&mut out, self, Some("  "), 0);
        out
    }

    /// Reads one JSON value; text after it (except whitespace) is an error.
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut p = Parser {
            s: text.as_bytes(),
            i: 0,
        };
        p.ws();
        let v = p.value(0)?;
        p.ws();
        if p.i != p.s.len() {
            return Err(format!("unexpected text at {}", p.i));
        }
        Ok(v)
    }
}

/// Java's `Double.toString`: `10.0`, `0.25`, `1.0E7`, `1.0E-4`.
pub fn java_double(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let a = v.abs();
    if (1e-3..1e7).contains(&a) {
        let s = format!("{v}");
        return if s.contains('.') { s } else { s + ".0" };
    }
    // scientific: shortest digits, mantissa with at least one decimal
    let s = format!("{v:e}");
    let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let mant = if mant.contains('.') {
        mant.to_owned()
    } else {
        format!("{mant}.0")
    };
    format!("{mant}E{exp}")
}

/// Java's `Double.parseDouble`: trims, allows a `d`/`f` suffix, `NaN`, `Infinity`.
pub fn parse_java_double(s: &str) -> Option<f64> {
    let t = s.trim_matches(|c: char| c <= ' ');
    let t = t.strip_suffix(['d', 'D', 'f', 'F']).unwrap_or(t);
    let body = t.trim_start_matches(['+', '-']);
    if body == "NaN" {
        return Some(f64::NAN);
    }
    if body == "Infinity" {
        return Some(if t.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    // Rust also reads "inf"/"nan"; Java doesn't
    if body.is_empty() || !body.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
        return None;
    }
    t.parse::<f64>().ok()
}

fn write(out: &mut String, v: &Json, indent: Option<&str>, depth: usize) {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Int(i) => {
            let _ = write!(out, "{i}");
        }
        // Gson refuses NaN/Infinity by default; null keeps the file readable
        Json::Num(n) if !n.is_finite() => out.push_str("null"),
        Json::Num(n) => out.push_str(&java_double(*n)),
        Json::Str(s) => quote(out, s),
        Json::Arr(a) => {
            out.push('[');
            for (i, e) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, depth + 1);
                write(out, e, indent, depth + 1);
            }
            if !a.is_empty() {
                newline(out, indent, depth);
            }
            out.push(']');
        }
        Json::Obj(o) => {
            out.push('{');
            for (i, (k, e)) in o.0.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, depth + 1);
                quote(out, k);
                out.push_str(if indent.is_some() { ": " } else { ":" });
                write(out, e, indent, depth + 1);
            }
            if !o.is_empty() {
                newline(out, indent, depth);
            }
            out.push('}');
        }
    }
}

fn newline(out: &mut String, indent: Option<&str>, depth: usize) {
    if let Some(ind) = indent {
        out.push('\n');
        for _ in 0..depth {
            out.push_str(ind);
        }
    }
}

/// Gson's string escaping (HTML-safe, the default).
fn quote(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{c}' => out.push_str("\\f"),
            '<' | '>' | '&' | '=' | '\'' | '\u{2028}' | '\u{2029}' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn err<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("{what} at {}", self.i))
    }

    fn eat(&mut self, lit: &str) -> bool {
        if self.s[self.i..].starts_with(lit.as_bytes()) {
            self.i += lit.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > 512 {
            return self.err("too deep");
        }
        match self.s.get(self.i) {
            None => self.err("end of input"),
            Some(b'{') => {
                self.i += 1;
                let mut o = Obj::new();
                self.ws();
                if self.eat("}") {
                    return Ok(Json::Obj(o));
                }
                loop {
                    self.ws();
                    if self.s.get(self.i) != Some(&b'"') {
                        return self.err("expected a key");
                    }
                    let k = self.string()?;
                    self.ws();
                    if !self.eat(":") {
                        return self.err("expected ':'");
                    }
                    self.ws();
                    let v = self.value(depth + 1)?;
                    // a duplicate key: Gson keeps the last value
                    o.insert(&k, v);
                    self.ws();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("}") {
                        return Ok(Json::Obj(o));
                    }
                    return self.err("expected ',' or '}'");
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.eat("]") {
                    return Ok(Json::Arr(a));
                }
                loop {
                    self.ws();
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("]") {
                        return Ok(Json::Arr(a));
                    }
                    return self.err("expected ',' or ']'");
                }
            }
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') if self.eat("true") => Ok(Json::Bool(true)),
            Some(b'f') if self.eat("false") => Ok(Json::Bool(false)),
            Some(b'n') if self.eat("null") => Ok(Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => self.err("unexpected character"),
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.i;
        let mut integral = true;
        while let Some(&c) = self.s.get(self.i) {
            match c {
                b'0'..=b'9' | b'-' | b'+' => {}
                b'.' | b'e' | b'E' => integral = false,
                _ => break,
            }
            self.i += 1;
        }
        let t = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
        if integral && let Ok(i) = t.parse::<i64>() {
            return Ok(Json::Int(i));
        }
        match t.parse::<f64>() {
            Ok(n) if t.as_bytes().last().is_some_and(u8::is_ascii_digit) => Ok(Json::Num(n)),
            _ => Err(format!("bad number at {start}")),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut out = Vec::new();
        loop {
            let Some(&c) = self.s.get(self.i) else {
                return self.err("unterminated string");
            };
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let Some(&e) = self.s.get(self.i) else {
                        return self.err("unterminated string");
                    };
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            if (0xD800..0xDC00).contains(&hi) && self.eat("\\u") {
                                let lo = self.hex4()?;
                                char::from_u32(
                                    0x10000
                                        + ((hi - 0xD800) << 10)
                                        + (lo.wrapping_sub(0xDC00) & 0x3FF),
                                )
                                .unwrap_or('\u{FFFD}')
                            } else {
                                char::from_u32(hi).unwrap_or('\u{FFFD}')
                            }
                        }
                        _ => return self.err("bad escape"),
                    };
                    let mut buf = [0; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                c => out.push(c),
            }
        }
        String::from_utf8(out).map_err(|_| "invalid UTF-8".to_owned())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let h = self
            .s
            .get(self.i..self.i + 4)
            .and_then(|h| std::str::from_utf8(h).ok());
        match h.and_then(|h| u32::from_str_radix(h, 16).ok()) {
            Some(v) => {
                self.i += 4;
                Ok(v)
            }
            None => self.err("bad \\u escape"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_print() {
        let v = Json::parse(
            r#" {"a": [1, 2.5, -3e2, true, null], "b": {"c": "x\"y\u00fc"}, "d": {}} "#,
        )
        .unwrap();
        assert_eq!(
            v.compact(),
            r#"{"a":[1,2.5,-300.0,true,null],"b":{"c":"x\"yü"},"d":{}}"#
        );
        assert_eq!(
            v.pretty(),
            "{\n  \"a\": [\n    1,\n    2.5,\n    -300.0,\n    true,\n    null\n  ],\n  \"b\": {\n    \"c\": \"x\\\"yü\"\n  },\n  \"d\": {}\n}"
        );
        assert!(Json::parse("").is_err());
        assert!(Json::parse("{\"v\":8, \"modules\": {").is_err());
        assert!(Json::parse("not json").is_err());
        assert!(Json::parse("{} x").is_err());
        assert_eq!(Json::parse("\"\\ud83d\\ude00\"").unwrap(), Json::str("😀"));
    }

    #[test]
    fn gson_escapes_html_characters() {
        assert_eq!(
            Json::str("<a href='x'>&=").compact(),
            "\"\\u003ca href\\u003d\\u0027x\\u0027\\u003e\\u0026\\u003d\""
        );
    }

    #[test]
    fn numbers_compare_by_value_and_print_like_java() {
        assert_eq!(Json::Int(10), Json::Num(10.0));
        assert_ne!(Json::Int(10), Json::str("10"));
        assert_eq!(java_double(10.0), "10.0");
        assert_eq!(java_double(0.25), "0.25");
        assert_eq!(java_double(1e7), "1.0E7");
        assert_eq!(java_double(1.5e-4), "1.5E-4");
        assert_eq!(java_double(-2.0), "-2.0");
        assert_eq!(parse_java_double(" 5 "), Some(5.0));
        assert_eq!(parse_java_double("2.5d"), Some(2.5));
        assert!(parse_java_double("abc").is_none());
        assert!(parse_java_double("inf").is_none());
    }

    #[test]
    fn objects_compare_without_order() {
        let a = Json::parse(r#"{"x":1,"y":2}"#).unwrap();
        let b = Json::parse(r#"{"y":2,"x":1.0}"#).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn gson_conversions() {
        assert_eq!(Json::str("TRUE").as_bool(), Some(true));
        assert_eq!(Json::str("yes").as_bool(), Some(false));
        assert!(Json::Null.as_bool().is_none());
        assert_eq!(Json::Num(82.7).as_int(), Some(82));
        assert!(Json::str("82.5").as_int().is_none());
        assert_eq!(Json::Num(10.0).as_string(), Some("10.0".into()));
    }
}
