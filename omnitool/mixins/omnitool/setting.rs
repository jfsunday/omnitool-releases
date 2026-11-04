// Setting types as Java's core/setting/*.java: what a module declares
// (`SettingDef`: id, English name, kind with default and range) and the
// value it holds (`Value`), with Java's JSON reading and writing, number
// snapping and display rules. Added to the game as `game::omnitool_setting`;
// also a module of this crate, checked against tests/fixtures/core-settings.json.

use super::omnitool_json::Json;

/// KeySetting.NONE: no key bound.
pub const NONE: i32 = -1;

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)] // used by modules and menus still to come
pub enum Kind {
    Bool(bool),
    Number {
        default: f64,
        min: f64,
        max: f64,
        step: f64,
    },
    /// The constants' names (Java `name()`), and the default's index.
    Enum {
        values: &'static [&'static str],
        default: usize,
    },
    /// ARGB.
    Color(i32),
    Text(&'static str),
    /// A GLFW key code, `NONE` when unbound.
    Key(i32),
    /// A button; holds no value.
    Action,
}

/// A setting as a module declares it (Java's constructor arguments).
#[derive(Clone, Copy, Debug)]
pub struct SettingDef {
    /// The config key.
    pub id: &'static str,
    /// The English text (the translation key).
    pub name: &'static str,
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Num(f64),
    Enum(usize),
    Color(i32),
    Text(String),
    Key(i32),
    Action,
}

impl Value {
    pub fn on(&self) -> bool {
        matches!(self, Value::Bool(true))
    }
    #[allow(dead_code)] // used by modules and menus still to come
    pub fn num(&self) -> f64 {
        match *self {
            Value::Num(n) => n,
            _ => 0.0,
        }
    }
    pub fn index(&self) -> usize {
        match *self {
            Value::Enum(i) => i,
            _ => 0,
        }
    }
    /// The ARGB value of a colour setting.
    pub fn color(&self) -> i32 {
        match *self {
            Value::Color(c) => c,
            _ => 0,
        }
    }
    pub fn key(&self) -> i32 {
        match *self {
            Value::Key(k) => k,
            _ => NONE,
        }
    }
    pub fn text(&self) -> &str {
        match self {
            Value::Text(t) => t,
            _ => "",
        }
    }
}

impl SettingDef {
    pub const fn new(id: &'static str, name: &'static str, kind: Kind) -> SettingDef {
        SettingDef { id, name, kind }
    }

    pub fn default_value(&self) -> Value {
        match self.kind {
            Kind::Bool(b) => Value::Bool(b),
            Kind::Number { default, .. } => Value::Num(default),
            Kind::Enum { default, .. } => Value::Enum(default),
            Kind::Color(c) => Value::Color(c),
            Kind::Text(t) => Value::Text(t.to_owned()),
            Kind::Key(k) => Value::Key(k),
            Kind::Action => Value::Action,
        }
    }

    /// Java's `set`: numbers snap to the step and stay in range.
    pub fn set(&self, v: &mut Value, new: Value) {
        *v = match (self.kind, new) {
            (Kind::Number { min, max, step, .. }, Value::Num(n)) => {
                Value::Num(snap(n, min, max, step))
            }
            (Kind::Action, _) => Value::Action,
            (_, new) => new,
        };
    }

    /// Java's `toJson`.
    pub fn to_json(&self, v: &Value) -> Json {
        match (self.kind, v) {
            (_, Value::Bool(b)) => Json::Bool(*b),
            (_, Value::Num(n)) => Json::Num(*n),
            (Kind::Enum { values, .. }, Value::Enum(i)) => {
                Json::str(values.get(*i).copied().unwrap_or(""))
            }
            (_, Value::Color(c)) => Json::Str(color_json(*c)),
            (_, Value::Text(t)) => Json::str(t),
            (_, Value::Key(k)) => Json::Int(*k as i64),
            _ => Json::str(""),
        }
    }

    /// Java's `fromJson`: false where Java throws (the value stays). An
    /// unknown enum name is ignored without an error, as in Java.
    pub fn from_json(&self, v: &mut Value, json: &Json) -> bool {
        let new = match self.kind {
            Kind::Bool(_) => json.as_bool().map(Value::Bool),
            Kind::Number { .. } => json.as_double().map(Value::Num),
            Kind::Enum { values, .. } => match json.as_string() {
                Some(s) => match values.iter().position(|n| *n == s) {
                    Some(i) => Some(Value::Enum(i)),
                    None => return true,
                },
                None => None,
            },
            Kind::Color(_) => json
                .as_string()
                .and_then(|s| color_from(&s))
                .map(Value::Color),
            Kind::Text(_) => json.as_string().map(Value::Text),
            Kind::Key(_) => json.as_int().map(Value::Key),
            Kind::Action => return true,
        };
        match new {
            Some(new) => {
                self.set(v, new);
                true
            }
            None => false,
        }
    }

