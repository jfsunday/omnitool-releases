// Chat Tweaks (docs/spec/omnitool/chat.md, section 2): filters, join/leave
// hiding, `(x5)` stacking, timestamps, longer history; every line also goes
// to the Chat Log. Beta, on by default. `handle` decides everything as Java
// does and says what the line should become. The host can only watch the
// lines Horizon received, not change them, so the log is written but a
// filtered or stacked line still shows (that needs a mixin on Horizon's
// chat component: `ui.chat` `add` in horizon-client-render/src/chat.rs).
// Added to the game as `game::omnitool_chat_tweaks`; also a module of this
// crate.

use super::omnitool_chat::{self as chat, Filter, SpamStacker};
use super::omnitool_chat_log as log;
use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{self as setting, Kind, SettingDef, Value};
use std::cell::RefCell;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "chat_tweaks",
    name: "Chat Tweaks",
    category: Category::Misc,
    description: "Chat filters (text or regex, per server), hide join/leave, stack repeated messages, timestamps and a longer chat history.",
    default_on: true,
    cheaty: false,
    beta: true,
    fair_variant: false,
    replaces: "",
    settings: &[
        SettingDef::new(
            "filters",
            "Hide messages containing (separate with ;  re: = regex, @server = only there)",
            Kind::Text(""),
        ),
        SettingDef::new(
            "hide_join_leave",
            "Hide join/leave messages",
            Kind::Bool(false),
        ),
        SettingDef::new(
            "stack_spam",
            "Stack repeated messages (x5)",
            Kind::Bool(true),
        ),
        SettingDef::new("timestamps", "Timestamps [HH:mm]", Kind::Bool(false)),
        SettingDef::new(
            "timestamp_seconds",
            "Timestamps with seconds",
            Kind::Bool(false),
        ),
        SettingDef::new(
            "history",
            "Chat history length (messages)",
            Kind::Number {
                default: 1000.0,
                min: 100.0,
                max: 100_000.0,
                step: 100.0,
            },
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

#[derive(Default)]
struct State {
    parsed_from: Option<String>,
    filter: Filter,
    stacker: SpamStacker,
    last_shown: Option<String>,
    /// Text added before / after the raw line by the last `handle`.
    parts: (String, String),
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Who delivered the line (Java's `contextKind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind3 {
    /// Client or mod message.
    #[allow(dead_code)]
    Client = 0,
    /// Player chat.
    Chat = 1,
    /// Server / system message.
    System = 2,
}

/// What `handle` decided for a line.
#[derive(Debug, PartialEq)]
pub struct Outcome {
    /// Hide the line.
    pub filtered: bool,
    /// The text to show (with `(xN)` and timestamp); None when hidden.
    pub shown: Option<String>,
    /// The previous stacked line should be removed first.
    pub remove_previous: bool,
}

pub fn live(host: &dyn Host) -> bool {
    core::active(host, &MODULE)
}

fn setting_text(host: &dyn Host, id: &str) -> String {
    match core::value(host, &MODULE, id) {
        Value::Text(t) => t,
        _ => String::new(),
    }
}

/// `historyLimit`: Vanilla's 100 or the `history` setting.
pub fn history_limit(host: &dyn Host, vanilla: usize) -> usize {
    if !live(host) {
        return vanilla;
    }
    match core::value(host, &MODULE, "history") {
        Value::Num(n) => vanilla.max(n as usize),
        _ => vanilla,
    }
}

/// Spec 2.4: the log type of a line.
pub fn line_type(key: Option<&str>, text: &str, kind: Kind3) -> &'static str {
    if key.is_some_and(|k| k.starts_with("death.")) {
        return "death";
    }
    let jl_text = if kind == Kind3::Chat {
        None
    } else {
        Some(text)
    };
    if chat::is_join_leave(key, jl_text) {
        let left = match key {
            Some(k) => k.contains("left"),
            None => text.ends_with("left the game"),
        };
        return if left { "leave" } else { "join" };
    }
    match kind {
        Kind3::Chat => "chat",
        Kind3::System => "system",
        Kind3::Client => "client",
    }
}

fn clock(ms: u64, seconds: bool) -> String {
    let s = ms / 1000;
    let (h, m, sec) = (s / 3600 % 24, s / 60 % 60, s % 60);
    if seconds {
        format!("[{h:02}:{m:02}:{sec:02}] ")
    } else {
        format!("[{h:02}:{m:02}] ")
    }
}

/// Spec 2.3 `handle`: `raw` is the line with `§` codes, `key` the
/// translation key when known, `sender` the player name of a chat line.
pub fn handle(
    host: &mut dyn Host,
    raw: &str,
    key: Option<&str>,
    kind: Kind3,
    sender: Option<&str>,
) -> Outcome {
    let text = chat::plain(raw);
    let ty = line_type(key, &text, kind);
    let server = log::server_key(host);
    let live = live(host);
    let mut filtered = false;
    if live {
        let filters = setting_text(host, "filters");
        let hide =
            core::value(host, &MODULE, "hide_join_leave").on() && matches!(ty, "join" | "leave");
        filtered = STATE.with(|s| {
            let mut s = s.borrow_mut();
            if s.parsed_from.as_deref() != Some(filters.as_str()) {
                s.filter = Filter::parse(&filters);
                s.parsed_from = Some(filters);
            }
            hide || s.filter.find(&text, Some(&server)).is_some()
        });
    }
    let sender = if kind == Kind3::Chat { sender } else { None };
    log::log(host, ty, sender, &text, filtered, None);
    if filtered {
        return Outcome {
            filtered,
            shown: None,
            remove_previous: false,
        };
    }
    if !live {
        return Outcome {
            filtered,
            shown: Some(raw.to_owned()),
            remove_previous: false,
        };
    }
    let mut result = raw.to_owned();
    let (mut pre, mut post) = (String::new(), String::new());
    let mut remove_previous = false;
    if core::value(host, &MODULE, "stack_spam").on() {
        let n = STATE.with(|s| s.borrow_mut().stacker.offer(Some(&text)));
        if n > 1 {
            remove_previous = STATE.with(|s| s.borrow().last_shown.is_some());
            post = format!("§7{}", chat::suffix(n));
            result = format!("{raw}{post}");
        }
    }
    if core::value(host, &MODULE, "timestamps").on() {
        let secs = core::value(host, &MODULE, "timestamp_seconds").on();
        pre = format!("§8{}§r", clock(host.now_ms(), secs));
        result = format!("{pre}{result}");
    }
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.last_shown = Some(result.clone());
        s.parts = (pre, post);
    });
    Outcome {
        filtered,
        shown: Some(result),
        remove_previous,
    }
}

