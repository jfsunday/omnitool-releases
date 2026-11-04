// Hotbar presets (docs/spec/omnitool/inventory.md, section 5; Java's
// core/HotbarPresets, beta): named hotbar layouts (item ids only) in
// hotbars.json, restored by swapping matching items in with pickup clicks
// (`hotbar_swaps` in inventory_plan.rs, fixtures inventory-hotbar.json).
// `/ot hotbar save|load|delete <name>`. Added to the game as
// `game::omnitool_hotbar_presets`; also a module of this crate.

use super::omnitool_core::{self as core, Host};
use super::omnitool_inventory_plan::hotbar_swaps;
use super::omnitool_json::{Json, Obj};
use std::cell::RefCell;
use std::path::PathBuf;

const FILE: &str = "hotbars.json";

/// The presets in file order: name, 9 item ids ("" = empty slot).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Presets(pub Vec<(String, Vec<String>)>);

fn key(name: &str) -> String {
    name.trim().to_lowercase()
}

impl Presets {
    /// Reads the file's text; entries that aren't arrays of strings are
    /// skipped, unreadable text gives no presets.
    pub fn parse(text: &str) -> Presets {
        let Ok(Json::Obj(o)) = Json::parse(text) else {
            return Presets::default();
        };
        let mut out = Vec::new();
        for (name, v) in o.iter() {
            let Json::Arr(a) = v else { continue };
            let ids: Option<Vec<String>> = a
                .iter()
                .map(|e| match e {
                    Json::Str(s) => Some(s.clone()),
                    _ => None,
                })
                .collect();
            if let Some(ids) = ids {
                out.push((name.to_owned(), ids));
            }
        }
        Presets(out)
    }

    pub fn to_json(&self) -> Obj {
        let mut o = Obj::new();
        for (name, ids) in &self.0 {
            o.insert(name, Json::Arr(ids.iter().map(|s| Json::str(s)).collect()));
        }
        o
    }

    pub fn names(&self) -> Vec<String> {
        self.0.iter().map(|(n, _)| n.clone()).collect()
    }

    /// The saved name and ids; the name is found ignoring case and outer spaces.
    pub fn get(&self, name: &str) -> Option<&(String, Vec<String>)> {
        let k = key(name);
        self.0.iter().find(|(n, _)| key(n) == k)
    }

    /// Replaces a preset of that name (case-insensitive); the new one goes to
    /// the end under the name as typed.
    pub fn put(&mut self, name: &str, ids: Vec<String>) {
        let k = key(name);
        self.0.retain(|(n, _)| key(n) != k);
        self.0.push((name.trim().to_owned(), ids));
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let k = key(name);
        let before = self.0.len();
        self.0.retain(|(n, _)| key(n) != k);
        self.0.len() != before
    }
}

/// How many hotbar slots of `wanted` still hold the wrong item.
pub fn missing(wanted: &[String], hotbar_now: &[String]) -> usize {
    (0..9.min(wanted.len()))
        .filter(|&h| !wanted[h].is_empty() && hotbar_now.get(h) != Some(&wanted[h]))
        .count()
}

struct Store {
    dir: Option<PathBuf>,
    presets: Option<Presets>,
}

thread_local! {
    static STORE: RefCell<Store> = const { RefCell::new(Store { dir: None, presets: None }) };
}

/// Runs `f` on the presets, loading the file on first use.
fn with_presets<R>(host: &dyn Host, f: impl FnOnce(&mut Presets, Option<&PathBuf>) -> R) -> R {
    let dir = host.data_dir();
    STORE.with(|s| {
        let mut s = s.borrow_mut();
        if s.presets.is_none() || s.dir != dir {
            let file = dir.as_ref().map(|d| d.join(FILE));
            let text = file.and_then(|f| std::fs::read_to_string(f).ok());
            s.presets = Some(text.map_or_else(Presets::default, |t| Presets::parse(&t)));
            s.dir = dir.clone();
        }
        let file = dir.map(|d| d.join(FILE));
        f(s.presets.as_mut().unwrap(), file.as_ref())
    })
}

fn write(p: &Presets, file: Option<&PathBuf>) {
    if let Some(f) = file {
        super::omnitool_config::write(f, &p.to_json());
    }
}

/// All presets with their item ids (ClickGui list).
pub fn list(host: &dyn Host) -> Vec<(String, Vec<String>)> {
    with_presets(host, |p, _| p.0.clone())
}

/// The names for command suggestions.
pub fn names(host: &dyn Host) -> Vec<String> {
    with_presets(host, |p, _| p.names())
}

fn ids_of(
    slots: &[super::omnitool_auto_refill::Stack],
    range: std::ops::RangeInclusive<usize>,
) -> Vec<String> {
    range
        .map(|i| slots.get(i - 9).map_or_else(String::new, |s| s.id.clone()))
        .collect()
}

/// `save`: the hotbar's item ids under `name`; the answer text.
pub fn save(host: &mut dyn Host, name: &str) -> String {
    let Some(view) = host.refill_view() else {
        return "Join a world first.".to_owned();
    };
    if name.trim().is_empty() {
        return "Enter a name.".to_owned();
    }
    let ids = ids_of(&view.slots, 36..=44);
    with_presets(host, |p, file| {
        p.put(name, ids);
        write(p, file);
    });
    format!("Hotbar preset saved: {name}")
}

