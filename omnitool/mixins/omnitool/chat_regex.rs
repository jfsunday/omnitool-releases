// A small backtracking regex for the chat filter's `re:` rules (Java's
// `Pattern` with CASE_INSENSITIVE | UNICODE_CASE). The bundle has std only,
// so there is no `regex` crate. Supported: literals, `.`, classes `[a-z]`
// (also negated, with `\d\w\s` inside), `\d \D \w \W \s \S \b \B \p{..}`,
// `^ $`, groups `( )` `(?: )` `(?i)`, alternation, `* + ? {n,m}` with lazy
// `?` (possessive `+` is read as greedy). No back references or lookaround;
// those are errors here ("Unsupported ..."). Errors carry Java's wording
// where the case is common. Added to the game as `game::omnitool_chat_regex`;
// also a module of this crate.

#[derive(Debug)]
enum Node {
    Char(char),
    Any,
    Class(Vec<Item>, bool),
    Start,
    End,
    WordB(bool),
    Alt(Vec<Node>),
    Seq(Vec<Node>),
    Repeat(Box<Node>, usize, usize, bool),
}

#[derive(Debug, Clone)]
enum Item {
    Range(char, char),
    Digit(bool),
    Word(bool),
    Space(bool),
    Prop(&'static str, bool),
}

#[derive(Debug)]
pub struct Regex {
    root: Node,
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn prop(name: &str, c: char) -> bool {
    match name {
        "L" | "Alpha" | "IsAlphabetic" => c.is_alphabetic(),
        "Lu" | "Upper" => c.is_uppercase(),
        "Ll" | "Lower" => c.is_lowercase(),
        "N" | "Nd" | "Digit" => c.is_numeric(),
        "Alnum" => c.is_alphanumeric(),
        "Punct" | "P" => c.is_ascii_punctuation(),
        "Space" => c.is_whitespace(),
        _ => false,
    }
}

const PROPS: [&str; 12] = [
    "L",
    "Alpha",
    "IsAlphabetic",
    "Lu",
    "Upper",
    "Ll",
    "Lower",
    "N",
    "Nd",
    "Digit",
    "Alnum",
    "Punct",
];

impl Item {
    fn matches(&self, c: char, icase: bool) -> bool {
        match self {
            Item::Range(a, b) => {
                let hit = |c: char| *a <= c && c <= *b;
                hit(c) || (icase && (hit(fold(c)) || c.to_uppercase().next().is_some_and(hit)))
            }
            Item::Digit(neg) => c.is_ascii_digit() != *neg,
            Item::Word(neg) => (c.is_ascii_alphanumeric() || c == '_') != *neg,
            Item::Space(neg) => matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c') != *neg,
            Item::Prop(name, neg) => prop(name, c) != *neg,
        }
    }
}

struct Parser {
    s: Vec<char>,
    i: usize,
}

type PResult<T> = Result<T, String>;

impl Parser {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn alt(&mut self, depth: usize) -> PResult<Node> {
        let mut branches = vec![self.seq(depth)?];
        while self.peek() == Some('|') {
            self.i += 1;
            branches.push(self.seq(depth)?);
        }
        Ok(if branches.len() == 1 {
            branches.pop().unwrap_or(Node::Seq(Vec::new()))
        } else {
            Node::Alt(branches)
        })
    }

    fn seq(&mut self, depth: usize) -> PResult<Node> {
        let mut nodes = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || (c == ')' && depth > 0) {
                break;
            }
            if c == ')' {
                return Err(format!("Unmatched closing ')' near index {}", self.i));
            }
            let atom = self.atom(depth)?;
            let atom = self.quant(atom)?;
            nodes.push(atom);
        }
        Ok(Node::Seq(nodes))
    }

    fn quant(&mut self, atom: Node) -> PResult<Node> {
        let (min, max) = match self.peek() {
            Some('*') => (0, usize::MAX),
            Some('+') => (1, usize::MAX),
            Some('?') => (0, 1),
            Some('{') => {
                let start = self.i;
                let close = self.s[self.i..]
                    .iter()
                    .position(|c| *c == '}')
                    .ok_or_else(|| "Unclosed counted closure".to_owned())?;
                let body: String = self.s[self.i + 1..self.i + close].iter().collect();
                let num = |t: &str| t.trim().parse::<usize>().ok();
                let bounds = match body.split_once(',') {
                    None => num(&body).map(|n| (n, n)),
                    Some((a, "")) => num(a).map(|n| (n, usize::MAX)),
                    Some((a, b)) => num(a).zip(num(b)),
                };
                let Some((lo, hi)) = bounds else {
                    return Err(format!("Illegal repetition near index {start}"));
                };
                if hi < lo {
                    return Err("Illegal repetition range".to_owned());
                }
                self.i += close;
                (lo, hi)
            }
            _ => return Ok(atom),
        };
        self.i += 1;
        let lazy = self.peek() == Some('?');
        if lazy || self.peek() == Some('+') {
            self.i += 1;
        }
        if matches!(atom, Node::Start | Node::End | Node::WordB(_)) {
            return Ok(atom);
        }
        Ok(Node::Repeat(Box::new(atom), min, max, lazy))
    }

