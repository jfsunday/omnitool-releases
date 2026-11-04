// Conditions of the workflow `if` step (Java's core/Conditions): the parser
// is pure, the evaluation reads a `World` snapshot the host fills in. Checked
// against tests/fixtures/workflows.json (conditions_parse, compare,
// dimension_match, conditions_eval). Added to the game as
// `game::omnitool_conditions`; also a module of this crate.

use super::omnitool_safe::Server;
use super::omnitool_workflows::{item_matches, jtrim};

/// Java's `Conditions.Kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Health,
    Hunger,
    Y,
    Holding,
    Has,
    InventoryFull,
    Dimension,
    Server,
    Singleplayer,
    Sneaking,
    Day,
    Night,
    Module,
    ScreenOpen,
}

impl Kind {
    /// The Java enum name.
    #[allow(dead_code)] // the fixtures and the editor use it
    pub fn name(self) -> &'static str {
        match self {
            Kind::Health => "HEALTH",
            Kind::Hunger => "HUNGER",
            Kind::Y => "Y",
            Kind::Holding => "HOLDING",
            Kind::Has => "HAS",
            Kind::InventoryFull => "INVENTORY_FULL",
            Kind::Dimension => "DIMENSION",
            Kind::Server => "SERVER",
            Kind::Singleplayer => "SINGLEPLAYER",
            Kind::Sneaking => "SNEAKING",
            Kind::Day => "DAY",
            Kind::Night => "NIGHT",
            Kind::Module => "MODULE",
            Kind::ScreenOpen => "SCREEN_OPEN",
        }
    }
}

/// Java's `Cond`: numbers use op + num, HAS text = item and num = count,
/// MODULE text = module id and num = 1 (on) / 0 (off).
#[derive(Clone, Debug, PartialEq)]
pub struct Cond {
    pub kind: Kind,
    pub op: String,
    pub num: f64,
    pub text: String,
    pub negate: bool,
}

/// An item stack as conditions and triggers see it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    /// Registry id with namespace (`minecraft:torch`).
    pub id: String,
    /// The shown (translated) name.
    pub name: String,
    pub count: u32,
}

/// What conditions, triggers and the recorder read from the game, once per
/// use. `Host::world` gives it; None = no player or no level.
#[derive(Clone, Debug, PartialEq)]
pub struct World {
    /// Hearts × 2 (20 = full).
    pub health: f64,
    /// 0-20.
    pub hunger: i32,
    /// Feet position.
    pub y: f64,
    /// The main hand, None when empty.
    pub holding: Option<Item>,
    /// The non-empty stacks of inventory slots 0-35 (hotbar + main).
    pub inventory: Vec<Item>,
    /// No empty slot in 0-35.
    pub inventory_full: bool,
    /// Dimension id with namespace (`minecraft:the_nether`).
    pub dimension: String,
    pub server: Server,
    /// The world's name (single player), "" if unknown.
    pub world_name: String,
    pub sneaking: bool,
    pub dead: bool,
    /// Day time of the level (any value, taken mod 24000).
    pub day_time: i64,
    pub screen_open: bool,
}

const NUMBER_KEYS: [&str; 5] = ["health", "hp", "hunger", "food", "y"];
const OPS: [&str; 7] = ["<=", ">=", "!=", "==", "<", ">", "="];

/// Java's `Conditions.parse`; None for an unknown condition.
pub fn parse(raw: &str) -> Option<Cond> {
    let s = jtrim(raw)
        .to_lowercase()
        .split(is_space)
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    // Java's replaceAll("\\s+", " ") keeps a leading/trailing space only if
    // the trimmed text had one, which it hasn't
    if let Some(rest) = s.strip_prefix('!').or_else(|| s.strip_prefix("not ")) {
        let mut inner = parse(rest)?;
        inner.negate = !inner.negate;
        return Some(inner);
    }
    if let Some(c) = number(&s) {
        return Some(c);
    }
    if let Some(rest) = s.strip_prefix("holding ") {
        return text(Kind::Holding, rest);
    }
    if let Some(rest) = s.strip_prefix("has ") {
        // ^has\s+(.+?)(?:\s+(\d+))?$ (lazy item, optional count)
        let (item, count) = match rest.rsplit_once(' ') {
            Some((item, n)) if !item.is_empty() && is_digits(n) => {
                (item, n.parse().unwrap_or(f64::MAX))
            }
            _ => (rest, 1.0),
        };
        if !item.is_empty() {
            return Some(Cond {
                kind: Kind::Has,
                op: ">=".into(),
                num: count,
                text: jtrim(item).to_owned(),
                negate: false,
            });
        }
    }
    if let Some(rest) = s.strip_prefix("module ") {
        // ^module\s+(\S+)(?:\s+(on|off))?$
        let words: Vec<&str> = rest.split(' ').filter(|w| !w.is_empty()).collect();
        let num = match words.as_slice() {
            [_] => Some(1.0),
            [_, "on"] => Some(1.0),
            [_, "off"] => Some(0.0),
            _ => None,
        };
        if let Some(num) = num {
            return Some(Cond {
                kind: Kind::Module,
                op: "=".into(),
                num,
                text: words[0].to_owned(),
                negate: false,
            });
        }
    }
    for p in ["dimension ", "dim ", "in "] {
        if s.starts_with(p) && s.len() > p.len() {
            return text(Kind::Dimension, &s[p.len()..]);
        }
    }
    if let Some(rest) = s.strip_prefix("server ") {
        return text(Kind::Server, rest);
    }
    let kind = match s.as_str() {
        "inventory full" | "inventory_full" | "full inventory" => Kind::InventoryFull,
        "singleplayer" | "single player" => Kind::Singleplayer,
        "sneaking" | "sneak" => Kind::Sneaking,
        "day" => Kind::Day,
        "night" => Kind::Night,
        "screen open" | "screen" => Kind::ScreenOpen,
        _ => return None,
    };
    text(kind, "")
}