/// `restore`: swaps the wanted items into the hotbar; the answer text.
pub fn restore(host: &mut dyn Host, name: &str) -> String {
    let Some(view) = host.refill_view() else {
        return "Join a world first.".to_owned();
    };
    let Some((saved, wanted)) = with_presets(host, |p, _| p.get(name).cloned()) else {
        return format!("Unknown hotbar preset: {name}");
    };
    if !host.carried_empty() {
        return "Close other containers first.".to_owned();
    }
    let slots = ids_of(&view.slots, 9..=44);
    let mut done = 0;
    for (hotbar, source) in hotbar_swaps(&slots, &wanted) {
        // the target's state before the clicks
        let target_empty = view.slots.get(hotbar - 9).is_none_or(|s| s.is_empty());
        host.click_menu_slot(source);
        host.click_menu_slot(hotbar);
        if !target_empty {
            host.click_menu_slot(source);
        }
        done += 1;
    }
    // what the hotbar holds now, by the model of the swaps
    let after = apply(&slots, &wanted);
    let missing = missing(&wanted, &after[27..36]);
    if missing > 0 {
        format!("Hotbar preset {saved} loaded ({done} moved, {missing} items not found)")
    } else {
        format!("Hotbar preset {saved} loaded ({done} moved)")
    }
}

/// The 36 ids after the swaps for `wanted`.
fn apply(slots: &[String], wanted: &[String]) -> Vec<String> {
    let mut inv = slots.to_vec();
    for (hotbar, source) in hotbar_swaps(slots, wanted) {
        inv.swap(hotbar - 9, source - 9);
    }
    inv
}

/// `delete`: false for an unknown name.
pub fn delete(host: &dyn Host, name: &str) -> bool {
    with_presets(host, |p, file| {
        let gone = p.remove(name);
        if gone {
            write(p, file);
        }
        gone
    })
}

/// `/ot hotbar save|load|delete <name>` (beta only; the caller checks).
pub fn ot_hotbar(host: &mut dyn Host, args: &str) {
    let args = args.trim();
    let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
    let name = unquote(rest);
    match sub {
        "list" => {
            let names = names(host);
            if names.is_empty() {
                host.chat("§7 No hotbar presets saved");
            } else {
                core::ok(host, &format!("Hotbar presets: {}", names.join(", ")));
            }
        }
        "save" if !name.is_empty() => {
            let r = save(host, name);
            core::ok(host, &r);
        }
        "load" if !name.is_empty() => {
            if with_presets(host, |p, _| p.get(name).is_none()) {
                return core::fail(host, &format!("Unknown hotbar preset: {name}"));
            }
            let r = restore(host, name);
            core::ok(host, &r);
        }
        "delete" if !name.is_empty() => {
            if delete(host, name) {
                core::ok(host, &format!("Hotbar preset deleted: {name}"));
            } else {
                core::fail(host, &format!("Unknown hotbar preset: {name}"));
            }
        }
        _ => core::fail(host, "Usage: /ot hotbar list | save|load|delete <name>"),
    }
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    if s.len() > 1 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn file_format_and_names() {
        let p = Presets::parse(
            r#"{"Mining": ["minecraft:torch", ""], "bad": 3, "worse": [1], "b": []}"#,
        );
        assert_eq!(p.names(), ["Mining", "b"]);
        assert_eq!(p.get(" mining ").unwrap().0, "Mining");
        assert_eq!(
            p.to_json().len(),
            2,
            "skipped entries are dropped on the next write"
        );
        assert_eq!(Presets::parse("not json"), Presets::default());
    }

    #[test]
    fn put_replaces_ignoring_case_and_moves_to_end() {
        let mut p = Presets::default();
        p.put("a", ids(&["x"]));
        p.put("B", ids(&["y"]));
        p.put(" A ", ids(&["z"]));
        assert_eq!(p.names(), ["B", "A"]);
        assert!(p.remove("b"));
        assert!(!p.remove("b"));
    }

    #[test]
    fn missing_counts_wrong_slots() {
        let wanted = ids(&["a", "", "c"]);
        assert_eq!(missing(&wanted, &ids(&["a", "q", "x"])), 1);
        assert_eq!(missing(&wanted, &ids(&["a"])), 1);
    }

    #[test]
    fn list_command_shows_the_names() {
        use crate::omnitool_config::tests::temp_dir;
        use crate::omnitool_core::tests::Fake;
        let mut h = Fake {
            dir: Some(temp_dir()),
            ..Fake::default()
        };
        let say = |h: &mut Fake, args: &str| {
            h.lines.clear();
            ot_hotbar(h, args);
            h.lines.clone()
        };
        assert_eq!(say(&mut h, "list"), ["§7 No hotbar presets saved"]);
        with_presets(&h, |p, _| {
            p.put("Mining", ids(&["a"]));
            p.put("Build", ids(&["b"]));
        });
        assert_eq!(
            say(&mut h, "list"),
            ["§6[Omnitool] §fHotbar presets: Mining, Build"]
        );
        assert_eq!(
            say(&mut h, "bogus"),
            ["§c[Omnitool] Usage: /ot hotbar list | save|load|delete <name>"]
        );
    }

    #[test]
    fn apply_follows_the_swaps() {
        let mut slots = vec![String::new(); 36];
        slots[5] = "minecraft:torch".into();
        let wanted = ids(&["minecraft:torch"]);
        let after = apply(&slots, &wanted);
        assert_eq!(after[27], "minecraft:torch");
        assert_eq!(after[5], "");
    }
}
