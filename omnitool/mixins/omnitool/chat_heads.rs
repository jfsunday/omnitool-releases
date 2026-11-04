// Chat Heads (docs/spec/omnitool/chat.md, section 5): the sender's head next
// to chat lines. Here: the decision logic (who sent a line, which wrapped
// line gets the head, sizes, the quads of one head) as pure code. Drawing
// the heads, wrapping chat lines narrower and the skin textures need
// Horizon's chat renderer (horizon-client-render/src/chat.rs, the line
// drawing) and the tab list's skins; no hook gives a mod either, so the
// module only holds its settings in the game. Off by default. Added to the
// game as `game::omnitool_chat_heads`; also a module of this crate.

use super::omnitool_core::{Category, ModuleInfo};
use super::omnitool_setting::{self as setting, Kind, SettingDef};

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "chat_heads",
    name: "Chat Heads",
    category: Category::Hud,
    description: "Shows the sender's head next to chat messages.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "Chat Heads",
    settings: &[
        SettingDef::new(
            "render_position",
            "Head position",
            Kind::Enum {
                values: &["BEFORE_NAME", "BEFORE_LINE"],
                default: 0,
            },
        ),
        SettingDef::new(
            "three_deeness",
            "3D hat layer",
            Kind::Number {
                default: 0.0,
                min: 0.0,
                max: 1.0,
                step: 0.05,
            },
        ),
        SettingDef::new(
            "offset_non_player_text",
            "Indent lines without head (before line)",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "sender_detection",
            "Sender detection",
            Kind::Enum {
                values: &["UUID_ONLY", "HEURISTIC_ONLY", "UUID_AND_HEURISTIC"],
                default: 2,
            },
        ),
        SettingDef::new(
            "smart_heuristics",
            "Smart heuristics (no guessing once the server sends UUIDs)",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "system_messages",
            "Heads for system messages",
            Kind::Bool(true),
        ),
        SettingDef::new("shadow", "Head shadow", Kind::Bool(true)),
        SettingDef::new(
            "name_aliases",
            "Name aliases (alias=player, ...)",
            Kind::Text(""),
        ),
    ],
    command: None,
    key: setting::NONE,
    hold: false,
    hud: None,
    tick: None,
    on_key: None,
    hud_right: None,
    on_action: None,
};

#[allow(dead_code)]
const PADDING: i32 = 2;

/// Java's `Math.round` on a float.
#[allow(dead_code)]
fn round(v: f32) -> i32 {
    (v + 0.5).floor() as i32
}

/// `headWidth()`: 8 + 2 + round(2 * three_deeness).
#[allow(dead_code)]
pub fn head_width(three_deeness: f32) -> i32 {
    8 + PADDING + round(2.0 * three_deeness)
}

/// The head's offset: round(three_deeness).
#[allow(dead_code)]
pub fn head_pad(three_deeness: f32) -> i32 {
    round(three_deeness)
}

#[allow(dead_code)]
fn word(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// `wordIndex` (spec 6.7): the first place of `name` in `text` (in
/// characters) that isn't inside a longer word; -1 if none.
#[allow(dead_code)]
pub fn word_index(text: &str, name: &str) -> i32 {
    let t: Vec<char> = text.chars().collect();
    let n: Vec<char> = name.chars().collect();
    if n.is_empty() || n.len() > t.len() {
        return -1;
    }
    for i in 0..=t.len() - n.len() {
        if t[i..i + n.len()] != n[..] {
            continue;
        }
        let end = i + n.len();
        let start_ok = i == 0 || !word(Some(t[i - 1])) || !word(n.first().copied());
        let end_ok = end >= t.len() || !word(Some(t[end])) || !word(n.last().copied());
        if start_ok && end_ok {
            return i as i32;
        }
    }
    -1
}

/// `scan`: the name found first in `text`, the longer one on a tie.
#[allow(dead_code)]
pub fn scan<'a>(text: &str, names: impl IntoIterator<Item = &'a str>) -> Option<(&'a str, i32)> {
    let mut best: Option<(&str, i32)> = None;
    for name in names {
        let i = word_index(text, name);
        if i < 0 {
            continue;
        }
        let better = match best {
            None => true,
            Some((b, bi)) => i < bi || (i == bi && name.chars().count() > b.chars().count()),
        };
        if better {
            best = Some((name, i));
        }
    }
    best
}

