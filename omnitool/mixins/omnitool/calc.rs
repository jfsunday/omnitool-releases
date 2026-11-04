// The Calculator module: Java's core/Calc.java (the math parser, `fmt`,
// `items`) ported 1:1, and `/calc` from modules/Calculator.java. The live
// result above the chat input and Tab need a chat-screen hook Horizon doesn't
// offer yet. Added to the game as `game::omnitool_calc`; also a module of
// this crate, checked against tests/fixtures/calc.json (made from Java).

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, SettingDef};
use std::cell::Cell;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "calculator",
    name: "Calculator",
    category: Category::Misc,
    description: "Type math in chat (e.g. 164*9): the result shows above, Tab inserts it. Also /calc nether, dist, rates.",
    default_on: true,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "CalcMod, Chat Calc",
    key: super::omnitool_setting::NONE,
    hold: false,
    // read by the live preview and Tab, once a chat-screen hook exists
    settings: &[
        SettingDef::new("live", "Show result while typing in chat", Kind::Bool(true)),
        SettingDef::new("tab", "Tab inserts the result", Kind::Bool(true)),
    ],
    command: Some(command),
    hud: None,
    tick: None,
    on_key: None,
    hud_right: None,
    on_action: None,
};

/// Calc.CONSTANTS: Minecraft units and the two constants.
pub fn constant(name: &str) -> Option<f64> {
    Some(match name {
        "stack" => 64.0,
        "stack16" => 16.0,
        "stack1" => 1.0,
        "sb" => 1728.0,
        "sb16" => 432.0,
        "sb1" => 27.0,
        "dub" => 3456.0,
        "dub16" => 864.0,
        "dub1" => 54.0,
        "min" => 60.0,
        "h" => 3600.0,
        "day" => 24000.0,
        "pi" => std::f64::consts::PI,
        "e" => std::f64::consts::E,
        _ => return None,
    })
}

/// Calc.FUNCTIONS: one argument each, angles in degrees.
fn function(name: &str) -> Option<fn(f64) -> f64> {
    let f: fn(f64) -> f64 = match name {
        "sqrt" => f64::sqrt,
        "cbrt" => f64::cbrt,
        "abs" => f64::abs,
        "floor" => f64::floor,
        "ceil" => f64::ceil,
        "round" => |v| java_round(v) as f64,
        "sin" => |v: f64| v.to_radians().sin(),
        "cos" => |v: f64| v.to_radians().cos(),
        "tan" => |v: f64| v.to_radians().tan(),
        "log" => f64::log10,
        "ln" => f64::ln,
        _ => return None,
    };
    Some(f)
}

/// Java's `Math.round(double)`: half up, NaN is 0, clamped to the long range.
pub fn java_round(v: f64) -> i64 {
    if v.is_nan() {
        return 0;
    }
    let f = v.floor();
    (if v - f >= 0.5 { f + 1.0 } else { f }) as i64
}

/// Java's `Character.isWhitespace`.
fn space(c: char) -> bool {
    matches!(c, '\t'..='\r' | '\u{1c}'..='\u{1f}')
        || (c.is_whitespace() && !matches!(c, '\u{85}' | '\u{a0}' | '\u{2007}' | '\u{202f}'))
}

/// Java's `Character.isDigit`, for the digits a number can hold (parseDouble
/// takes only ASCII ones; Java reports other digits as a bad number).
fn digit(c: char) -> bool {
    c.is_ascii_digit()
}

/// Java's `Character.isLetter`.
fn letter(c: char) -> bool {
    c.is_alphabetic()
}

/// The parser's state: the lowercased input as chars and the position.
struct Parser<'a> {
    s: Vec<char>,
    pos: usize,
    vars: &'a dyn Fn(&str) -> Option<f64>,
}

type Result<T> = std::result::Result<T, String>;