    /// Java's `isDefault`: numbers within half a step.
    #[allow(dead_code)] // used by modules and menus still to come
    pub fn is_default(&self, v: &Value) -> bool {
        match (self.kind, v) {
            (Kind::Number { default, step, .. }, Value::Num(n)) => (n - default).abs() < step / 2.0,
            _ => *v == self.default_value(),
        }
    }

    /// The value as the menu shows it (untranslated).
    #[allow(dead_code)] // used by modules and menus still to come
    pub fn display(&self, v: &Value) -> String {
        match (self.kind, v) {
            (Kind::Number { step, .. }, Value::Num(n)) => number_display(step, *n),
            (Kind::Enum { values, .. }, Value::Enum(i)) => {
                enum_display(values.get(*i).copied().unwrap_or(""))
            }
            (_, Value::Bool(b)) => if *b { "On" } else { "Off" }.into(),
            _ => self.to_json(v).as_string().unwrap_or_default(),
        }
    }

    /// Java's `Workflows.setSetting` after the lookup (`/ot set`): the
    /// text as a JSON string; an error text, or None.
    pub fn set_text(&self, v: &mut Value, text: &str) -> Option<String> {
        let before = v.clone();
        if !self.from_json(v, &Json::str(text)) {
            return Some(format!("Invalid value {text}"));
        }
        let now = self.to_json(v).as_string().unwrap_or_default();
        if *v == before && !now.eq_ignore_ascii_case(text) {
            return Some(format!("Invalid value {text}"));
        }
        None
    }
}

/// Java's `Math.round`: floor(x + 0.5), halves go up.
pub fn java_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// NumberSetting.set: snap to the step, then clamp.
pub fn snap(v: f64, min: f64, max: f64, step: f64) -> f64 {
    let snapped = java_round(v / step) * step;
    max.min(snapped).max(min)
}

/// NumberSetting.setFraction (from a 0..1 slider position).
#[allow(dead_code)] // used by modules and menus still to come
pub fn from_fraction(f: f64, min: f64, max: f64, step: f64) -> f64 {
    snap(min + (max - min) * f.clamp(0.0, 1.0), min, max, step)
}

/// NumberSetting.display: whole numbers from step 1 up, else 2 decimals
/// (`%.2f`, halves up, always a dot).
#[allow(dead_code)] // used by modules and menus still to come
pub fn number_display(step: f64, v: f64) -> String {
    if step >= 1.0 {
        return format!("{}", java_round(v) as i64);
    }
    two_decimals(v)
}

/// `String.format("%.2f")`: rounds the shortest decimal form half up.
#[allow(dead_code)] // used by modules and menus still to come
fn two_decimals(v: f64) -> String {
    let s = format!("{}", v.abs());
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    let mut digits: Vec<u8> = int
        .bytes()
        .chain(frac.bytes().chain(std::iter::repeat(b'0')).take(2))
        .collect();
    if frac.as_bytes().get(2).is_some_and(|&d| d >= b'5') {
        let mut i = digits.len();
        loop {
            if i == 0 {
                digits.insert(0, b'1');
                break;
            }
            i -= 1;
            if digits[i] == b'9' {
                digits[i] = b'0';
            } else {
                digits[i] += 1;
                break;
            }
        }
    }
    let n = digits.len();
    let body = format!(
        "{}.{}",
        String::from_utf8_lossy(&digits[..n - 2]),
        String::from_utf8_lossy(&digits[n - 2..])
    );
    if v < 0.0 && body.bytes().any(|d| (b'1'..=b'9').contains(&d)) {
        format!("-{body}")
    } else {
        body
    }
}

/// ColorSetting.toJson: `#AARRGGBB`.
pub fn color_json(argb: i32) -> String {
    format!("#{:08X}", argb as u32)
}

/// ColorSetting.fromJson: every `#` removed, hex as a long, cut to an int.
pub fn color_from(s: &str) -> Option<i32> {
    i64::from_str_radix(&s.replace('#', ""), 16)
        .ok()
        .map(|v| v as i32)
}

/// ColorSetting.channel(shift).
#[allow(dead_code)] // used by modules and menus still to come
pub fn channel(argb: i32, shift: u32) -> i32 {
    ((argb as u32) >> shift & 0xFF) as i32
}