/// Java's `\s`.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0B' | '\x0C' | '\r')
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `^(health|hp|hunger|food|y)\s*(op)\s*(-?\d+(?:[.,]\d+)?)$`.
fn number(s: &str) -> Option<Cond> {
    let key = NUMBER_KEYS.iter().find(|k| s.starts_with(*k))?;
    let rest = s[key.len()..].trim_start_matches(' ');
    let op = OPS.iter().find(|o| rest.starts_with(*o))?;
    let n = rest[op.len()..].trim_start_matches(' ');
    let digits = n.strip_prefix('-').unwrap_or(n);
    let (int, frac) = match digits.find(['.', ',']) {
        Some(i) => (&digits[..i], Some(&digits[i + 1..])),
        None => (digits, None),
    };
    if !is_digits(int) || frac.is_some_and(|f| !is_digits(f)) {
        return None;
    }
    let kind = match *key {
        "health" | "hp" => Kind::Health,
        "hunger" | "food" => Kind::Hunger,
        _ => Kind::Y,
    };
    Some(Cond {
        kind,
        op: if *op == "==" { "=" } else { op }.to_owned(),
        num: n.replace(',', ".").parse().ok()?,
        text: String::new(),
        negate: false,
    })
}

fn text(kind: Kind, text: &str) -> Option<Cond> {
    let t = jtrim(text);
    if t.is_empty() && matches!(kind, Kind::Holding | Kind::Dimension | Kind::Server) {
        return None;
    }
    Some(Cond {
        kind,
        op: String::new(),
        num: 0.0,
        text: t.to_owned(),
        negate: false,
    })
}

/// Java's `Conditions.compare`.
pub fn compare(op: &str, a: f64, b: f64) -> bool {
    match op {
        "<" => a < b,
        ">" => a > b,
        "<=" => a <= b,
        ">=" => a >= b,
        "!=" => a != b,
        _ => a == b,
    }
}

/// Java's `dimensionMatches`: `nether` = `minecraft:the_nether`, full ids too.
pub fn dimension_matches(query: &str, id: &str) -> bool {
    let q = jtrim(query).to_lowercase();
    let path = id.split_once(':').map_or(id, |(_, p)| p);
    id == q || path == q || path == format!("the_{q}")
}

/// Java's `isLocalServer`.
pub fn local(server: &Server) -> bool {
    matches!(server, Server::Local)
}

/// The lowercase address of a multiplayer server, "" otherwise.
pub fn server_ip(server: &Server) -> String {
    match server {
        Server::Remote(ip) => ip.to_lowercase(),
        _ => String::new(),
    }
}

/// Java's `test(Cond, mc)`: false without a world (also when negated).
/// `module_on` = whether a module is on (None = unknown module).
pub fn test(c: &Cond, world: Option<&World>, module_on: &dyn Fn(&str) -> Option<bool>) -> bool {
    let Some(w) = world else { return false };
    let r = match c.kind {
        Kind::Health => compare(&c.op, w.health, c.num),
        Kind::Hunger => compare(&c.op, w.hunger.into(), c.num),
        Kind::Y => compare(&c.op, w.y, c.num),
        Kind::Holding => w
            .holding
            .as_ref()
            .is_some_and(|i| item_matches(&c.text, &i.id, &i.name)),
        Kind::Has => count(w, &c.text) as f64 >= c.num,
        Kind::InventoryFull => w.inventory_full,
        Kind::Dimension => dimension_matches(&c.text, &w.dimension),
        Kind::Server => match &w.server {
            Server::Remote(ip) => ip.to_lowercase().contains(&c.text),
            _ => false,
        },
        Kind::Singleplayer => local(&w.server),
        Kind::Sneaking => w.sneaking,
        Kind::Day | Kind::Night => (c.kind == Kind::Day) == day(w.day_time),
        Kind::Module => module_on(&c.text) == Some(c.num != 0.0),
        Kind::ScreenOpen => w.screen_open,
    };
    r != c.negate
}