/// Calc.eval: the value of `expression`, or the error Java would throw.
/// `vars` answers the player's names (x, y, z, health, ans).
pub fn eval(expression: &str, vars: &dyn Fn(&str) -> Option<f64>) -> Result<f64> {
    let mut p = Parser {
        s: expression.to_lowercase().chars().collect(),
        pos: 0,
        vars,
    };
    let v = p.sum()?;
    p.skip();
    if p.pos < p.s.len() {
        return Err(format!("Unexpected '{}'", p.s[p.pos]));
    }
    if v.is_nan() {
        return Err("Not a number".into());
    }
    Ok(v)
}

impl Parser<'_> {
    fn skip(&mut self) {
        while self.pos < self.s.len() && space(self.s[self.pos]) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, chars: &str) -> bool {
        self.skip();
        if self.pos < self.s.len() && chars.contains(self.s[self.pos]) {
            self.pos += 1;
            return true;
        }
        false
    }

    /// The next char, or '\0' at the end.
    fn peek(&mut self) -> char {
        self.skip();
        self.s.get(self.pos).copied().unwrap_or('\0')
    }

    fn at(&self, i: usize) -> Option<char> {
        self.s.get(i).copied()
    }

    fn sum(&mut self) -> Result<f64> {
        let mut v = self.product()?;
        loop {
            if self.eat("+") {
                v += self.product()?;
            } else if self.eat("-−") {
                v -= self.product()?;
            } else {
                return Ok(v);
            }
        }
    }

    fn product(&mut self) -> Result<f64> {
        let mut v = self.unary()?;
        loop {
            if self.eat("*×∙·") {
                v *= self.unary()?;
            } else if self.eat("/:÷") {
                let d = self.unary()?;
                if d == 0.0 {
                    return Err("Division by zero".into());
                }
                v /= d;
            } else if self.eat("#") {
                v %= self.unary()?;
            } else if self.starts_implicit() {
                // 2sb, 3(4+1), 2pi
                v *= self.unary()?;
            } else {
                return Ok(v);
            }
        }
    }

    fn starts_implicit(&mut self) -> bool {
        let c = self.peek();
        c == '(' || c == '√' || letter(c)
    }

    /// Sign and roots bind weaker than ^, so -2^2 = -4.
    fn unary(&mut self) -> Result<f64> {
        if self.eat("-−") {
            return Ok(-self.unary()?);
        }
        if self.eat("+") {
            return self.unary();
        }
        if self.eat("√") {
            return Ok(self.unary()?.sqrt());
        }
        if self.eat("∛") {
            return Ok(self.unary()?.cbrt());
        }
        self.power()
    }

    fn power(&mut self) -> Result<f64> {
        let p = self.primary()?;
        let base = self.postfix(p)?;
        if self.eat("^") {
            // right associative, 2^-1 allowed
            return Ok(base.powf(self.unary()?));
        }
        Ok(base)
    }

    fn postfix(&mut self, mut v: f64) -> Result<f64> {
        loop {
            if self.eat("!") {
                v = factorial(v)?;
            } else if self.eat("%") {
                v /= 100.0;
            } else {
                return Ok(v);
            }
        }
    }

    fn primary(&mut self) -> Result<f64> {
        self.skip();
        if self.eat("(") {
            let v = self.sum()?;
            if !self.eat(")") {
                return Err("Missing ')'".into());
            }
            return Ok(v);
        }
        let start = self.pos;
        let c = self.peek();
        if digit(c) || c == '.' {
            while self
                .at(self.pos)
                .is_some_and(|c| digit(c) || c == '.' || c == '_')
            {
                self.pos += 1;
            }
            // scientific notation 1e5 / 2.5e-3 (but "2e" alone stays 2 * e)
            if self.pos + 1 < self.s.len() && self.s[self.pos] == 'e' {
                let mut p = self.pos + 1;
                if (self.s[p] == '-' || self.s[p] == '+') && p + 1 < self.s.len() {
                    p += 1;
                }
                if digit(self.s[p]) {
                    self.pos = p;
                    while self.at(self.pos).is_some_and(digit) {
                        self.pos += 1;
                    }
                }
            }
            let text: String = self.s[start..self.pos].iter().collect();
            let mut v: f64 = text
                .replace('_', "")
                .parse()
                .map_err(|_| format!("Bad number '{text}'"))?;
            if self.at(self.pos) == Some('k') && !self.next_is_letter(self.pos + 1) {
                self.pos += 1;
                v *= 1000.0;
            }
            return Ok(v);
        }
        if letter(c) {
            while self.at(self.pos).is_some_and(|c| letter(c) || digit(c)) {
                self.pos += 1;
            }
            let name: String = self.s[start..self.pos].iter().collect();
            if let Some(f) = function(&name) {
                if !self.eat("(") {
                    return Err(format!("{name} needs ( )"));
                }
                let arg = self.sum()?;
                if self.eat(",") {
                    let second = self.sum()?;
                    if !self.eat(")") {
                        return Err("Missing ')'".into());
                    }
                    return Err(format!(
                        "{name} takes one value, got {}, {}",
                        fmt(arg),
                        fmt(second)
                    ));
                }
                if !self.eat(")") {
                    return Err("Missing ')'".into());
                }
                return Ok(f(arg));
            }
            if (name == "min" || name == "max") && self.eat("(") {
                let a = self.sum()?;
                if !self.eat(",") {
                    return Err(format!("{name}(a, b)"));
                }
                let b = self.sum()?;
                if !self.eat(")") {
                    return Err("Missing ')'".into());
                }
                return Ok(if name == "min" {
                    java_min(a, b)
                } else {
                    java_max(a, b)
                });
            }
            return constant(&name)
                .or_else(|| (self.vars)(&name))
                .ok_or_else(|| format!("Unknown name '{name}'"));
        }
        Err(if c == '\0' {
            "Expression ends too early".into()
        } else {
            format!("Unexpected '{c}'")
        })
    }

    fn next_is_letter(&self, i: usize) -> bool {
        self.at(i).is_some_and(letter)
    }
}

