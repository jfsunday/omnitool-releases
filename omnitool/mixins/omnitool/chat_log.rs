// Chat Log (docs/spec/omnitool/chat.md, section 3): every chat line of every
// server goes to `<omnitool folder>/chatlog/` (JSON Lines, rotating
// segments, size cap); on joining a server the last lines come back.
// Beta, on by default. What the host sees: received lines (glue.rs reads
// them each tick), commands that go to the server, `/ot chat ...`. Missing
// (needs a Horizon mixin): the plain chat lines you send and the
// action bar. Times are UTC (std has no time zone; Java: local offset). Added
// to the game as `game::omnitool_chat_log`; also a module of this crate.

use super::omnitool_chat::{self as chat, ChatLine, Store};
use super::omnitool_chat_tweaks as tweaks;
use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_json::Obj;
use super::omnitool_setting::{self as setting, Kind, SettingDef};
use std::cell::RefCell;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "chat_log",
    name: "Chat Log",
    category: Category::Misc,
    description: "Saves all chat messages of every server to config/omnitool/chatlog (JSON Lines) and restores the last messages when you join.",
    default_on: true,
    cheaty: false,
    beta: true,
    fair_variant: false,
    replaces: "",
    settings: &[
        SettingDef::new(
            "max_size",
            "Max log size (KB, MB, GB, TB)",
            Kind::Text("100 MB"),
        ),
        SettingDef::new(
            "restore",
            "Restore messages on join (0 = off)",
            Kind::Number {
                default: 200.0,
                min: 0.0,
                max: 2000.0,
                step: 10.0,
            },
        ),
        SettingDef::new(
            "log_commands",
            "Log your commands (passwords masked)",
            Kind::Bool(true),
        ),
        SettingDef::new(
            "log_action_bar",
            "Log action bar messages",
            Kind::Bool(false),
        ),
    ],
    command: None,
    key: setting::NONE,
    hold: false,
    hud: None,
    tick: Some(tick),
    on_key: None,
    hud_right: None,
    on_action: None,
};

const DEFAULT_CAP: u64 = 100 * chat::MB;