    fn atom(&mut self, depth: usize) -> PResult<Node> {
        let c = self.s[self.i];
        self.i += 1;
        match c {
            '(' => {
                if self.peek() == Some('?') {
                    match self.s.get(self.i + 1) {
                        Some(':') => self.i += 2,
                        Some('i') if self.s.get(self.i + 2) == Some(&')') => {
                            self.i += 3;
                            return Ok(Node::Seq(Vec::new()));
                        }
                        _ => return Err("Unsupported group construct".to_owned()),
                    }
                }
                let inner = self.alt(depth + 1)?;
                if self.peek() != Some(')') {
                    return Err(format!("Unclosed group near index {}", self.s.len()));
                }
                self.i += 1;
                Ok(inner)
            }
            '[' => self.class(),
            '.' => Ok(Node::Any),
            '^' => Ok(Node::Start),
            '$' => Ok(Node::End),
            '*' | '+' | '?' => Err(format!(
                "Dangling meta character '{c}' near index {}",
                self.i - 1
            )),
            '\\' => self.escape(),
            c => Ok(Node::Char(c)),
        }
    }

    /// The escape after a backslash; `Item`-like ones become one-item classes.
    fn escape(&mut self) -> PResult<Node> {
        let Some(c) = self.peek() else {
            return Err("Unexpected internal error".to_owned());
        };
        self.i += 1;
        if c == 'b' || c == 'B' {
            return Ok(Node::WordB(c == 'b'));
        }
        if let Some(item) = self.class_escape(c)? {
            return Ok(Node::Class(vec![item], false));
        }
        Ok(Node::Char(self.char_escape(c)?))
    }

    fn class_escape(&mut self, c: char) -> PResult<Option<Item>> {
        Ok(Some(match c {
            'd' => Item::Digit(false),
            'D' => Item::Digit(true),
            'w' => Item::Word(false),
            'W' => Item::Word(true),
            's' => Item::Space(false),
            'S' => Item::Space(true),
            'p' | 'P' => {
                if self.peek() != Some('{') {
                    return Err("Unknown character property name".to_owned());
                }
                let close = self.s[self.i..].iter().position(|c| *c == '}');
                let Some(close) = close else {
                    return Err("Unclosed character family".to_owned());
                };
                let name: String = self.s[self.i + 1..self.i + close].iter().collect();
                self.i += close + 1;
                let known = PROPS.iter().find(|p| **p == name);
                match known {
                    Some(p) => Item::Prop(p, c == 'P'),
                    None => return Err(format!("Unknown character property name {{{name}}}")),
                }
            }
            _ => return Ok(None),
        }))
    }

    fn char_escape(&mut self, c: char) -> PResult<char> {
        let hex = |p: &mut Parser, n: usize| -> PResult<char> {
            let text: String = p.s.iter().skip(p.i).take(n).collect();
            let v = u32::from_str_radix(&text, 16)
                .ok()
                .filter(|_| text.len() == n);
            p.i += n;
            v.and_then(char::from_u32)
                .ok_or_else(|| "Illegal hexadecimal escape sequence".to_owned())
        };
        Ok(match c {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            'f' => '\x0c',
            'e' => '\x1b',
            'x' => hex(self, 2)?,
            'u' => hex(self, 4)?,
            c if c.is_ascii_alphabetic() => {
                return Err(format!(
                    "Illegal/unsupported escape sequence near index {}",
                    self.i - 1
                ));
            }
            c if c.is_ascii_digit() => return Err("Back references are not supported".to_owned()),
            c => c,
        })
    }

    fn class(&mut self) -> PResult<Node> {
        let neg = self.peek() == Some('^');
        if neg {
            self.i += 1;
        }
        let mut items = Vec::new();
        let mut first = true;
        loop {
            let Some(c) = self.peek() else {
                return Err(format!(
                    "Unclosed character class near index {}",
                    self.s.len() - 1
                ));
            };
            self.i += 1;
            if c == ']' && !first {
                break;
            }
            first = false;
            let lo = if c == '\\' {
                let Some(e) = self.peek() else {
                    return Err("Unexpected internal error".to_owned());
                };
                self.i += 1;
                if let Some(item) = self.class_escape(e)? {
                    items.push(item);
                    continue;
                }
                self.char_escape(e)?
            } else if c == '[' || (c == '&' && self.peek() == Some('&')) {
                return Err("Unsupported character class construct".to_owned());
            } else {
                c
            };
            // a range `lo-hi` unless the dash ends the class
            if self.peek() == Some('-') && self.s.get(self.i + 1).is_some_and(|c| *c != ']') {
                self.i += 1;
                let mut hi = self.s[self.i];
                self.i += 1;
                if hi == '\\' {
                    let Some(e) = self.peek() else {
                        return Err("Unexpected internal error".to_owned());
                    };
                    self.i += 1;
                    hi = self.char_escape(e)?;
                }
                if hi < lo {
                    return Err("Illegal character range".to_owned());
                }
                items.push(Item::Range(lo, hi));
            } else {
                items.push(Item::Range(lo, lo));
            }
        }
        Ok(Node::Class(items, neg))
    }
}

impl Regex {
    /// Compiles `pattern` case-insensitively; the error is Java's
    /// `PatternSyntaxException.getDescription()` where known.
    pub fn new(pattern: &str) -> Result<Regex, String> {
        let mut p = Parser {
            s: pattern.chars().collect(),
            i: 0,
        };
        let root = p.alt(0)?;
        if p.i < p.s.len() {
            return Err(format!("Unmatched closing ')' near index {}", p.i));
        }
        Ok(Regex { root })
    }