/// What the chat injection does with a line (`intercept`).
#[derive(Debug, PartialEq)]
pub enum Verdict {
    /// Show the line unchanged.
    Keep,
    /// Do not add it to the chat.
    Hide,
    /// Show it with this text before and after (timestamp, `(xN)`).
    Decorate(String, String),
}

/// The sender of a plain `<Name> text` line and the line's kind.
fn classify(plain: &str) -> (Option<&str>, Kind3) {
    let sender = plain
        .strip_prefix('<')
        .and_then(|r| r.split_once("> "))
        .map(|(n, _)| n)
        .filter(|n| !n.is_empty() && !n.contains(' '));
    (
        sender,
        if sender.is_some() {
            Kind3::Chat
        } else {
            Kind3::System
        },
    )
}

/// Called by the chat injection for a line before Horizon adds it: runs
/// `handle` (log, filters, stacking, timestamps) and says what to show.
pub fn intercept(host: &mut dyn Host, plain: &str) -> Verdict {
    let (sender, kind) = classify(plain);
    let mut out = None;
    core::guard(host, &MODULE, |h| {
        out = Some(handle(h, plain, None, kind, sender));
    });
    match out {
        Some(o) if o.filtered => Verdict::Hide,
        Some(_) => {
            let (pre, post) = STATE.with(|s| s.borrow().parts.clone());
            if pre.is_empty() && post.is_empty() {
                Verdict::Keep
            } else {
                Verdict::Decorate(pre, post)
            }
        }
        None => Verdict::Keep,
    }
}