/// Day = `t < 12542 || t >= 23460` with `t = floorMod(dayTime, 24000)`.
pub fn day(day_time: i64) -> bool {
    let t = day_time.rem_euclid(24000);
    !(12542..23460).contains(&t)
}

/// Items matching `query` in slots 0-35.
pub fn count(w: &World, query: &str) -> u64 {
    w.inventory
        .iter()
        .filter(|i| item_matches(query, &i.id, &i.name))
        .map(|i| u64::from(i.count))
        .sum()
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use serde_json::Value;

    fn fixtures() -> Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/workflows.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// The fixture world (conditions_eval / flow_sim) with the overrides.
    pub fn world(base: &Value, over: &Value) -> (Option<World>, Vec<(String, bool)>) {
        let mut v = base.clone();
        if let Some(o) = over.as_object() {
            for (k, x) in o {
                v[k] = x.clone();
            }
        }
        let modules = v["modules"]
            .as_object()
            .map(|m| {
                m.iter()
                    .map(|(k, b)| (k.clone(), b.as_bool().unwrap()))
                    .collect()
            })
            .unwrap_or_default();
        if v["in_world"] == Value::Bool(false) {
            return (None, modules);
        }
        let holding_id = v["holding_id"].as_str().unwrap_or("");
        let w = World {
            health: v["health"].as_f64().unwrap(),
            hunger: v["hunger"].as_i64().unwrap() as i32,
            y: v["y"].as_f64().unwrap(),
            holding: (!holding_id.is_empty()).then(|| Item {
                id: holding_id.into(),
                name: v["holding_name"].as_str().unwrap_or("").into(),
                count: 1,
            }),
            inventory: v["inventory"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| Item {
                    id: i["id"].as_str().unwrap().into(),
                    name: i["name"].as_str().unwrap().into(),
                    count: i["count"].as_u64().unwrap() as u32,
                })
                .collect(),
            inventory_full: v["inventory_full"].as_bool().unwrap(),
            dimension: v["dimension"].as_str().unwrap().into(),
            server: match v["server"].as_str() {
                Some(s) => Server::Remote(s.into()),
                None => Server::Local,
            },
            world_name: String::new(),
            sneaking: v["sneaking"].as_bool().unwrap(),
            dead: false,
            day_time: v["day_time"].as_i64().unwrap(),
            screen_open: v["screen_open"].as_bool().unwrap(),
        };
        (Some(w), modules)
    }

    pub fn module_lookup(modules: &[(String, bool)], id: &str) -> Option<bool> {
        modules.iter().find(|(m, _)| m == id).map(|(_, on)| *on)
    }

    #[test]
    fn parse_matches_java() {
        for case in fixtures()["conditions_parse"].as_array().unwrap() {
            let raw = case["raw"].as_str().unwrap();
            let got = parse(raw);
            let want = &case["cond"];
            if want.is_null() {
                assert_eq!(got, None, "{raw}");
                continue;
            }
            let got = got.unwrap_or_else(|| panic!("{raw}: no condition"));
            assert_eq!(got.kind.name(), want["kind"], "{raw}");
            assert_eq!(got.op, want["op"].as_str().unwrap(), "{raw}");
            assert!(
                (got.num - want["num"].as_f64().unwrap()).abs() < 1e-6,
                "{raw}"
            );
            assert_eq!(got.text, want["text"].as_str().unwrap(), "{raw}");
            assert_eq!(got.negate, want["negate"].as_bool().unwrap(), "{raw}");
        }
    }

    #[test]
    fn compare_and_dimensions() {
        let f = fixtures();
        for c in f["compare"].as_array().unwrap() {
            let got = compare(
                c["op"].as_str().unwrap(),
                c["a"].as_f64().unwrap(),
                c["b"].as_f64().unwrap(),
            );
            assert_eq!(got, c["result"].as_bool().unwrap(), "{c}");
        }
        for c in f["dimension_match"].as_array().unwrap() {
            let got = dimension_matches(
                c["query"].as_str().unwrap(),
                c["dimension"].as_str().unwrap(),
            );
            assert_eq!(got, c["match"].as_bool().unwrap(), "{c}");
        }
    }

    #[test]
    fn eval_matches_java() {
        let f = fixtures();
        let eval = &f["conditions_eval"];
        for c in eval["cases"].as_array().unwrap() {
            let raw = c["cond"].as_str().unwrap();
            let (w, modules) = world(&eval["world"], &c["override"]);
            let got = parse(raw)
                .is_some_and(|cond| test(&cond, w.as_ref(), &|id| module_lookup(&modules, id)));
            assert_eq!(got, c["result"].as_bool().unwrap(), "{c}");
        }
    }
}