    /// Whether the pattern matches anywhere in `text` (`Matcher.find`).
    pub fn find(&self, text: &str) -> bool {
        let t: Vec<char> = text.chars().collect();
        (0..=t.len()).any(|start| m(&self.root, &t, start, &mut |_| true))
    }
}

type Cont<'a> = &'a mut dyn FnMut(usize) -> bool;

fn is_word_at(t: &[char], i: usize) -> bool {
    t.get(i).is_some_and(|c| word_char(*c))
}

fn m(node: &Node, t: &[char], i: usize, k: Cont) -> bool {
    match node {
        Node::Char(c) => t.get(i).is_some_and(|x| fold(*x) == fold(*c)) && k(i + 1),
        Node::Any => {
            t.get(i)
                .is_some_and(|c| !matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'))
                && k(i + 1)
        }
        Node::Class(items, neg) => {
            t.get(i)
                .is_some_and(|c| items.iter().any(|it| it.matches(*c, true)) != *neg)
                && k(i + 1)
        }
        Node::Start => i == 0 && k(i),
        Node::End => (i == t.len() || (i + 1 == t.len() && t[i] == '\n')) && k(i),
        Node::WordB(want) => {
            let edge = (i > 0 && is_word_at(t, i - 1)) != is_word_at(t, i);
            edge == *want && k(i)
        }
        Node::Alt(branches) => branches.iter().any(|b| m(b, t, i, k)),
        Node::Seq(nodes) => seq(nodes, t, i, k),
        Node::Repeat(inner, min, max, lazy) => rep(inner, *min, *max, *lazy, 0, t, i, k),
    }
}

fn seq(nodes: &[Node], t: &[char], i: usize, k: Cont) -> bool {
    match nodes.split_first() {
        None => k(i),
        Some((first, rest)) => m(first, t, i, &mut |j| seq(rest, t, j, k)),
    }
}

#[allow(clippy::too_many_arguments)]
fn rep(
    node: &Node,
    min: usize,
    max: usize,
    lazy: bool,
    n: usize,
    t: &[char],
    i: usize,
    k: Cont,
) -> bool {
    let more = |k: Cont| {
        n < max
            && m(node, t, i, &mut |j| {
                // an empty iteration past the minimum would loop forever
                (j > i || n < min) && rep(node, min, max, lazy, n + 1, t, j, k)
            })
    };
    if n < min {
        return more(k);
    }
    #[allow(clippy::if_same_then_else)]
    if lazy {
        k(i) || more(k)
    } else {
        more(k) || k(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(p: &str, t: &str) -> bool {
        Regex::new(p).unwrap().find(t)
    }

    #[test]
    fn basics() {
        assert!(find("^\\[VIP\\]", "[vip] Steve"));
        assert!(!find("^\\[VIP\\]", "Steve [VIP]"));
        assert!(find("a;b", "a;b"));
        assert!(find("^hi", "HI there"));
        assert!(find("gr(a|e)y", "GREY"));
        assert!(find("\\d{2,3}x", "a 1234x"));
        assert!(!find("^\\d{2,3}x", "1234x"));
        assert!(find("[^a-c]+z", "abcdz"));
        assert!(find("a.*?b", "xxaxxb"));
        assert!(find("\\bcat\\b", "a cat sat"));
        assert!(!find("\\bcat\\b", "concat"));
        assert!(find("(?:ab)+$", "xxabab"));
        assert!(find("(a*)*b", "aaab"));
        assert!(find("\\p{Lu}", "xÄ"));
    }

    #[test]
    fn errors() {
        assert_eq!(
            Regex::new("(unclosed").unwrap_err().split(" near").next(),
            Some("Unclosed group")
        );
        assert!(
            Regex::new("*a")
                .unwrap_err()
                .starts_with("Dangling meta character")
        );
        assert!(
            Regex::new("[abc")
                .unwrap_err()
                .starts_with("Unclosed character class")
        );
        assert!(Regex::new("a)").is_err());
        assert!(Regex::new("a{2,1}").is_err());
    }
}