thread_local! {
    static STORE: RefCell<Option<Store>> = const { RefCell::new(None) };
    /// The server key of the connection restored for; None = not joined.
    static JOINED: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Java's `live()`.
pub fn live(host: &dyn Host) -> bool {
    core::active(host, &MODULE)
}

pub fn cap(host: &dyn Host) -> u64 {
    match core::value(host, &MODULE, "max_size") {
        setting::Value::Text(t) => chat::parse_cap(&t, DEFAULT_CAP),
        _ => DEFAULT_CAP,
    }
}

/// `serverKey()` (spec 3.4).
pub fn server_key(host: &dyn Host) -> String {
    let name = host.world().map(|w| w.world_name).unwrap_or_default();
    super::omnitool_death_coords::server_key(&host.server(), &name)
}

fn with_store<R>(host: &dyn Host, f: impl FnOnce(&mut Store) -> R) -> Option<R> {
    let dir = host.data_dir()?.join("chatlog");
    STORE.with(|s| {
        let mut s = s.borrow_mut();
        if s.as_ref().is_none_or(|st| st.dir() != dir) {
            *s = Some(Store::new(dir));
        }
        s.as_mut().map(f)
    })
}

/// `ChatLog.log` (spec 3.5): nothing unless the module is live.
pub fn log(
    host: &mut dyn Host,
    kind: &str,
    sender: Option<&str>,
    text: &str,
    filtered: bool,
    extra: Option<Obj>,
) {
    if !live(host) {
        return;
    }
    let now = super::omnitool_death_coords::timestamp(host.now_ms());
    let mut line = ChatLine::new(&now, Some(&server_key(host)), kind, sender, text, filtered);
    line.extra = extra;
    let cap = cap(host);
    let err = with_store(host, |s| s.append(&line, cap, &now[..10])).and_then(Result::err);
    if let Some(e) = err {
        super::omnitool_config::log(&e);
    }
}

/// A line you sent (`sendChat` / `sendCommand`, the command without `/`).
pub fn sent(host: &mut dyn Host, message: &str, command: bool) {
    if !live(host) || (command && !core::value(host, &MODULE, "log_commands").on()) {
        return;
    }
    let name = host.player_name();
    if command {
        log(
            host,
            "command",
            name.as_deref(),
            &chat::mask_command(message),
            false,
            None,
        );
    } else {
        log(host, "sent", name.as_deref(), message, false, None);
    }
}

/// An action bar message.
#[allow(dead_code)] // needs the action bar of a server message (no hook yet)
pub fn action_bar(host: &mut dyn Host, text: &str) {
    if live(host) && core::value(host, &MODULE, "log_action_bar").on() {
        log(host, "actionbar", None, &chat::plain(text), false, None);
    }
}

/// `dd.MM. HH:mm` of an ISO time; `?` when it isn't one.
pub fn short_time(iso: &str) -> String {
    let b = iso.as_bytes();
    let ok = iso.len() >= 16
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && iso.is_char_boundary(16);
    if ok {
        format!("{}.{}. {}", &iso[8..10], &iso[5..7], &iso[11..16])
    } else {
        "?".to_owned()
    }
}

fn shown(kind: Option<&str>) -> bool {
    kind.is_some_and(|k| !matches!(k, "sent" | "command" | "actionbar"))
}

/// Spec 3.6: once per join, the last lines of this server.
fn tick(host: &mut dyn Host) {
    let key = host.player().map(|_| server_key(host));
    let Some(key) = key else {
        JOINED.with(|j| *j.borrow_mut() = None);
        return;
    };
    if JOINED.with(|j| j.borrow().as_deref() == Some(key.as_str())) {
        return;
    }
    JOINED.with(|j| *j.borrow_mut() = Some(key.clone()));
    if !live(host) {
        return;
    }
    let restore = match core::value(host, &MODULE, "restore") {
        setting::Value::Num(n) => n as usize,
        _ => 0,
    };
    let n = restore.min(tweaks::history_limit(host, 100));
    if n == 0 {
        return;
    }
    let lines = with_store(host, |s| {
        s.tail(n, u64::MAX, |l| {
            l.server.as_deref() == Some(key.as_str()) && !l.filtered && shown(l.kind.as_deref())
        })
    })
    .unwrap_or_default();
    if lines.is_empty() {
        return;
    }
    let head = core::tr(host, "Last messages on this server:");
    tweaks::print(host, &format!("§8[history] {head} {}", lines.len()));
    for l in &lines {
        tweaks::print(host, &format!("§8[{}] §7{}", short_time(&l.time), l.text));
    }
    tweaks::print(host, "§8[history] ---");
}

/// `/ot chat search <text>` (spec 3.7).
pub fn search(host: &mut dyn Host, query: &str) {
    let found = with_store(host, |s| s.search(query, 20)).unwrap_or_default();
    if found.is_empty() {
        let t = core::tr(host, "No matches in the chat log");
        return tweaks::print(host, &format!("§6[Omnitool] {t}"));
    }
    let t = core::tr(host, "Latest matches:");
    tweaks::print(host, &format!("§6[Omnitool] {t} {}", found.len()));
    for l in found.iter().rev() {
        tweaks::print(
            host,
            &format!(
                "§8[{} {}] §7{}",
                short_time(&l.time),
                l.server.as_deref().unwrap_or("?"),
                l.text
            ),
        );
    }
}

/// `/ot chat log`: folder and size.
pub fn info(host: &dyn Host) -> Option<String> {
    let cap = cap(host);
    with_store(host, |s| {
        format!(
            "Chat log: {} ({} / {})",
            s.dir().display(),
            chat::format_size(s.total_size()),
            chat::format_size(cap)
        )
    })
}

/// The log file closes when the game ends or the module goes off.
#[allow(dead_code)] // no shutdown hook yet; the OS closes the file
pub fn close() {
    STORE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            st.close();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_config::tests::temp_dir;
    use crate::omnitool_core::tests::{Fake, restart};

    fn host() -> Fake {
        restart();
        STORE.with(|s| *s.borrow_mut() = None);
        JOINED.with(|j| *j.borrow_mut() = None);
        Fake {
            dir: Some(temp_dir()),
            now: 1_760_709_802_123,
            ..Fake::default()
        }
    }

    #[test]
    fn short_time_formats() {
        assert_eq!(short_time("2026-10-17T14:03:22.123+02:00"), "17.10. 14:03");
        assert_eq!(short_time("nope"), "?");
    }

    #[test]
    fn logs_masks_and_searches() {
        let mut h = host();
        log(&mut h, "chat", Some("Steve"), "hello world", false, None);
        sent(&mut h, "login hunter2", true);
        sent(&mut h, "hi all", false);
        search(&mut h, "HELLO");
        assert!(h.lines.iter().any(|l| l.contains("Latest matches: 1")));
        search(&mut h, "hunter2");
        assert!(h.lines.last().unwrap().contains("No matches"));
        search(&mut h, "***");
        assert!(h.lines.last().unwrap().contains("/login ***"));
        assert!(info(&h).unwrap().starts_with("Chat log: "));
    }

    #[test]
    fn restores_once_per_join() {
        let mut h = host();
        h.player = Some(core::Player {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            dimension: "overworld".into(),
        });
        log(&mut h, "chat", None, "old line", false, None);
        log(&mut h, "sent", None, "my own", false, None);
        log(&mut h, "chat", None, "secret", true, None);
        tick(&mut h);
        assert_eq!(h.lines.len(), 3, "{:?}", h.lines);
        assert!(h.lines[0].contains("Last messages on this server: 1"));
        assert!(h.lines[1].ends_with("§7old line"));
        tick(&mut h);
        assert_eq!(h.lines.len(), 3);
    }
}