/// Java's `Math.min`: NaN wins, -0.0 is below 0.0.
fn java_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() { a } else { b }
    } else {
        a.min(b)
    }
}

/// Java's `Math.max`.
fn java_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() { b } else { a }
    } else {
        a.max(b)
    }
}

fn factorial(v: f64) -> Result<f64> {
    if v < 0.0 || v != v.floor() {
        return Err("! needs a whole number >= 0".into());
    }
    if v > 170.0 {
        return Ok(f64::INFINITY);
    }
    let mut r = 1.0;
    let mut i = 2.0;
    while i <= v {
        r *= i;
        i += 1.0;
    }
    Ok(r)
}

/// Calc.fmt: plain number for copying, no grouping, up to 6 decimals.
pub fn fmt(v: f64) -> String {
    if v.is_infinite() {
        return if v > 0.0 { "∞" } else { "-∞" }.into();
    }
    if (v - v.round_ties_even()).abs() < 1e-9 && v.abs() < 1e15 {
        return java_round(v).to_string();
    }
    let out = fixed6(v);
    let out = out.trim_end_matches('0');
    out.strip_suffix('.').unwrap_or(out).to_owned()
}

/// Java's `String.format(Locale.ROOT, "%.6f", v)` for a finite `v`: Java
/// rounds the shortest decimal form of `v` half up (0.0078125 → 0.007813),
/// not its exact binary value as Rust's `{:.6}` does.
fn fixed6(v: f64) -> String {
    // "d.ddde±x": the shortest digits and the exponent
    let sci = format!("{:e}", v.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let mut digits: Vec<u8> = mantissa
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
    // digits before the point
    let mut point = exp.parse::<i64>().unwrap_or(0) + 1;
    let keep = point + 6;
    if keep < digits.len() as i64 {
        let up = keep >= 0 && digits[keep as usize] >= 5;
        digits.truncate(keep.max(0) as usize);
        if up {
            let mut i = digits.len();
            loop {
                if i == 0 {
                    digits.insert(0, 1);
                    point += 1;
                    break;
                }
                i -= 1;
                if digits[i] == 9 {
                    digits[i] = 0;
                } else {
                    digits[i] += 1;
                    break;
                }
            }
        }
    }
    // the value times 10^6 as a whole number, at least 7 digits
    let len = (point + 6).max(0) as usize;
    digits.resize(len, 0);
    while digits.len() < 7 {
        digits.insert(0, 0);
    }
    let text: String = digits.iter().map(|d| char::from(b'0' + d)).collect();
    let (int, frac) = text.split_at(text.len() - 6);
    format!(
        "{}{int}.{frac}",
        if v.is_sign_negative() { "-" } else { "" }
    )
}

/// Calc.items: "1 shulker + 3 stacks + 5" for whole item counts >= 64.
pub fn items(v: f64) -> Option<String> {
    if (v - v.round_ties_even()).abs() > 1e-9 || v < 64.0 || v > 1e12 {
        return None;
    }
    let n = java_round(v);
    let (sb, stacks, rest) = (n / 1728, n % 1728 / 64, n % 64);
    let mut b = String::new();
    let plural = |k: i64, one: &str, many: &str| format!("{k}{}", if k == 1 { one } else { many });
    if sb > 0 {
        b += &plural(sb, " shulker", " shulkers");
    }
    if stacks > 0 {
        b += if b.is_empty() { "" } else { " + " };
        b += &plural(stacks, " stack", " stacks");
    }
    if rest > 0 {
        b += if b.is_empty() { "" } else { " + " };
        b += &rest.to_string();
    }
    if sb > 0 {
        let more = if n % 64 > 0 {
            format!(" + {}", n % 64)
        } else {
            String::new()
        };
        b += &format!("  ({}{more})", plural(n / 64, " stack", " stacks"));
    }
    Some(b)
}

thread_local! {
    /// `ans`: the last result of /calc (Java keeps it per session).
    static ANS: Cell<f64> = const { Cell::new(0.0) };
}

/// The player's names: x, y, z (block), ans. `health` needs a hook Horizon
/// doesn't offer yet, so it is unknown.
fn vars(player: Option<core::Player>) -> impl Fn(&str) -> Option<f64> {
    move |name| match name {
        "x" => player.as_ref().map(|p| p.x.floor()),
        "y" => player.as_ref().map(|p| p.y.floor()),
        "z" => player.as_ref().map(|p| p.z.floor()),
        "ans" => Some(ANS.with(Cell::get)),
        _ => None,
    }
}

/// Java's `String.trim`: chars up to the space off both ends.
fn java_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// `/calc`, `/calc <math>` and its tools.
fn command(host: &mut dyn Host, name: &str, args: &str) -> bool {
    if name == "craft" {
        if !core::active(host, &MODULE) {
            core::error(host, "Calculator module is disabled in Omnitool.");
            return true;
        }
        return super::omnitool_craft_ui::command(host, name, args);
    }
    if name != "calc" {
        return false;
    }
    if java_trim(args).is_empty() {
        help(host);
    } else {
        run(host, args);
    }
    true
}

pub const HELP: &[&str] = &[
    " Just type math in chat (164*9, 5sb/3, 2^10, √81, 12%*300, 7!) - result shows above, Tab inserts it",
    " /calc <math> prints it to chat, click the green result to copy",
    " Units: stack sb dub (+16/1 variants), min, h, day. Vars: x y z health ans",
    " /calc nether|overworld [x y z], /calc dist x y z [x2 y2 z2], /calc rates <items> <seconds>",
];

fn help(host: &mut dyn Host) {
    host.chat("§6Omnitool calculator");
    for line in HELP {
        host.chat(&format!("§7{line}"));
    }
}

fn run(host: &mut dyn Host, input: &str) {
    if !core::active(host, &MODULE) {
        return core::error(host, "Calculator module is disabled in Omnitool.");
    }
    let player = host.player();
    let vars = vars(player.clone());
    // Java: input.trim().split("\\s+")
    let w: Vec<&str> = java_trim(input)
        .split([' ', '\t', '\n', '\u{b}', '\u{c}', '\r'])
        .filter(|s| !s.is_empty())
        .collect();
    let nums = |from: usize, count: usize| -> Result<Vec<f64>> {
        (from..from + count).map(|i| eval(w[i], &vars)).collect()
    };
    let here = || -> Result<Vec<f64>> {
        let p = player.as_ref().ok_or("Not in a world")?;
        Ok(vec![p.x, p.y, p.z])
    };
    let result = (|| -> Result<()> {
        match w[0].to_lowercase().as_str() {
            sub @ ("nether" | "overworld") => {
                let to_nether = sub == "nether";
                let c = if w.len() >= 4 { nums(1, 3)? } else { here()? };
                let f = if to_nether { 1.0 / 8.0 } else { 8.0 };
                let r = format!(
                    "{} {} {}",
                    fmt((c[0] * f).floor()),
                    fmt(c[1].floor()),
                    fmt((c[2] * f).floor())
                );
                send(
                    host,
                    if to_nether { "Nether: " } else { "Overworld: " },
                    &r,
                    None,
                );
            }
            "dist" => {
                let (a, b) = if w.len() >= 7 {
                    (nums(1, 3)?, nums(4, 3)?)
                } else if w.len() >= 4 {
                    (here()?, nums(1, 3)?)
                } else {
                    return Err("/calc dist x y z [x2 y2 z2]".into());
                };
                let (dx, dy, dz) = (b[0] - a[0], b[1] - a[1], b[2] - a[2]);
                let flat = fmt(java_round((dx * dx + dz * dz).sqrt() * 10.0) as f64 / 10.0);
                let full =
                    fmt(java_round((dx * dx + dy * dy + dz * dz).sqrt() * 10.0) as f64 / 10.0);
                send(
                    host,
                    "Distance: ",
                    &flat,
                    Some(&format!(" blocks (3D: {full})")),
                );
            }
            "rates" => {
                if w.len() < 3 {
                    return Err("/calc rates <items> <seconds>".into());
                }
                let v = nums(1, 2)?;
                if v[1] <= 0.0 {
                    return Err("Seconds must be > 0".into());
                }
                let ans = java_round(v[0] / v[1] * 3600.0) as f64;
                ANS.with(|a| a.set(ans));
                let r = fmt(ans);
                let more = items(ans).map(|i| format!(" = {i}")).unwrap_or_default();
                send(host, "Rate: ", &r, Some(&format!(" items/hour{more}")));
            }
            _ => {
                let ans = eval(input, &vars)?;
                ANS.with(|a| a.set(ans));
                let r = fmt(ans);
                let more = items(ans).map(|i| format!("  = {i}"));
                send(
                    host,
                    &format!("{} = ", java_trim(input)),
                    &r,
                    more.as_deref(),
                );
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        core::error(host, &format!("Calc: {e}"));
    }
}

/// A grey line with the result in green (Java: click to copy, which plain
/// chat lines can't do yet).
fn send(host: &mut dyn Host, prefix: &str, shown: &str, suffix: Option<&str>) {
    let suffix = suffix.map(|s| format!("§7{s}")).unwrap_or_default();
    host.chat(&format!("§7{prefix}§a{shown}{suffix}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::{Player, tests::Fake};

    fn fixtures() -> serde_json::Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/calc.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// Java's Double.toString as the fixtures hold it.
    fn num(v: &serde_json::Value) -> f64 {
        v.as_str().unwrap().parse().unwrap()
    }

    #[test]
    fn eval_matches_java() {
        let f = fixtures();
        let vars = |n: &str| f["vars"].get(n).map(num);
        for case in f["eval"].as_array().unwrap() {
            let input = case["input"].as_str().unwrap();
            let got = eval(input, &vars);
            match case.get("error") {
                Some(e) => assert_eq!(got, Err(e.as_str().unwrap().to_owned()), "{input}"),
                None => {
                    let v = got.unwrap_or_else(|e| panic!("{input}: {e}"));
                    let want = num(&case["value"]);
                    let close = v == want || (v - want).abs() <= want.abs() * 1e-15;
                    assert!(close, "{input}: {v} != {want}");
                    assert_eq!(fmt(v), case["shown"].as_str().unwrap(), "{input}");
                }
            }
        }
    }

    #[test]
    fn fmt_matches_java() {
        for case in fixtures()["fmt"].as_array().unwrap() {
            let v = num(&case["value"]);
            assert_eq!(fmt(v), case["out"].as_str().unwrap(), "{v:e}");
        }
    }

    #[test]
    fn items_matches_java() {
        for case in fixtures()["items"].as_array().unwrap() {
            let v = num(&case["value"]);
            assert_eq!(items(v).as_deref(), case["out"].as_str(), "{v}");
        }
    }

    fn calc(h: &mut Fake, line: &str) -> Vec<String> {
        h.lines.clear();
        assert!(core::command(h, line));
        h.lines.clone()
    }

    fn at(x: f64, y: f64, z: f64) -> Option<Player> {
        Some(Player {
            x,
            y,
            z,
            yaw: 0.0,
            pitch: 0.0,
            dimension: "minecraft:overworld".into(),
        })
    }

    #[test]
    fn calc_command() {
        let mut h = Fake {
            player: at(800.7, 64.2, -80.5),
            ..Fake::default()
        };
        assert_eq!(calc(&mut h, "calc")[0], "§6Omnitool calculator");
        assert_eq!(
            calc(&mut h, "calc  5sb/3 "),
            ["§75sb/3 = §a2880§7  = 1 shulker + 18 stacks  (45 stacks)"]
        );
        assert_eq!(
            calc(&mut h, "calc ans+1"),
            ["§7ans+1 = §a2881§7  = 1 shulker + 18 stacks + 1  (45 stacks + 1)"]
        );
        assert_eq!(calc(&mut h, "calc 1/3"), ["§71/3 = §a0.333333"]);
        assert_eq!(calc(&mut h, "calc x"), ["§7x = §a800§7  = 12 stacks + 32"]);
        assert_eq!(calc(&mut h, "calc 10/0"), ["§cCalc: Division by zero"]);
        assert_eq!(calc(&mut h, "calc Nether"), ["§7Nether: §a100 64 -11"]);
        assert_eq!(
            calc(&mut h, "calc overworld 10 70 -3"),
            ["§7Overworld: §a80 70 -24"]
        );
        assert_eq!(
            calc(&mut h, "calc dist 0 0 0 3 4 4"),
            ["§7Distance: §a5§7 blocks (3D: 6.4)"]
        );
        assert_eq!(
            calc(&mut h, "calc dist 800 64 -80"),
            ["§7Distance: §a0.9§7 blocks (3D: 0.9)"]
        );
        assert_eq!(
            calc(&mut h, "calc dist 1 2"),
            ["§cCalc: /calc dist x y z [x2 y2 z2]"]
        );
        assert_eq!(
            calc(&mut h, "calc rates 64 60"),
            ["§7Rate: §a3840§7 items/hour = 2 shulkers + 6 stacks  (60 stacks)"]
        );
        assert_eq!(
            calc(&mut h, "calc rates 1 0"),
            ["§cCalc: Seconds must be > 0"]
        );
        assert_eq!(
            calc(&mut h, "calc rates 1"),
            ["§cCalc: /calc rates <items> <seconds>"]
        );
        assert_eq!(
            calc(&mut h, "calc health"),
            ["§cCalc: Unknown name 'health'"]
        );
        h.player = None;
        assert_eq!(calc(&mut h, "calc nether"), ["§cCalc: Not in a world"]);
        assert_eq!(calc(&mut h, "calc x"), ["§cCalc: Unknown name 'x'"]);
        assert_eq!(calc(&mut h, "ot off calculator").len(), 1);
        assert_eq!(
            calc(&mut h, "calc 1+1"),
            ["§cCalculator module is disabled in Omnitool."]
        );
    }
}
