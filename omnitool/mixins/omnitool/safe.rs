// Java's core/SafeMode.java as pure decisions: which server we are on, whether
// Auto mode counts it as foreign, whether Safe Mode blocks, and the manual
// per-server override. The settings (`safe_mode`, `safe_whitelist`,
// `safe_key`) live in core.rs. Added to the game as `game::omnitool_safe`;
// also a module of this crate, checked against tests/fixtures/core-edition.json.

use super::omnitool_setting::entries;

/// The `safe_mode` setting's values.
pub const MODES: &[&str] = &["OFF", "ON", "AUTO"];
pub const AUTO: usize = 2;

/// Where the player is, as far as the host knows.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)] // used by modules and menus still to come
pub enum Server {
    /// Singleplayer (Java: `isLocalServer`).
    Local,
    /// No server data (Java: `getCurrentServer() == null`).
    Nowhere,
    /// A multiplayer server: the address as typed, with an optional `:port`.
    Remote(String),
    /// The host can't tell (Horizon has no hook for it yet). The spec's
    /// fallback: counts as a foreign server.
    Unknown,
}

/// Java's `serverKey`: what a manual override is tied to.
pub fn server_key(server: &Server) -> String {
    match server {
        Server::Local => "@local".into(),
        Server::Nowhere => "@none".into(),
        Server::Remote(ip) => ip.to_lowercase(),
        Server::Unknown => "@unknown".into(),
    }
}

/// Java's `onForeignServer`: a multiplayer server not in the whitelist
/// (exactly, or with a `:port`).
pub fn foreign(server: &Server, whitelist: &str) -> bool {
    match server {
        Server::Local | Server::Nowhere => false,
        Server::Unknown => true,
        Server::Remote(ip) => {
            let ip = ip.to_lowercase();
            !entries(whitelist)
                .iter()
                .any(|a| ip == *a || ip.starts_with(&format!("{a}:")))
        }
    }
}

/// Java's `byMode`: OFF, ON, or AUTO = on a foreign server.
pub fn by_mode(mode: usize, server: &Server, whitelist: &str) -> bool {
    match mode {
        0 => false,
        1 => true,
        _ => foreign(server, whitelist),
    }
}

/// The session's manual switch: on/off for one server (Java's
/// `override`/`overrideServer`). Not saved.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Override(pub Option<(bool, String)>);

impl Override {
    /// Whether it applies on `server` (Java's `isOverridden`).
    pub fn active(&self, server: &Server) -> Option<bool> {
        match &self.0 {
            Some((on, key)) if *key == server_key(server) => Some(*on),
            _ => None,
        }
    }
}

/// Java's `isBlocking`.
pub fn blocking(
    curseforge: bool,
    mode: usize,
    server: &Server,
    whitelist: &str,
    manual: &Override,
) -> bool {
    if curseforge {
        return true;
    }
    manual
        .active(server)
        .unwrap_or_else(|| by_mode(mode, server, whitelist))
}

/// Java's `toggle`: flips Safe Mode for this server; returns the new state.
/// Back to what the mode says clears the override.
pub fn toggle(
    curseforge: bool,
    mode: usize,
    server: &Server,
    whitelist: &str,
    manual: &mut Override,
) -> bool {
    let next = !blocking(curseforge, mode, server, whitelist, manual);
    manual.0 = if next == by_mode(mode, server, whitelist) {
        None
    } else {
        Some((next, server_key(server)))
    };
    next
}

/// Java's `trustCurrentServer`: the whitelist with this server added (if it
/// isn't in it), and the address; None without a multiplayer server.
pub fn trust(server: &Server, whitelist: &str) -> Option<(String, String)> {
    let Server::Remote(ip) = server else {
        return None;
    };
    let ip = ip.to_lowercase();
    if entries(whitelist).contains(&ip) {
        return Some((whitelist.to_owned(), ip));
    }
    let cur = whitelist.trim();
    let list = if cur.is_empty() {
        ip.clone()
    } else {
        format!("{cur}, {ip}")
    };
    Some((list, ip))
}

