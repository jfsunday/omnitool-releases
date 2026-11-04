// ClickGui "phase A" (docs/spec/omnitool/clickgui.md): Omnitool's page in
// Options > Mods has only buttons (a label and a click), so the ClickGui's
// tabs become screens of buttons: Home -> category / favourites / mod
// replacements -> module -> its settings, plus the global settings. This file
// is the state machine: `buttons` lists the labels of a screen, `click` turns
// a click into the next screen and a change. No game types, so it is tested
// as a plain library. Added to the game as `game::omnitool_menu`; also a
// module of this crate.

use super::omnitool_core::{self as core, Category, GLOBALS, Host, ModuleInfo, State, key_name};
use super::omnitool_lang as lang;
use super::omnitool_registry::MODULES;
use super::omnitool_safe::Server;
use super::omnitool_setting::{self as setting, Kind, NONE, SettingDef, Value};
use std::cell::{Cell, RefCell};

/// Which module list a screen shows (the ClickGui's module tabs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Favourites,
    Category(Category),
    Replacements,
}

impl Tab {
    fn title(self) -> &'static str {
        match self {
            Tab::Favourites => "Favourites",
            Tab::Category(c) => c.title(),
            Tab::Replacements => "Mod replacements",
        }
    }

    fn has(self, s: &State, m: &ModuleInfo) -> bool {
        match self {
            Tab::Favourites => s.state(m).favorite,
            Tab::Category(c) => m.category == c,
            Tab::Replacements => !m.replaces.is_empty(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Home,
    List(Tab),
    /// A module's settings, by id.
    Module(&'static str),
    Globals,
}

/// What a setting button does to its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Bool: switch.
    Toggle,
    /// Enum: next (+1) or previous (-1) value.
    Cycle(i32),
    /// Number: one step up or down.
    Step(i32),
    /// Color: next preset.
    Preset,
    /// Action: the module's handler (`ModuleInfo::on_action`).
    Run,
    /// Key: wait for the next key press (`capture_key`).
    Capture,
    /// Text: only says how to change it.
    Hint,
}

/// Which value a setting button changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Index into the module's own `settings`.
    Own(&'static str, usize),
    /// One of the module's four built-in settings (`core::KEY`, ...).
    Hold(&'static str),
    SafeBlock(&'static str),
    SafeIgnore(&'static str),
    /// Index into `GLOBALS`.
    Global(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Nothing to do (an info line).
    #[allow(dead_code)]
    None,
    Go(Screen),
    Back,
    SafeMode,
    TrustServer,
    SaveConfig,
    ToggleModule(&'static str),
    ToggleFavourite(&'static str),
    Set(Target, Change),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    pub label: String,
    pub action: Action,
}

/// Where the player is: the current screen and the way back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nav {
    pub screen: Screen,
    back: Vec<Screen>,
}

impl Default for Nav {
    fn default() -> Nav {
        Nav {
            screen: Screen::Home,
            back: Vec::new(),
        }
    }
}

impl Nav {
    pub fn go(&mut self, to: Screen) {
        self.back.push(self.screen);
        self.screen = to;
    }

    pub fn back(&mut self) {
        self.screen = self.back.pop().unwrap_or(Screen::Home);
    }
}

thread_local! {
    static NAV: RefCell<Nav> = RefCell::new(Nav::default());
    /// The Key setting that waits for the next key press.
    /// Action settings clicked, run once the state is free again.
    static ACTIONS: RefCell<Vec<(&'static str, &'static str)>> = const { RefCell::new(Vec::new()) };
    static CAPTURE: Cell<Option<Target>> = const { Cell::new(None) };
}

/// Forgets where the player was (the page opens at Home again).
#[allow(dead_code)] // for the page's open hook
pub fn reset() {
    NAV.with(|n| *n.borrow_mut() = Nav::default());
    CAPTURE.set(None);
}

// ------------------------------------------------------------ text

/// German texts of this page that Java's de.json has no entry for.
const DE: &[(&str, &str)] = &[
    ("Back", "Zurück"),
    ("Black", "Schwarz"),
    ("Blue", "Blau"),
    ("Cyan", "Cyan"),
    ("Edit with: /ot set", "Ändern mit: /ot set"),
    ("Press a key", "Taste drücken"),
    ("Esc = cancel, Del = none", "Esc = abbrechen, Entf = keine"),
    ("Enabled", "Aktiv"),
    ("Gray", "Grau"),
    ("Green", "Grün"),
    ("Magenta", "Magenta"),
    ("Orange", "Orange"),
    ("Red", "Rot"),
    ("Save config", "Config speichern"),
    ("Settings...", "Einstellungen..."),
    ("White", "Weiß"),
    ("Yellow", "Gelb"),
    ("custom", "eigene"),
    ("none", "keine"),
];

/// Java's `t`: de.json first, then this page's own table.
pub fn t(german: bool, english: &str) -> String {
    if german {
        if let Some(de) = lang::german_text(english) {
            return de.to_owned();
        }
        if let Some((_, de)) = DE.iter().find(|(k, _)| *k == english) {
            return (*de).to_owned();
        }
    }
    english.to_owned()
}

const ON: &str = "ON";
const OFF: &str = "OFF";

fn on_off(german: bool, on: bool) -> String {
    t(german, if on { ON } else { OFF })
}

/// The color presets (ARGB without alpha), as `Color` buttons cycle them.
pub const PRESETS: &[(&str, u32)] = &[
    ("White", 0xFFFFFF),
    ("Gray", 0xAAAAAA),
    ("Black", 0x000000),
    ("Red", 0xFF5555),
    ("Orange", 0xFFAA00),
    ("Yellow", 0xFFFF55),
    ("Green", 0x55FF55),
    ("Cyan", 0x55FFFF),
    ("Blue", 0x5555FF),
    ("Magenta", 0xFF55FF),
];

fn preset_of(argb: i32) -> Option<usize> {
    PRESETS
        .iter()
        .position(|(_, rgb)| *rgb == argb as u32 & 0xFF_FFFF)
}

/// The next preset after `argb`'s (the first when it has none), alpha kept.
pub fn next_preset(argb: i32) -> i32 {
    let next = preset_of(argb).map_or(0, |i| (i + 1) % PRESETS.len());
    ((argb as u32 & 0xFF00_0000) | PRESETS[next].1) as i32
}

fn key_text(german: bool, code: i32) -> String {
    if code == NONE {
        t(german, "none")
    } else {
        key_name(code)
    }
}

/// A value as a button shows it, translated.
fn value_text(german: bool, def: &SettingDef, v: &Value) -> String {
    match (def.kind, v) {
        (Kind::Bool(_), Value::Bool(b)) => on_off(german, *b),
        (Kind::Enum { .. }, _) => t(german, &def.display(v)),
        (Kind::Color(_), Value::Color(c)) => {
            let name =
                preset_of(*c).map_or_else(|| t(german, "custom"), |i| t(german, PRESETS[i].0));
            format!("{name} #{:08X}", *c as u32)
        }
        (Kind::Key(_), Value::Key(k)) => key_text(german, *k),
        _ => def.display(v),
    }
}

// ------------------------------------------------------------ buttons

fn btn(label: String, action: Action) -> Button {
    Button { label, action }
}

fn back(german: bool) -> Button {
    btn(format!("< {}", t(german, "Back")), Action::Back)
}

/// A module's switch text: `Name: ON`, with `(blocked by Safe Mode)` and
/// the Beta / cheaty marks.
fn module_label(s: &State, host: &dyn Host, german: bool, m: &ModuleInfo) -> String {
    let st = s.state(m);
    let mut label = format!("{}: {}", t(german, m.name), on_off(german, st.enabled));
    if st.failure.is_some() {
        label += &format!(" [{}]", t(german, "error - disabled"));
    } else if st.enabled && s.blocked(host, m) {
        label += &format!(" ({})", t(german, "blocked by Safe Mode"));
    }
    if m.beta {
        label += &format!(" [{}]", t(german, "Beta"));
    }
    if m.cheaty && !st.safe_ignore {
        label += &format!(" [{}]", t(german, "blocked when Safe Mode is active"));
    }
    label
}

pub fn safe_mode_label(s: &State, host: &dyn Host, german: bool) -> String {
    let mode = if s.safe_override.active(&host.server()).is_some() {
        t(german, "manual")
    } else {
        t(
            german,
            &GLOBALS[core::G_SAFE_MODE].display(&s.globals[core::G_SAFE_MODE]),
        )
    };
    format!(
        "Safe Mode {} ({mode})",
        on_off(german, s.safe_blocking(host))
    )
}

fn setting_buttons(
    out: &mut Vec<Button>,
    german: bool,
    def: &SettingDef,
    v: &Value,
    target: Target,
) {
    let capturing = CAPTURE.get() == Some(target);
    let name = t(german, def.name);
    let shown = value_text(german, def, v);
    match def.kind {
        Kind::Bool(_) => out.push(btn(
            format!("{name}: {shown}"),
            Action::Set(target, Change::Toggle),
        )),
        Kind::Enum { .. } => out.push(btn(
            format!("{name}: {shown}"),
            Action::Set(target, Change::Cycle(1)),
        )),
        Kind::Number { .. } => {
            out.push(btn(
                format!("- {name}: {shown}"),
                Action::Set(target, Change::Step(-1)),
            ));
            out.push(btn(
                format!("+ {name}: {shown}"),
                Action::Set(target, Change::Step(1)),
            ));
        }
        Kind::Color(_) => out.push(btn(
            format!("{name}: {shown}"),
            Action::Set(target, Change::Preset),
        )),
        Kind::Key(_) if capturing => out.push(btn(
            format!(
                "{name}: > {} <  ({})",
                t(german, "Press a key"),
                t(german, "Esc = cancel, Del = none")
            ),
            Action::Set(target, Change::Capture),
        )),
        Kind::Key(_) => out.push(btn(
            format!("{name}: {shown}"),
            Action::Set(target, Change::Capture),
        )),
        Kind::Text(_) => out.push(btn(
            format!("{name}: {shown}  ({})", t(german, "Edit with: /ot set")),
            Action::Set(target, Change::Hint),
        )),
        Kind::Action => out.push(btn(name, Action::Set(target, Change::Run))),
    }
}

/// The buttons of `screen`, top to bottom.
pub fn buttons(s: &State, host: &dyn Host, screen: Screen) -> Vec<Button> {
    let german = s.german(host);
    let mut out = Vec::new();
    match screen {
        Screen::Home => {
            out.push(btn(safe_mode_label(s, host, german), Action::SafeMode));
            let tabs = std::iter::once(Tab::Favourites)
                .chain(Category::ALL.iter().map(|&c| Tab::Category(c)))
                .chain([Tab::Replacements]);
            for tab in tabs {
                let n = MODULES
                    .iter()
                    .filter(|m| s.loaded(m) && tab.has(s, m))
                    .count();
                out.push(btn(
                    format!("{} ({n})", t(german, tab.title())),
                    Action::Go(Screen::List(tab)),
                ));
            }
            out.push(btn(t(german, "Settings"), Action::Go(Screen::Globals)));
        }
        Screen::List(tab) => {
            out.push(back(german));
            for m in MODULES.iter().filter(|m| s.loaded(m) && tab.has(s, m)) {
                out.push(btn(
                    module_label(s, host, german, m),
                    Action::ToggleModule(m.id),
                ));
                out.push(btn(
                    format!("   {}", t(german, "Settings...")),
                    Action::Go(Screen::Module(m.id)),
                ));
            }
        }
        Screen::Module(id) => {
            out.push(back(german));
            let Some(m) = s.loaded_module(id) else {
                return out;
            };
            let st = s.state(m);
            out.push(btn(
                module_label(s, host, german, m),
                Action::ToggleModule(m.id),
            ));
            out.push(btn(
                format!(
                    "{}: {}",
                    t(german, "Favourite"),
                    on_off(german, st.favorite)
                ),
                Action::ToggleFavourite(m.id),
            ));
            for (i, def) in m.settings.iter().enumerate() {
                setting_buttons(&mut out, german, def, &st.values[i], Target::Own(m.id, i));
            }
            setting_buttons(
                &mut out,
                german,
                &core::KEY,
                &Value::Key(st.key),
                Target::Own(m.id, usize::MAX),
            );
            if st.key != NONE {
                setting_buttons(
                    &mut out,
                    german,
                    &core::HOLD,
                    &Value::Bool(st.hold),
                    Target::Hold(m.id),
                );
            }
            setting_buttons(
                &mut out,
                german,
                &core::SAFE_BLOCK,
                &Value::Bool(st.safe_block),
                Target::SafeBlock(m.id),
            );
            if m.fair_variant {
                setting_buttons(
                    &mut out,
                    german,
                    &core::SAFE_IGNORE,
                    &Value::Bool(st.safe_ignore),
                    Target::SafeIgnore(m.id),
                );
            }
        }
        Screen::Globals => {
            out.push(back(german));
            out.push(btn(safe_mode_label(s, host, german), Action::SafeMode));
            for (i, def) in GLOBALS.iter().enumerate() {
                setting_buttons(&mut out, german, def, &s.globals[i], Target::Global(i));
                if i == core::G_WHITELIST && matches!(host.server(), Server::Remote(_)) {
                    out.push(btn(
                        t(german, "Trust this server (whitelist)"),
                        Action::TrustServer,
                    ));
                }
            }
            out.push(btn(t(german, "Save config"), Action::SaveConfig));
        }
    }
    out
}

// ------------------------------------------------------------ clicks

/// Applies one action (screen moves are `Nav`'s, done by `click`).
fn apply(s: &mut State, host: &mut dyn Host, action: Action) {
    match action {
        Action::None | Action::Go(_) | Action::Back => {}
        Action::SafeMode => {
            s.toggle_safe_mode(host);
        }
        Action::TrustServer => {
            if let Some(ip) = s.trust_server(host) {
                host.chat(&format!("§a[Omnitool] §r{ip}"));
            }
            s.save();
        }
        Action::SaveConfig => s.save(),
        Action::ToggleModule(id) => {
            if let Some(m) = s.loaded_module(id) {
                let st = s.state_mut(m);
                st.enabled = !st.enabled;
                s.save();
            }
        }
        Action::ToggleFavourite(id) => {
            if let Some(m) = s.loaded_module(id) {
                let st = s.state_mut(m);
                st.favorite = !st.favorite;
                s.save();
            }
        }
        Action::Set(target, change) => set(s, host, target, change),
    }
}

fn set(s: &mut State, host: &mut dyn Host, target: Target, change: Change) {
    match target {
        Target::Hold(id) | Target::SafeBlock(id) | Target::SafeIgnore(id) => {
            if let Some(m) = s.loaded_module(id) {
                let st = s.state_mut(m);
                let flag = match target {
                    Target::Hold(_) => &mut st.hold,
                    Target::SafeBlock(_) => &mut st.safe_block,
                    _ => &mut st.safe_ignore,
                };
                *flag = !*flag;
                s.save();
            }
        }
        Target::Own(id, i) => {
            let Some(m) = s.loaded_module(id) else { return };
            let kind = if i == usize::MAX {
                core::KEY.kind
            } else {
                m.settings[i].kind
            };
            match (kind, change) {
                (Kind::Action, _) => {
                    // the handler may use the state itself
                    return ACTIONS.with_borrow_mut(|q| q.push((m.id, m.settings[i].id)));
                }
                (Kind::Key(_), _) => return CAPTURE.set(Some(target)),
                _ => {}
            }
            let def = &m.settings[i];
            let value = &mut s.state_mut(m).values[i];
            if edit(def, value, change) {
                s.save();
            } else {
                hint(s, host, m.id, def.id, def.display(&s.state(m).values[i]));
            }
        }
        Target::Global(i) => {
            let def = &GLOBALS[i];
            if matches!(def.kind, Kind::Key(_)) {
                return CAPTURE.set(Some(target));
            }
            if edit(def, &mut s.globals[i], change) {
                s.save();
            } else {
                let value = def.display(&s.globals[i]);
                let german = s.german(host);
                host.chat(&format!(
                    "§7[Omnitool] {}: {value} - /ot set",
                    t(german, def.name)
                ));
            }
        }
    }
}

/// Sets the Key setting `target` to `code` (`NONE` unbinds).
fn bind(s: &mut State, target: Target, code: i32) {
    match target {
        Target::Own(id, i) => {
            let Some(m) = s.loaded_module(id) else { return };
            if i == usize::MAX {
                s.state_mut(m).key = code;
            } else {
                let def = &m.settings[i];
                def.set(&mut s.state_mut(m).values[i], Value::Key(code));
            }
        }
        Target::Global(i) => GLOBALS[i].set(&mut s.globals[i], Value::Key(code)),
        _ => return,
    }
    s.save();
}

/// GLFW code of a vanilla key name (`key.keyboard.c`), the reverse of
/// `core::vanilla_key_name`.
pub fn key_code(name: &str) -> Option<i32> {
    (32..350).find(|&c| core::vanilla_key_name(c).as_deref() == Some(name))
}

/// `native_key` while a Key button waits: the key is bound (Esc cancels,
/// Delete unbinds). True when the event was taken, so nothing else sees it.
pub fn capture_key(host: &mut dyn Host, key: &str) -> bool {
    let Some(target) = CAPTURE.get() else {
        return false;
    };
    let code = match key {
        "key.keyboard.escape" => None,
        "key.keyboard.delete" => Some(NONE),
        other => match key_code(other) {
            Some(c) => Some(c),
            None => return true, // no name for it: keep waiting
        },
    };
    CAPTURE.set(None);
    if let Some(code) = code {
        core::with(host, |s, _| bind(s, target, code));
    }
    true
}

/// Chat line telling how to change a Key / Text setting.
fn hint(s: &State, host: &mut dyn Host, module: &str, setting: &str, value: String) {
    let german = s.german(host);
    let _ = value;
    host.chat(&format!(
        "§7[Omnitool] {}: /ot set {module} {setting} <value>",
        t(german, "Edit with: /ot set")
    ));
}

/// Changes `v`; false when this change is only a hint.
fn edit(def: &SettingDef, v: &mut Value, change: Change) -> bool {
    let new = match (def.kind, &*v, change) {
        (Kind::Bool(_), Value::Bool(b), Change::Toggle) => Value::Bool(!b),
        (Kind::Enum { values, .. }, Value::Enum(i), Change::Cycle(d)) => {
            Value::Enum(setting::cycle(*i, values.len(), d))
        }
        (Kind::Number { step, .. }, Value::Num(n), Change::Step(d)) => {
            Value::Num(n + step * f64::from(d))
        }
        (Kind::Color(_), Value::Color(c), Change::Preset) => Value::Color(next_preset(*c)),
        _ => return false,
    };
    def.set(v, new);
    true
}

/// Click on button `index` of the current screen. Out of range: nothing.
pub fn click(s: &mut State, host: &mut dyn Host, nav: &mut Nav, index: usize) {
    let Some(button) = buttons(s, host, nav.screen).into_iter().nth(index) else {
        return;
    };
    // any click ends a wait for a key; a Key button starts it again
    CAPTURE.set(None);
    match button.action {
        Action::Go(to) => nav.go(to),
        Action::Back => nav.back(),
        action => apply(s, host, action),
    }
}

/// Runs the Action settings `click` queued (their handlers use the state).
fn run_actions(host: &mut dyn Host) {
    for (module, setting) in ACTIONS.take() {
        core::run_action(host, module, setting);
    }
}

/// The labels of the page now.
pub fn labels(host: &dyn Host) -> Vec<String> {
    let screen = NAV.with(|n| n.borrow().screen);
    core::read(host, |s| {
        buttons(s, host, screen)
            .into_iter()
            .map(|b| b.label)
            .collect()
    })
}

/// A click on the page's button `index`.
pub fn page_click(host: &mut dyn Host, index: usize) {
    let mut nav = NAV.with(|n| n.borrow().clone());
    core::with(host, |s, host| click(s, host, &mut nav, index));
    run_actions(host);
    NAV.with(|n| *n.borrow_mut() = nav);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};

    fn all(host: &Fake, screen: Screen) -> Vec<Button> {
        core::read(host, |s| buttons(s, host, screen))
    }

    fn press(host: &mut Fake, nav: &mut Nav, label: &str) {
        let i = core::read(host, |s| buttons(s, host, nav.screen))
            .iter()
            .position(|b| b.label.starts_with(label))
            .unwrap_or_else(|| panic!("no button {label}"));
        core::with(host, |s, host| click(s, host, nav, i));
        run_actions(host);
    }

    fn texts(host: &Fake, screen: Screen) -> Vec<String> {
        all(host, screen).into_iter().map(|b| b.label).collect()
    }

    #[test]
    fn home_lists_tabs_and_navigation_goes_back() {
        restart();
        let mut h = Fake::default();
        let home = texts(&h, Screen::Home);
        assert_eq!(home[0], "Safe Mode OFF (Auto)");
        assert_eq!(home[1], "Favourites (0)");
        let render = MODULES
            .iter()
            .filter(|m| m.category == Category::Render)
            .count();
        assert_eq!(home[2], format!("Render ({render})"));
        let players = MODULES
            .iter()
            .filter(|m| m.category == Category::Player)
            .count();
        assert_eq!(home[4], format!("Player ({players})"));
        assert_eq!(home.last().unwrap(), "Settings");
        let mut nav = Nav::default();
        press(&mut h, &mut nav, "Player");
        assert_eq!(nav.screen, Screen::List(Tab::Category(Category::Player)));
        press(&mut h, &mut nav, "   Settings...");
        assert!(matches!(nav.screen, Screen::Module("zoom")));
        press(&mut h, &mut nav, "< Back");
        press(&mut h, &mut nav, "< Back");
        assert_eq!(nav.screen, Screen::Home);
        nav.back();
        assert_eq!(nav.screen, Screen::Home, "back at Home stays at Home");
        let mut nav = Nav::default();
        let before = nav.clone();
        core::with(&mut h, |s, host| click(s, host, &mut nav, 999));
        assert_eq!(nav, before);
    }

    #[test]
    fn module_list_toggles_and_marks() {
        restart();
        let mut h = Fake::default();
        let list = Screen::List(Tab::Category(Category::Hud));
        let before = texts(&h, list);
        let mut nav = Nav {
            screen: list,
            ..Nav::default()
        };
        let first = before[1].clone();
        let name = first.split(':').next().unwrap().to_owned();
        press(&mut h, &mut nav, &name);
        let after = texts(&h, list);
        assert_ne!(before[1], after[1]);
        assert!(after[1].starts_with(&name));
        press(&mut h, &mut nav, &name);
        assert_eq!(texts(&h, list)[1], before[1]);
    }

    #[test]
    fn detail_changes_settings() {
        restart();
        let mut h = Fake::default();
        let mut nav = Nav {
            screen: Screen::Module("module_list"),
            ..Nav::default()
        };
        let d = texts(&h, nav.screen);
        assert!(d.contains(&"Rainbow: OFF".to_owned()), "{d:?}");
        assert!(
            d.iter().any(|l| l.starts_with("Text color: White"))
                || d.iter().any(|l| l.starts_with("Text color:"))
        );
        press(&mut h, &mut nav, "Rainbow");
        assert!(texts(&h, nav.screen).contains(&"Rainbow: ON".to_owned()));
        // color presets cycle
        let before = texts(&h, nav.screen)
            .into_iter()
            .find(|l| l.starts_with("Text color"))
            .unwrap();
        press(&mut h, &mut nav, "Text color");
        let after = texts(&h, nav.screen)
            .into_iter()
            .find(|l| l.starts_with("Text color"))
            .unwrap();
        assert_ne!(before, after);
        assert!(before.contains("custom #FF60D0FF"), "{before}");
        assert!(after.contains("White #FFFFFFFF"), "{after}");
        // a Key button waits for the next key (see the capture tests)
        press(&mut h, &mut nav, "Shortcut key");
        assert!(
            texts(&h, nav.screen)
                .iter()
                .any(|l| l.contains("> Press a key <"))
        );
        press(&mut h, &mut nav, "Rainbow");
        assert!(!capturing(), "any other click cancels");
        press(&mut h, &mut nav, "Rainbow");
        // hold only appears with a key
        assert!(
            !texts(&h, nav.screen)
                .iter()
                .any(|l| l.starts_with("Only on while"))
        );
        // favourite
        press(&mut h, &mut nav, "Favourite");
        assert!(texts(&h, nav.screen).contains(&"Favourite: ON".to_owned()));
        assert_eq!(texts(&h, Screen::Home)[1], "Favourites (1)");
    }

    #[test]
    fn numbers_step_in_spec_increments_and_clamp() {
        restart();
        let mut h = Fake::default();
        let id = MODULES
            .iter()
            .find(|m| {
                m.settings
                    .iter()
                    .any(|d| matches!(d.kind, Kind::Number { .. }))
            })
            .expect("a module with a number")
            .id;
        let m = core::module(id).unwrap();
        let (i, def) = m
            .settings
            .iter()
            .enumerate()
            .find(|(_, d)| matches!(d.kind, Kind::Number { .. }))
            .unwrap();
        let Kind::Number {
            default,
            min,
            max,
            step,
        } = def.kind
        else {
            unreachable!()
        };
        let mut nav = Nav {
            screen: Screen::Module(m.id),
            ..Nav::default()
        };
        let value = |h: &Fake| core::read(h, |s| s.state(m).values[i].num());
        assert_eq!(value(&h), default);
        press(&mut h, &mut nav, &format!("+ {}", def.name));
        assert_eq!(value(&h), (default + step).min(max));
        press(&mut h, &mut nav, &format!("- {}", def.name));
        press(&mut h, &mut nav, &format!("- {}", def.name));
        assert_eq!(value(&h), (default - step).max(min));
        for _ in 0..10_000 {
            if value(&h) <= min {
                break;
            }
            press(&mut h, &mut nav, &format!("- {}", def.name));
        }
        assert_eq!(value(&h), min);
        press(&mut h, &mut nav, &format!("- {}", def.name));
        assert_eq!(value(&h), min, "stays in range");
    }

    #[test]
    fn globals_safe_mode_and_language() {
        restart();
        let mut h = Fake::default();
        let mut nav = Nav {
            screen: Screen::Globals,
            ..Nav::default()
        };
        assert!(texts(&h, nav.screen).contains(&"Safe Mode: Auto".to_owned()));
        press(&mut h, &mut nav, "Safe Mode: Auto");
        assert!(texts(&h, nav.screen).contains(&"Safe Mode: Off".to_owned()));
        press(&mut h, &mut nav, "Safe Mode: Off");
        assert_eq!(texts(&h, Screen::Home)[0], "Safe Mode ON (On)");
        // the badge flips for this session
        press(&mut h, &mut nav, "Safe Mode ON");
        assert_eq!(texts(&h, Screen::Home)[0], "Safe Mode OFF (manual)");
        // no server to trust in singleplayer
        assert!(!texts(&h, nav.screen).iter().any(|l| l.starts_with("Trust")));
        press(&mut h, &mut nav, "Language: Auto");
        assert!(texts(&h, nav.screen).contains(&"Language: English".to_owned()));
        press(&mut h, &mut nav, "Language: English");
        let g = texts(&h, nav.screen);
        assert!(g.contains(&"Sprache: Deutsch".to_owned()), "{g:?}");
        assert!(g.contains(&"Config speichern".to_owned()));
        assert_eq!(g[0], "< Zurück");
    }

    #[test]
    fn german_labels_and_safe_mode_lock() {
        restart();
        let mut h = Fake::default();
        core::with(&mut h, |s, _| {
            s.globals[core::G_LANGUAGE] = Value::Enum(2);
            s.globals[core::G_SAFE_MODE] = Value::Enum(1);
        });
        let home = texts(&h, Screen::Home);
        assert_eq!(home[0], "Safe Mode AN (An)");
        assert!(home.contains(&"Favoriten (0)".to_owned()));
        let replacing = MODULES.iter().filter(|m| !m.replaces.is_empty()).count();
        assert!(home.contains(&format!("Mod-Ersatz ({replacing})")));
        assert!(home.contains(&"Einstellungen".to_owned()));
        let cheaty = MODULES.iter().find(|m| m.cheaty && !m.beta).map(|m| m.id);
        if let Some(id) = cheaty {
            let m = core::module(id).unwrap();
            core::with(&mut h, |s, _| s.state_mut(m).enabled = true);
            let l = texts(&h, Screen::List(Tab::Category(m.category)));
            assert!(
                l.iter().any(|l| l.contains("von Safe Mode blockiert")),
                "{l:?}"
            );
        }
    }

    #[test]
    fn untranslated_text_stays_english_and_presets_keep_alpha() {
        assert_eq!(t(true, "no such text"), "no such text");
        assert_eq!(t(false, "Back"), "Back");
        assert_eq!(t(true, "Back"), "Zurück");
        assert_eq!(next_preset(0x80FFFFFFu32 as i32) as u32, 0x80AAAAAA);
        assert_eq!(next_preset(0xFF123456u32 as i32) as u32, 0xFFFFFFFF);
        let last = PRESETS[PRESETS.len() - 1].1;
        assert_eq!(
            next_preset(last as i32 | 0xFF00_0000u32 as i32) as u32,
            0xFFFFFFFF
        );
    }

    fn capturing() -> bool {
        CAPTURE.get().is_some()
    }

    fn key_of(h: &Fake, id: &str) -> i32 {
        core::read(h, |s| s.state(core::module(id).unwrap()).key)
    }

    #[test]
    fn key_button_binds_next_key_esc_cancels_del_unbinds() {
        restart();
        let mut h = Fake::default();
        let mut nav = Nav {
            screen: Screen::Module("module_list"),
            ..Nav::default()
        };
        press(&mut h, &mut nav, "Shortcut key");
        assert!(capturing());
        // Esc cancels
        assert!(capture_key(&mut h, "key.keyboard.escape"));
        assert!(!capturing());
        assert_eq!(key_of(&h, "module_list"), NONE);
        // a key binds; the event is taken, Hold appears
        press(&mut h, &mut nav, "Shortcut key");
        assert!(capture_key(&mut h, "key.keyboard.k"));
        assert_eq!(key_of(&h, "module_list"), 75);
        assert!(texts(&h, nav.screen).contains(&"Shortcut key (optional): K".to_owned()));
        assert!(
            texts(&h, nav.screen)
                .iter()
                .any(|l| l.starts_with("Only on while"))
        );
        // not waiting: events pass
        assert!(!capture_key(&mut h, "key.keyboard.j"));
        // Delete unbinds
        press(&mut h, &mut nav, "Shortcut key");
        assert!(capture_key(&mut h, "key.keyboard.delete"));
        assert_eq!(key_of(&h, "module_list"), NONE);
        // a press through the key dispatcher is swallowed, no module toggles
        press(&mut h, &mut nav, "Shortcut key");
        core::key_event(&mut h, "key.keyboard.l", true, false);
        assert_eq!(key_of(&h, "module_list"), 76);
        assert!(!capturing());
    }

    #[test]
    fn global_key_binds() {
        restart();
        let mut h = Fake::default();
        let mut nav = Nav {
            screen: Screen::Globals,
            ..Nav::default()
        };
        press(&mut h, &mut nav, "Open Omnitool menu");
        assert!(capturing());
        assert!(capture_key(&mut h, "key.keyboard.f9"));
        let code = core::read(&h, |s| s.globals[core::G_GUI_KEY].clone());
        assert_eq!(code, Value::Key(298));
    }

    #[test]
    fn every_action_setting_has_a_handler_and_runs() {
        restart();
        let mut h = Fake::default();
        let mut n = 0;
        for m in MODULES.iter() {
            for d in m.settings.iter().filter(|d| matches!(d.kind, Kind::Action)) {
                n += 1;
                assert!(m.on_action.is_some(), "{}.{} has no handler", m.id, d.id);
                let mut nav = Nav {
                    screen: Screen::Module(m.id),
                    ..Nav::default()
                };
                let label = t(false, d.name);
                press(&mut h, &mut nav, &label);
                assert!(
                    texts(&h, nav.screen).contains(&label),
                    "{} button stays a plain button",
                    d.id
                );
            }
        }
        assert!(n > 0);
    }

    #[test]
    fn reset_stats_button_clears_the_session() {
        restart();
        let mut h = Fake::default();
        let mut nav = Nav {
            screen: Screen::Module("session_stats"),
            ..Nav::default()
        };
        crate::omnitool_session_stats::test_add_deaths(2);
        assert_eq!(crate::omnitool_session_stats::test_deaths(), 2);
        press(&mut h, &mut nav, "Reset stats");
        assert_eq!(crate::omnitool_session_stats::test_deaths(), 0);
    }
}