/// ColorSetting.setChannel(shift, v): v clamped to 0..255.
#[allow(dead_code)] // used by modules and menus still to come
pub fn set_channel(argb: i32, shift: u32, v: i32) -> i32 {
    let v = v.clamp(0, 255) as u32;
    ((argb as u32 & !(0xFF << shift)) | v << shift) as i32
}

/// EnumSetting.cycle(dir): the index after moving `dir` steps, wrapping.
#[allow(dead_code)] // used by modules and menus still to come
pub fn cycle(index: usize, len: usize, dir: i32) -> usize {
    (index as i64 + dir as i64).rem_euclid(len as i64) as usize
}

/// EnumSetting.display: `A_B` -> `A b`.
#[allow(dead_code)] // used by modules and menus still to come
pub fn enum_display(name: &str) -> String {
    let n = name.to_lowercase().replace('_', " ");
    let mut c = n.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => n,
    }
}

/// TextSetting.entries: split at `,`, trimmed, lower case, no empty ones.
pub fn entries(text: &str) -> Vec<String> {
    text.split(',')
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty())
        .collect()
}

/// TextSetting.ids: entries without a namespace get `minecraft:`.
#[allow(dead_code)] // used by modules and menus still to come
pub fn ids(text: &str) -> Vec<String> {
    entries(text)
        .into_iter()
        .map(|e| {
            if e.contains(':') {
                e
            } else {
                format!("minecraft:{e}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value as J;

    fn fixture() -> J {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/core-settings.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn f(v: &J) -> f64 {
        v.as_f64().unwrap()
    }

    fn number(c: &J) -> SettingDef {
        let kind = Kind::Number {
            default: c.get("default").map_or(0.0, f),
            min: c.get("min").map_or(-1e9, f),
            max: c.get("max").map_or(1e9, f),
            step: f(&c["step"]),
        };
        SettingDef::new("n", "N", kind)
    }

    #[test]
    fn numbers() {
        let fx = fixture();
        for c in fx["number"].as_array().unwrap() {
            let s = number(c);
            let mut v = s.default_value();
            s.set(&mut v, Value::Num(f(&c["set"])));
            assert_eq!(v, Value::Num(f(&c["value"])), "{c}");
        }
        for c in fx["number_fraction"].as_array().unwrap() {
            assert_eq!(
                from_fraction(f(&c["fraction"]), f(&c["min"]), f(&c["max"]), f(&c["step"])),
                f(&c["value"])
            );
        }
        for c in fx["number_display"].as_array().unwrap() {
            assert_eq!(
                number_display(f(&c["step"]), f(&c["value"])),
                c["text"].as_str().unwrap()
            );
        }
        for c in fx["number_is_default"].as_array().unwrap() {
            let s = number(c);
            assert_eq!(
                s.is_default(&Value::Num(f(&c["value"]))),
                c["is_default"].as_bool().unwrap(),
                "{c}"
            );
        }
        assert_eq!(two_decimals(0.125), "0.13");
        assert_eq!(two_decimals(9.999), "10.00");
        assert_eq!(two_decimals(-0.001), "0.00");
        assert_eq!(two_decimals(-1.5), "-1.50");
    }

    #[test]
    fn colors() {
        let fx = fixture();
        for c in fx["color"].as_array().unwrap() {
            assert_eq!(
                color_json(c["argb"].as_i64().unwrap() as i32),
                c["json"].as_str().unwrap()
            );
        }
        for c in fx["color_from_json"].as_array().unwrap() {
            let got = color_from(c["json"].as_str().unwrap());
            match c.get("error") {
                Some(_) => assert_eq!(got, None),
                None => assert_eq!(got, Some(c["argb"].as_i64().unwrap() as i32)),
            }
        }
        for c in fx["color_channel"].as_array().unwrap() {
            let argb = c["argb"].as_i64().unwrap() as i32;
            let got = set_channel(
                argb,
                c["shift"].as_u64().unwrap() as u32,
                c["set"].as_i64().unwrap() as i32,
            );
            assert_eq!(got, c["result_argb"].as_i64().unwrap() as i32);
        }
        assert_eq!(channel(-2130771713, 24), 0x80);
    }

    fn names(c: &J) -> &'static [&'static str] {
        let v: Vec<&'static str> = c["values"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| &*v.as_str().unwrap().to_owned().leak())
            .collect();
        v.leak()
    }

    #[test]
    fn enums() {
        let fx = fixture();
        for c in fx["enum"].as_array().unwrap() {
            let values = names(c);
            if let Some(d) = c.get("display") {
                assert_eq!(
                    enum_display(d.as_str().unwrap()),
                    c["text"].as_str().unwrap()
                );
                continue;
            }
            let i = values.iter().position(|v| *v == c["default"]).unwrap();
            let next = cycle(i, values.len(), c["cycle"].as_i64().unwrap() as i32);
            assert_eq!(values[next], c["result"]);
        }
        for c in fx["enum_from_json"].as_array().unwrap() {
            let s = SettingDef::new(
                "e",
                "E",
                Kind::Enum {
                    values: names(c),
                    default: 0,
                },
            );
            let mut v = s.default_value();
            assert!(s.from_json(&mut v, &Json::str(c["json"].as_str().unwrap())));
            let want = if c["result"].as_str().unwrap().starts_with("unchanged") {
                "NORMAL"
            } else {
                "SLOW"
            };
            assert_eq!(s.to_json(&v), Json::str(want));
        }
    }

    #[test]
    fn texts_bools_keys_actions() {
        let fx = fixture();
        for c in fx["text_entries"].as_array().unwrap() {
            let text = c["value"].as_str().unwrap();
            assert_eq!(serde_json::json!(entries(text)), c["entries"]);
            assert_eq!(serde_json::json!(ids(text)), c["ids"]);
        }
        let b = SettingDef::new("b", "B", Kind::Bool(true));
        for c in fx["bool_from_json"].as_array().unwrap() {
            let mut v = Value::Bool(!c["value"].as_bool().unwrap());
            let json = Json::parse(&c["json"].to_string()).unwrap();
            assert!(b.from_json(&mut v, &json));
            assert_eq!(v, Value::Bool(c["value"].as_bool().unwrap()));
        }
        assert_eq!(fx["key"]["none"], NONE);
        let k = SettingDef::new("key", "Key", Kind::Key(NONE));
        for code in fx["key"]["json_roundtrip"].as_array().unwrap() {
            let mut v = k.default_value();
            assert!(k.from_json(&mut v, &Json::Int(code.as_i64().unwrap())));
            assert_eq!(k.to_json(&v), Json::Int(code.as_i64().unwrap()));
        }
        let a = SettingDef::new("run", "Run", Kind::Action);
        let mut v = a.default_value();
        assert_eq!(
            a.to_json(&v),
            Json::str(fx["action"]["to_json"].as_str().unwrap())
        );
        assert!(a.from_json(&mut v, &Json::Int(3)));
        a.set(&mut v, Value::Num(1.0));
        assert_eq!(v, Value::Action);
    }

    #[test]
    fn set_text_errors() {
        let fx = fixture();
        let cases = fx["workflow_set_setting"].as_array().unwrap();
        let n = SettingDef::new(
            "n",
            "N",
            Kind::Number {
                default: 1.0,
                min: 0.0,
                max: 10.0,
                step: 1.0,
            },
        );
        let mut v = n.default_value();
        assert_eq!(
            n.set_text(&mut v, "abc").as_deref(),
            cases[1]["error"].as_str()
        );
        let e = SettingDef::new(
            "e",
            "E",
            Kind::Enum {
                values: &["A", "B"],
                default: 0,
            },
        );
        let mut v = e.default_value();
        assert_eq!(
            e.set_text(&mut v, "NOPE").as_deref(),
            cases[2]["error"].as_str()
        );
        assert_eq!(e.set_text(&mut v, "B"), None);
        // unknown name, but the current value matches ignoring case: no error
        assert_eq!(e.set_text(&mut v, "b"), None);
        assert_eq!(e.set_text(&mut v, "a").as_deref(), Some("Invalid value a"));
        // Java quirks: "1" on a number already 1.0 is "Invalid value 1" (toJson is
        // "1.0"); any non-"true" text switches a bool off without an error
        let mut v = n.default_value();
        assert_eq!(n.set_text(&mut v, "1").as_deref(), Some("Invalid value 1"));
        assert_eq!(n.set_text(&mut v, "1.0"), None);
        assert_eq!(n.set_text(&mut v, "7.4"), None);
        assert_eq!(v, Value::Num(7.0));
        let b = SettingDef::new("b", "B", Kind::Bool(true));
        let mut v = b.default_value();
        assert_eq!(b.set_text(&mut v, "yes"), None);
        assert_eq!(v, Value::Bool(false));
        assert_eq!(
            b.set_text(&mut v, "yes").as_deref(),
            Some("Invalid value yes")
        );
    }
}