/// A line Horizon received (glue.rs reads them each tick). Its plain text
/// has no sender context: `<Name> text` is player chat, the rest a system
/// message.
pub fn on_line(host: &mut dyn Host, line: &str) {
    let plain = chat::plain(line);
    let (sender, kind) = classify(&plain);
    core::guard(host, &MODULE, |h| {
        handle(h, line, None, kind, sender);
    });
}

/// `ChatTweaks.print`: a line of the mod's own, past the filters.
pub fn print(host: &mut dyn Host, line: &str) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.stacker.reset();
        s.last_shown = None;
    });
    host.chat(line);
}

// ------------------------------------------------------------ /ot chat

fn filter_rules(host: &dyn Host) -> Vec<String> {
    chat::split(&setting_text(host, "filters"))
}

fn save_rules(host: &mut dyn Host, rules: &[String]) {
    let text = chat::join(rules);
    core::with(host, |s, _| {
        let _ = s.set_setting(&MODULE, "filters", &text);
        s.save();
    });
}

/// `/ot chat search|log|filter ...` (commands.md 4.3 - 4.5).
pub fn ot_chat(host: &mut dyn Host, args: &str) {
    let args = args.trim_start();
    let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
    match sub {
        "search" if !rest.trim().is_empty() => {
            if !log::live(host) {
                return core::fail(host, "Chat Log is off");
            }
            core::ok(host, "Searching the chat log...");
            log::search(host, rest);
        }
        "log" => match log::info(host) {
            Some(i) => core::ok(host, &i),
            None => core::fail(host, "Chat log: no folder"),
        },
        "filter" => filter(host, rest.trim_start()),
        _ => core::fail(
            host,
            "Usage: /ot chat search <text> | log | filter list|add|remove",
        ),
    }
}