/// Module.safeMode(): Safe Mode applies to this module (CurseForge ignores
/// `safe_ignore`).
pub fn module_safe_mode(curseforge: bool, blocking: bool, safe_ignore: bool) -> bool {
    blocking && (curseforge || !safe_ignore)
}

/// Module.isBlocked().
pub fn module_blocked(
    curseforge: bool,
    blocking: bool,
    safe_block: bool,
    safe_ignore: bool,
) -> bool {
    safe_block && module_safe_mode(curseforge, blocking, safe_ignore)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture() -> Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/core-edition.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn b(v: &Value) -> bool {
        v.as_bool().unwrap()
    }

    #[test]
    fn blocking_table() {
        for c in fixture()["safe_mode_is_blocking"].as_array().unwrap() {
            let mode = MODES.iter().position(|m| *m == c["mode"]).unwrap();
            let server = if b(&c["foreign_server"]) {
                Server::Remote("play.x.net".into())
            } else {
                Server::Local
            };
            let list = if b(&c["whitelisted"]) {
                "play.x.net"
            } else {
                "localhost"
            };
            let got = blocking(
                b(&c["curseforge"]),
                mode,
                &server,
                list,
                &Override::default(),
            );
            assert_eq!(got, b(&c["is_blocking"]), "{c}");
        }
    }

    #[test]
    fn module_table() {
        for c in fixture()["module_blocking"].as_array().unwrap() {
            let (cf, on) = (b(&c["curseforge"]), b(&c["safe_mode_blocking"]));
            assert_eq!(
                module_safe_mode(cf, on, b(&c["safe_ignore"])),
                b(&c["module.safeMode"]),
                "{c}"
            );
            let got = module_blocked(cf, on, b(&c["safe_block"]), b(&c["safe_ignore"]));
            assert_eq!(got, b(&c["module.isBlocked"]), "{c}");
        }
    }

    #[test]
    fn host_matching() {
        for c in fixture()["host_match"].as_array().unwrap() {
            let server = match (c["server_ip"].as_str(), c.get("singleplayer")) {
                (_, Some(_)) => Server::Local,
                (None, _) => Server::Nowhere,
                (Some(ip), _) => Server::Remote(ip.into()),
            };
            assert_eq!(
                foreign(&server, c["whitelist"].as_str().unwrap()),
                b(&c["foreign"]),
                "{c}"
            );
        }
        assert!(foreign(&Server::Unknown, "localhost"));
    }

    #[test]
    fn override_follows_the_server() {
        let a = Server::Remote("A.net".into());
        let mut o = Override::default();
        assert!(blocking(false, AUTO, &a, "", &o));
        assert!(!toggle(false, AUTO, &a, "", &mut o));
        assert_eq!(o.0, Some((false, "a.net".into())));
        assert!(!blocking(false, AUTO, &a, "", &o));
        // another server: the mode again
        assert!(blocking(
            false,
            AUTO,
            &Server::Remote("b.net".into()),
            "",
            &o
        ));
        // back to what the mode says: no override
        assert!(toggle(false, AUTO, &a, "", &mut o));
        assert_eq!(o, Override::default());
    }

    #[test]
    fn trusting_a_server() {
        let s = Server::Remote("Play.X.net:25565".into());
        assert_eq!(
            trust(&s, " localhost "),
            Some((
                "localhost, play.x.net:25565".into(),
                "play.x.net:25565".into()
            ))
        );
        assert_eq!(
            trust(&s, ""),
            Some(("play.x.net:25565".into(), "play.x.net:25565".into()))
        );
        assert_eq!(trust(&s, "play.x.net:25565").unwrap().0, "play.x.net:25565");
        assert_eq!(trust(&Server::Local, "x"), None);
    }
}