/// A player the connection knows (the tab list entry).
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub struct Player {
    pub id: String,
    pub profile_name: String,
    pub display_name: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Detection {
    UuidOnly,
    HeuristicOnly,
    UuidAndHeuristic,
}

/// The owner of a chat message: the player and where his name stands.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub struct Owner {
    pub player: String,
    pub name: Option<String>,
}

/// Who delivered the message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Context {
    Client,
    Chat,
    System,
}

#[allow(dead_code)]
fn strip(s: &str) -> String {
    super::omnitool_chat::plain(s)
}

/// The settings `detect` reads.
#[allow(dead_code)]
pub struct DetectSettings<'a> {
    pub detection: Detection,
    pub smart: bool,
    pub system_messages: bool,
    pub aliases: &'a str,
}

/// `addNames`: the profile name (wins) and the tab list name.
#[allow(dead_code)]
fn add_names<'a>(names: &mut Vec<(String, &'a Player)>, p: &'a Player) {
    let profile = strip(&p.profile_name);
    if !profile.is_empty() {
        match names.iter_mut().find(|n| n.0 == profile) {
            Some(n) => n.1 = p,
            None => names.push((profile, p)),
        }
    }
    if let Some(d) = &p.display_name {
        let d = strip(d).trim().to_owned();
        if !d.is_empty() && !names.iter().any(|n| n.0 == d) {
            names.push((d, p));
        }
    }
}

/// `detect` (spec 5.6). `given` is the tab list entry of the sender's UUID
/// (None when none). `server_sent_uuid` is the session flag it may set.
#[allow(dead_code)]
pub fn detect(
    message: &str,
    context: Context,
    given: Option<&Player>,
    online: &[Player],
    s: &DetectSettings,
    server_sent_uuid: &mut bool,
) -> Option<Owner> {
    let player = context == Context::Chat;
    if !player && !s.system_messages {
        return None;
    }
    let mut given = if player { given } else { None };
    match s.detection {
        Detection::HeuristicOnly => given = None,
        _ if given.is_some() => *server_sent_uuid = true,
        Detection::UuidOnly => return None,
        _ if *server_sent_uuid && s.smart => return None,
        _ => {}
    }
    let text = strip(message);
    let mut names: Vec<(String, &Player)> = Vec::new();
    match given {
        Some(p) => add_names(&mut names, p),
        None => online
            .iter()
            .filter(|p| !p.profile_name.starts_with("|slot_"))
            .for_each(|p| add_names(&mut names, p)),
    }
    for pair in s.aliases.split(',') {
        let Some((alias, target)) = pair.split_once('=').filter(|(a, _)| !a.trim().is_empty())
        else {
            continue;
        };
        let (alias, target) = (alias.trim(), target.trim());
        let hit = names
            .iter()
            .find(|n| n.0.to_lowercase() == target.to_lowercase())
            .map(|n| n.1);
        if let Some(p) = hit
            && !names.iter().any(|n| n.0 == alias)
        {
            names.push((alias.to_owned(), p));
        }
    }
    if let Some((found, _)) = scan(&text, names.iter().map(|n| n.0.as_str())) {
        let p = names.iter().find(|n| n.0 == found)?.1;
        return Some(Owner {
            player: p.id.clone(),
            name: Some(found.to_owned()),
        });
    }
    given.map(|p| Owner {
        player: p.id.clone(),
        name: None,
    })
}

/// What one wrapped line gets: the head of `player` (None = only indent)
/// after `split` characters.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub struct Line {
    pub player: Option<String>,
    pub split: usize,
}