fn filter(host: &mut dyn Host, args: &str) {
    let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
    let rest = rest.trim();
    let mut rules = filter_rules(host);
    match sub {
        "list" => {
            if rules.is_empty() {
                return core::ok(
                    host,
                    "No chat filters. Add one with /ot chat filter add <text | re:regex | @server ...>",
                );
            }
            for (i, r) in rules.iter().enumerate() {
                host.chat(&format!("§7 {}. {r}", i + 1));
            }
            for e in Filter::parse(&setting_text(host, "filters")).errors {
                host.chat(&format!("§c {e}"));
            }
        }
        "add" => match chat::rule(rest) {
            Ok(Some(_)) => {
                rules.push(rest.to_owned());
                save_rules(host, &rules);
                core::ok(host, &format!("Filter added: {rest}"));
            }
            Ok(None) => core::fail(host, "Empty rule"),
            Err(e) => core::fail(
                host,
                &format!("Invalid regex: {}", e.split(" near").next().unwrap_or(&e)),
            ),
        },
        "remove" => {
            let at = rules.iter().position(|r| r == rest).or_else(|| {
                let digits = !rest.is_empty() && rest.bytes().all(|c| c.is_ascii_digit());
                let n: usize = rest.parse().ok().filter(|_| digits)?;
                (1..=rules.len()).contains(&n).then(|| n - 1)
            });
            match at {
                Some(i) => {
                    rules.remove(i);
                    save_rules(host, &rules);
                    core::ok(host, "Filter removed");
                }
                None => core::fail(host, &format!("No such filter: {rest}")),
            }
        }
        _ => core::fail(
            host,
            "Usage: /ot chat filter list|add <rule>|remove <rule|nr>",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_config::tests::temp_dir;
    use crate::omnitool_core::tests::{Fake, restart};

    fn host() -> Fake {
        restart();
        STATE.with(|s| *s.borrow_mut() = State::default());
        Fake {
            dir: Some(temp_dir()),
            now: 3_723_000,
            ..Fake::default()
        }
    }

    fn set(h: &mut Fake, id: &str, v: &str) {
        core::with(h, |s, _| {
            s.set_setting(&MODULE, id, v).unwrap();
        });
    }

    #[test]
    fn types() {
        assert_eq!(
            line_type(Some("death.attack.x"), "x", Kind3::System),
            "death"
        );
        assert_eq!(
            line_type(Some("multiplayer.player.left"), "x", Kind3::System),
            "leave"
        );
        assert_eq!(
            line_type(None, "Bob joined the game", Kind3::System),
            "join"
        );
        assert_eq!(
            line_type(None, "<Bob> I left the game", Kind3::Chat),
            "chat"
        );
        assert_eq!(line_type(None, "hi", Kind3::Client), "client");
    }

    #[test]
    fn filters_stacks_and_stamps() {
        let mut h = host();
        set(&mut h, "filters", "buy gold; re:^\\[vip\\]");
        let o = handle(&mut h, "Cheap §aBUY GOLD", None, Kind3::System, None);
        assert!(o.filtered && o.shown.is_none());
        // a hidden line doesn't break a series
        assert_eq!(
            handle(&mut h, "hi", None, Kind3::System, None)
                .shown
                .as_deref(),
            Some("hi")
        );
        handle(&mut h, "buy gold", None, Kind3::System, None);
        let o = handle(&mut h, "hi", None, Kind3::System, None);
        assert_eq!(o.shown.as_deref(), Some("hi§7 (x2)"));
        assert!(o.remove_previous);
        set(&mut h, "timestamps", "true");
        set(&mut h, "timestamp_seconds", "true");
        let o = handle(&mut h, "other", None, Kind3::System, None);
        assert_eq!(o.shown.as_deref(), Some("§8[01:02:03] §rother"));
        set(&mut h, "hide_join_leave", "true");
        assert!(handle(&mut h, "Bob left the game", None, Kind3::System, None).filtered);
    }

    #[test]
    fn history_and_logging() {
        let mut h = host();
        assert_eq!(history_limit(&h, 100), 1000);
        set(&mut h, "history", "50");
        assert_eq!(history_limit(&h, 100), 100);
        on_line(&mut h, "<Steve> hello");
        log::search(&mut h, "hello");
        assert!(h.lines.last().unwrap().ends_with("<Steve> hello"));
    }

    #[test]
    fn filter_commands() {
        let mut h = host();
        ot_chat(&mut h, "filter add re:(bad");
        assert!(
            h.lines
                .last()
                .unwrap()
                .contains("Invalid regex: Unclosed group")
        );
        ot_chat(&mut h, "filter add a;b");
        ot_chat(&mut h, "filter add spam");
        assert_eq!(filter_rules(&h), ["a;b", "spam"]);
        ot_chat(&mut h, "filter remove 1");
        ot_chat(&mut h, "filter remove nothing");
        assert!(h.lines.last().unwrap().contains("No such filter: nothing"));
        ot_chat(&mut h, "filter list");
        assert!(h.lines.last().unwrap().ends_with("1. spam"));
        ot_chat(&mut h, "filter add @server");
        assert!(h.lines.last().unwrap().contains("Empty rule"));
        ot_chat(&mut h, "search spam");
    }

    #[test]
    fn intercept_verdicts() {
        let mut h = host();
        assert_eq!(intercept(&mut h, "hello"), Verdict::Keep);
        assert_eq!(
            intercept(&mut h, "hello"),
            Verdict::Decorate(String::new(), "§7 (x2)".into())
        );
        assert_eq!(intercept(&mut h, "Bob left the game"), Verdict::Keep);
    }
}