/// `tag` (spec 5.7) for the plain texts of the wrapped lines.
#[allow(dead_code)]
pub fn tag(
    owner: Option<&Owner>,
    lines: &[&str],
    before_line: bool,
    offset_non_player: bool,
) -> Vec<Option<Line>> {
    let (mut head_line, mut split) = (0, 0);
    if let Some(name) = owner
        .and_then(|o| o.name.as_deref())
        .filter(|_| !before_line)
    {
        for (i, l) in lines.iter().enumerate() {
            let at = word_index(l, name);
            if at >= 0 {
                (head_line, split) = (i, at as usize);
                break;
            }
        }
    }
    (0..lines.len())
        .map(|i| match owner {
            Some(o) if i == head_line => Some(Line {
                player: Some(o.player.clone()),
                split,
            }),
            _ if before_line && offset_non_player => Some(Line {
                player: None,
                split: 0,
            }),
            _ => None,
        })
        .collect()
}

/// One drawing step of a head.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum Op {
    Quad {
        x: i32,
        y: i32,
        u: i32,
        v: i32,
        argb: u32,
    },
    PushScale {
        cx: f32,
        cy: f32,
        scale: f32,
    },
    Pop,
}

/// `paint` (spec 5.5 item 5): the steps that draw a head at `x, y`.
#[allow(dead_code)]
pub fn paint(x: i32, y: i32, hat: bool, three_deeness: f32, shadow: bool, alpha: f32) -> Vec<Op> {
    let a = round(alpha * 255.0).clamp(0, 255) as u32;
    let color = a << 24 | 0xFF_FFFF;
    let shadow_color = a << 24 | 0x40_4040;
    let mut ops = Vec::new();
    let mut layer = |x: i32, y: i32, argb: u32| {
        ops.push(Op::Quad {
            x,
            y,
            u: 8,
            v: 8,
            argb,
        });
        if hat {
            if three_deeness > 0.0 {
                ops.push(Op::PushScale {
                    cx: x as f32 + 4.0,
                    cy: y as f32 + 4.0,
                    scale: 1.0 + 0.25 * three_deeness,
                });
            }
            ops.push(Op::Quad {
                x,
                y,
                u: 40,
                v: 8,
                argb,
            });
            if three_deeness > 0.0 {
                ops.push(Op::Pop);
            }
        }
    };
    if shadow {
        layer(x + 1, y, shadow_color);
    }
    layer(x, y - i32::from(shadow), color);
    ops
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

    #[test]
    fn fixtures() {
        let f = &fx()["chat_heads"];
        for c in f["word_index"].as_array().unwrap() {
            let got = word_index(c["text"].as_str().unwrap(), c["name"].as_str().unwrap());
            assert_eq!(i64::from(got), c["expect"].as_i64().unwrap(), "{c}");
        }
        for c in f["scan"].as_array().unwrap() {
            let names: Vec<&str> = c["names"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_str().unwrap())
                .collect();
            let got = scan(c["text"].as_str().unwrap(), names).map(|g| g.0);
            assert_eq!(got, c["expect"].as_str(), "{c}");
        }
        for c in f["head_width"].as_array().unwrap() {
            assert_eq!(
                i64::from(head_width(c[0].as_f64().unwrap() as f32)),
                c[1].as_i64().unwrap()
            );
        }
        for c in f["head_pad"].as_array().unwrap() {
            assert_eq!(
                i64::from(head_pad(c[0].as_f64().unwrap() as f32)),
                c[1].as_i64().unwrap()
            );
        }
        for c in f["paint"].as_array().unwrap() {
            let got = paint(
                c["x"].as_i64().unwrap() as i32,
                c["y"].as_i64().unwrap() as i32,
                c["hat"].as_bool().unwrap(),
                c["three_deeness"].as_f64().unwrap() as f32,
                c["shadow"].as_bool().unwrap(),
                c["alpha"].as_f64().unwrap() as f32,
            );
            let want: Vec<Op> = c["ops"]
                .as_array()
                .unwrap()
                .iter()
                .map(|o| match o[0].as_str().unwrap() {
                    "quad" => Op::Quad {
                        x: o[1].as_i64().unwrap() as i32,
                        y: o[2].as_i64().unwrap() as i32,
                        u: o[3].as_i64().unwrap() as i32,
                        v: o[4].as_i64().unwrap() as i32,
                        argb: u32::from_str_radix(o[5].as_str().unwrap(), 16).unwrap(),
                    },
                    "push_scale" => Op::PushScale {
                        cx: o[1].as_f64().unwrap() as f32,
                        cy: o[2].as_f64().unwrap() as f32,
                        scale: o[3].as_f64().unwrap() as f32,
                    },
                    _ => Op::Pop,
                })
                .collect();
            assert_eq!(got, want, "{c}");
        }
    }

    fn p(id: &str, name: &str) -> Player {
        Player {
            id: id.into(),
            profile_name: name.into(),
            display_name: None,
        }
    }

    fn settings(detection: Detection) -> DetectSettings<'static> {
        DetectSettings {
            detection,
            smart: true,
            system_messages: true,
            aliases: "",
        }
    }

    #[test]
    fn detects_by_uuid_and_name() {
        let online = [p("1", "Steve"), p("2", "Alex"), p("3", "|slot_1")];
        let mut sent = false;
        let o = detect(
            "<Alex> hi",
            Context::Chat,
            Some(&online[1]),
            &online,
            &settings(Detection::UuidAndHeuristic),
            &mut sent,
        )
        .unwrap();
        assert_eq!((o.player.as_str(), o.name.as_deref()), ("2", Some("Alex")));
        assert!(sent);
        // once the server sends UUIDs, no guessing
        assert!(
            detect(
                "Steve joined",
                Context::System,
                None,
                &online,
                &settings(Detection::UuidAndHeuristic),
                &mut sent
            )
            .is_none()
        );
        // heuristics alone ignore the UUID
        let o = detect(
            "Steve: hi",
            Context::Chat,
            Some(&online[1]),
            &online,
            &settings(Detection::HeuristicOnly),
            &mut false,
        )
        .unwrap();
        assert_eq!(o.player, "1");
        // UUID known, name not in the text: head at the line start
        let o = detect(
            "hello",
            Context::Chat,
            Some(&online[0]),
            &online,
            &settings(Detection::UuidOnly),
            &mut false,
        )
        .unwrap();
        assert_eq!(o.name, None);
        assert!(
            detect(
                "x Steve",
                Context::Chat,
                None,
                &online,
                &settings(Detection::UuidOnly),
                &mut false
            )
            .is_none()
        );
        // fake tab entries and system messages
        assert!(
            detect(
                "|slot_1 hi",
                Context::System,
                None,
                &online,
                &settings(Detection::HeuristicOnly),
                &mut false
            )
            .is_none()
        );
        let mut s = settings(Detection::HeuristicOnly);
        s.system_messages = false;
        assert!(detect("Steve", Context::System, None, &online, &s, &mut false).is_none());
    }

    #[test]
    fn aliases() {
        let online = [p("1", "Steve")];
        let mut s = settings(Detection::HeuristicOnly);
        s.aliases = "stevie = steve, ghost=nobody, =x";
        let o = detect("<stevie> yo", Context::Chat, None, &online, &s, &mut false).unwrap();
        assert_eq!(
            (o.player.as_str(), o.name.as_deref()),
            ("1", Some("stevie"))
        );
    }

    #[test]
    fn tags_lines() {
        let o = Owner {
            player: "1".into(),
            name: Some("Bob".into()),
        };
        let t = tag(Some(&o), &["hello there", "and <Bob> said"], false, true);
        assert_eq!(t[0], None);
        assert_eq!(
            t[1],
            Some(Line {
                player: Some("1".into()),
                split: 5
            })
        );
        let t = tag(Some(&o), &["<Bob> a", "b"], true, true);
        assert_eq!(
            t[0],
            Some(Line {
                player: Some("1".into()),
                split: 0
            })
        );
        assert_eq!(
            t[1],
            Some(Line {
                player: None,
                split: 0
            })
        );
        assert_eq!(tag(None, &["x"], true, false), [None]);
    }
}
