// Omnitool's core (Java's core/Module, Modules, Config, Profiles, SafeMode,
// Lang, Beta, ShareCode export/import, OmniCommand): the module registry's
// types, every module's state and settings, config.json in Java's format,
// profiles, Safe Mode, the language, the `Host` the hooks hand in, and
// `/omnitool` (`/ot`). No game types: glue.rs implements `Host` for the game,
// the tests implement it in memory. Added to the game as
// `game::omnitool_core`; also a module of this crate.

use super::omnitool_conditions::{self as conditions, World};
use super::omnitool_config as config;
use super::omnitool_hud::{
    ELEMENTS as HUD_ELEMENTS, Effect as HudEffect, HudBox, Inventory as HudInventory,
    Worn as HudWorn,
};
use super::omnitool_json::{Json, Obj};
use super::omnitool_lang::{self as lang, LANGUAGES};
use super::omnitool_overlay_entries::OverlayInfo;
use super::omnitool_recorder::{Recorder, Sample};
use super::omnitool_registry::MODULES;
use super::omnitool_safe::{self as safe, AUTO, MODES, Override, Server};
use super::omnitool_setting::{Kind, NONE, SettingDef, Value};
use super::omnitool_share as share;
use super::omnitool_triggers::Triggers;
use super::omnitool_workflows::{self as workflows, Binding, Env, Input, Runners, by_name};
use std::cell::RefCell;
use std::path::PathBuf;

/// The native mod's id (its data folder), name and version.
pub const ID: &str = "omnitool";
pub const NAME: &str = "Omnitool";
pub const VERSION: &str = "0.1.0";
pub const DESCRIPTION: &str =
    "Omnitool's modules as a native mod. /ot help lists the commands, /ot list the modules.";

/// Java's `Edition.CURSEFORGE`: native mods have no CurseForge build, so the
/// full edition. The logic stays for parity.
pub const CURSEFORGE: bool = false;
/// Java's `Edition.BUILD`; 0 = a build from source (Java: beta features on).
pub const BUILD: u32 = 0;

/// Java's `Category`, in its order (`/ot list` prints them in it).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Render,
    Hud,
    Player,
    Combat,
    World,
    Misc,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Category::Render,
        Category::Hud,
        Category::Player,
        Category::Combat,
        Category::World,
        Category::Misc,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Category::Render => "Render",
            Category::Hud => "HUD",
            Category::Player => "Player",
            Category::Combat => "Combat",
            Category::World => "World",
            Category::Misc => "Misc",
        }
    }
}

/// The player, as the hooks give it.
#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub dimension: String,
}

/// The camera's screen: GUI size in pixels and the vertical field of view in
/// degrees (Waypoints' labels in the world).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub gui_w: f32,
    pub gui_h: f32,
    pub fov: f32,
}

/// What Session Stats reads each tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionSample {
    /// Counts up each time the player joins a world or server.
    pub join: u64,
    pub flying: bool,
    pub fall_flying: bool,
    pub passenger: bool,
    pub spectator: bool,
    /// `isDeadOrDying`.
    pub dead: bool,
    /// The game's statistics: blocks mined and mobs killed so far.
    pub mined_total: i64,
    pub kills_total: i64,
}

/// Java's `WindowState` (Dynamic FPS).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowState {
    pub focused: bool,
    pub hovered: bool,
    /// Minimized, hidden or covered.
    pub invisible: bool,
}

/// What the game offers a module. glue.rs implements it on the game; the
/// tests on memory. The defaults stand in where the game gives nothing.
pub trait Host {
    /// A line in chat; legacy `§` codes colour it.
    fn chat(&mut self, line: &str);
    fn player(&self) -> Option<Player>;
    /// The camera's screen; None where there is none.
    fn view(&self) -> Option<View> {
        None
    }
    /// Omnitool's folder (config.json, profiles/); None keeps nothing.
    fn data_dir(&self) -> Option<PathBuf>;
    /// Java's action bar message (`Mc.actionBar`); chat where there is none.
    fn action_bar(&mut self, text: &str) {
        self.chat(text);
    }
    /// The text on the clipboard; None when there is none.
    fn clipboard(&mut self) -> Option<String> {
        None
    }
    /// Whether the text is on the clipboard now.
    fn set_clipboard(&mut self, _text: &str) -> bool {
        false
    }
    /// The game's language code (`de_de`), for language AUTO.
    fn language_code(&self) -> Option<String> {
        None
    }
    fn server(&self) -> Server {
        Server::Unknown
    }
    /// Width of a text line in GUI pixels (`native_text_width`).
    fn text_width(&self, text: &str) -> f32 {
        text.chars().count() as f32 * 6.0
    }
    /// Wall clock in ms (HUD blinking, rainbow, pickup timers).
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64)
    }
    /// Armor and held items.
    fn worn(&self) -> HudWorn {
        HudWorn::default()
    }
    /// Active effects.
    fn effects(&self) -> Vec<HudEffect> {
        Vec::new()
    }
    /// Inventory counts and screen/gamemode state for Item Pickups.
    fn inventory(&self) -> HudInventory {
        HudInventory::default()
    }
    /// A vanilla key mapping (`key.forward`): held, and its key label.
    fn mapping(&mut self, _name: &str) -> Option<(bool, String)> {
        None
    }
    /// The warning sound of Durability Warning.
    fn play_warning_sound(&mut self) {}
    /// The game state conditions, triggers and the recorder read; None
    /// without a player or level.
    fn world(&self) -> Option<World> {
        None
    }
    /// Any screen open (inventory, chat, menu).
    fn screen_open(&self) -> bool {
        false
    }
    /// Focus, mouse hover and minimized/hidden state of the window.
    fn window_state(&self) -> WindowState {
        WindowState::default()
    }
    /// Fullbright: the renderer's full-brightness flag on top of the player's
    /// own option (called every tick; false gives the option back).
    fn set_fullbright(&mut self, _on: bool) {}
    /// No Fog: whether the distance fog is off on top of the player's own
    /// option (called every tick; false gives the option back).
    fn set_distance_fog_removed(&mut self, _removed: bool) {}
    /// Leaves the world or server like the pause menu's Disconnect button
    /// (Java's `Session.disconnect`); nothing without a level.
    fn disconnect(&mut self) {}
    /// The player's name (Chat Log's `sender` of sent lines).
    fn player_name(&self) -> Option<String> {
        None
    }
    /// Sends a chat line to the server (Java's `sendChat`).
    fn send_chat(&mut self, _text: &str) {}
    /// Sends a command without its slash (Java's `sendCommand`).
    fn send_command(&mut self, _command: &str) {}
    /// Simulated player input for workflow steps; None = not available.
    fn input(&mut self) -> Option<&mut dyn Input> {
        None
    }
    /// The main hand item as `namespace:path`, "" for an empty hand
    /// (Autoclicker's item filter).
    fn main_hand_id(&self) -> String {
        String::new()
    }
    /// The crosshair is on a block (`hitResult` of type BLOCK).
    fn aiming_at_block(&self) -> bool {
        false
    }
    /// `getAttackStrengthScale(0) >= 1`: the attack is fully charged.
    fn attack_charged(&self) -> bool {
        true
    }
    /// For the block under the crosshair: the 9 hotbar items' base
    /// destroy speed and durability left (None = not damageable). None
    /// without a block in reach.
    fn mining_hotbar(&self) -> Option<Vec<HotbarTool>> {
        None
    }
    /// Auto Refill's view of the inventory menu; None without a player.
    fn refill_view(&self) -> Option<RefillView> {
        None
    }
    /// A left click (PICKUP) on a slot of the inventory menu; false when the
    /// game didn't take it.
    fn click_menu_slot(&mut self, _slot: usize) -> bool {
        false
    }
    /// The cursor holds no stack.
    fn carried_empty(&self) -> bool {
        true
    }
    /// What the Omni Overlay entries read; unknown parts stay empty.
    fn overlay_info(&self) -> OverlayInfo {
        OverlayInfo::default()
    }
    /// The statistic `time since rest` in ticks (Omni Overlay, phantoms).
    fn time_since_rest(&self) -> Option<i64> {
        None
    }
    /// Asks the server for the statistics (`REQUEST_STATS`).
    fn request_stats(&mut self) {}
    /// World Map: the chunks the game has loaded and the world's revision.
    fn map_loaded(&self) -> Option<super::omnitool_world_map::Loaded> {
        None
    }
    /// World Map: the columns of a loaded chunk; None when it isn't loaded.
    fn map_scan(&mut self, _cx: i32, _cz: i32) -> Option<super::omnitool_world_map::Tile> {
        None
    }
    /// Shift (left or right) is held.
    fn shift_down(&self) -> bool {
        false
    }
    /// Alt (left or right) is held.
    fn alt_down(&self) -> bool {
        false
    }
    /// Ctrl (left or right) is held.
    fn ctrl_down(&self) -> bool {
        false
    }
    /// The selected hotbar slot 0-8 (recorder).
    fn hotbar_slot(&self) -> i32 {
        0
    }
    /// What Session Stats reads; None without a player.
    fn session_sample(&self) -> Option<SessionSample> {
        None
    }
    /// `System.nanoTime()`.
    fn nano_time(&self) -> i64 {
        super::omnitool_tps::nano_time()
    }
    /// The craft calculator's recipes and inventory (`native_recipes`,
    /// `native_known_recipes`); None without a world.
    fn craft_data(&mut self) -> Option<super::omnitool_craft_calc::CraftData> {
        None
    }
    /// Shows recipes in the item panel under `title` (`native_panel_recipes`).
    fn craft_panel(&mut self, _title: &str, _recipes: &[super::omnitool_craft_calc::PanelRecipe]) {}
    /// Puts the recipe book recipe `handle` (a `CraftOption` handle) into the
    /// open crafting grid (`native_place_recipe`).
    fn craft_place(&mut self, _handle: &str) -> Result<(), String> {
        Err("Placing recipes is not available.".into())
    }
}

/// One hotbar item as Auto Tool sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HotbarTool {
    /// `ItemStack.getDestroySpeed(state)`: 1.0 for an empty hand.
    pub speed: f64,
    pub durability_left: Option<i32>,
}

/// The inventory menu as Auto Refill reads it each tick.
#[derive(Clone, Debug, Default)]
pub struct RefillView {
    /// Menu slots 9..=45 (index = slot - 9).
    pub slots: Vec<super::omnitool_auto_refill::Stack>,
    /// No screen, cursor empty, not creative: a refill may click now.
    pub ready: bool,
}

/// A command a module answers: `(host, name, args)` → whether it ran.
/// `name` is the word after the slash, `args` the rest after one space.
pub type Command = fn(&mut dyn Host, &str, &str) -> bool;

/// One module, as Java's `Module` describes it. Each module's file has one as
/// `MODULE`; registry.rs lists them.
#[allow(dead_code)]
pub struct ModuleInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub category: Category,
    pub description: &'static str,
    /// On in a fresh config (Java: `setEnabled(true)` in the constructor).
    pub default_on: bool,
    /// Java's `cheaty()`: Safe Mode blocks it by default; not in the
    /// CurseForge edition.
    pub cheaty: bool,
    /// Java's `beta()`: only on the beta channel.
    pub beta: bool,
    /// Java's `fairVariant()`: switches to fair play under Safe Mode.
    pub fair_variant: bool,
    /// Java's `replaces(...)`: the mods it replaces, or "".
    pub replaces: &'static str,
    /// The `key` and `hold` a fresh config has (Java sets them in the
    /// constructor; `NONE`/false for most modules).
    pub key: i32,
    pub hold: bool,
    /// The module's own settings, in Java's order (not the four built-in ones).
    pub settings: &'static [SettingDef],
    pub command: Option<Command>,
    /// The module's HUD element this frame (None = nothing to draw).
    pub hud: Option<HudFn>,
    /// Java's `tick`: every client tick in a world, active or not.
    pub tick: Option<TickFn>,
    /// A key event (vanilla key name, pressed) for modules with their own
    /// key logic (Omni Overlay).
    pub on_key: Option<KeyFn>,
    /// A second HUD box (the right column of Omni Overlay).
    pub hud_right: Option<HudFn>,
    /// A click on one of the module's `Kind::Action` settings (the setting's
    /// id); the Settings page's button for it. None = the module has none.
    pub on_action: Option<ActionFn>,
}

pub type HudFn = fn(&mut dyn Host) -> Option<HudBox>;
pub type TickFn = fn(&mut dyn Host);
pub type KeyFn = fn(&mut dyn Host, &str, bool);
pub type ActionFn = fn(&mut dyn Host, &str);

/// The four settings every module has (Java's `Module` fields).
pub const KEY: SettingDef = SettingDef::new("key", "Shortcut key (optional)", Kind::Key(NONE));
pub const HOLD: SettingDef =
    SettingDef::new("hold", "Only on while the key is held", Kind::Bool(false));
pub const SAFE_BLOCK: SettingDef =
    SettingDef::new("safe_block", "Blocked by Safe Mode", Kind::Bool(false));
pub const SAFE_IGNORE: SettingDef = SettingDef::new(
    "safe_ignore",
    "Ignore Safe Mode for this module",
    Kind::Bool(false),
);

/// The global settings, in config.json's order.
pub const GLOBALS: [SettingDef; 8] = [
    SettingDef::new(
        "safe_mode",
        "Safe Mode",
        Kind::Enum {
            values: MODES,
            default: AUTO,
        },
    ),
    SettingDef::new(
        "safe_whitelist",
        "Servers where Safe Mode stays off in Auto mode (comma separated)",
        Kind::Text("localhost"),
    ),
    SettingDef::new("gui_key", "Open Omnitool menu", Kind::Key(344)),
    SettingDef::new(
        "safe_key",
        "Toggle Safe Mode (this session)",
        Kind::Key(NONE),
    ),
    SettingDef::new(
        "language",
        "Language",
        Kind::Enum {
            values: LANGUAGES,
            default: 0,
        },
    ),
    SettingDef::new(
        "update_mode",
        "Updates",
        Kind::Enum {
            values: &["AUTO", "NOTIFY", "OFF"],
            default: 0,
        },
    ),
    SettingDef::new(
        "update_channel",
        "Update channel",
        Kind::Enum {
            values: &["STABLE", "BETA"],
            default: 0,
        },
    ),
    SettingDef::new("radial_key", "Radial menu (hold)", Kind::Key(90)),
];
pub const G_SAFE_MODE: usize = 0;
pub const G_WHITELIST: usize = 1;
pub const G_GUI_KEY: usize = 2;
pub const G_SAFE_KEY: usize = 3;
pub const G_LANGUAGE: usize = 4;
pub const G_RADIAL_KEY: usize = 7;

/// Config autosave interval in ticks (Java's `AUTOSAVE_TICKS`).
const AUTOSAVE_TICKS: u32 = 60;

/// A module's state (Java's `Module` instance fields).
#[derive(Clone, Debug, PartialEq)]
pub struct ModState {
    pub enabled: bool,
    pub key: i32,
    pub hold: bool,
    pub safe_block: bool,
    pub safe_ignore: bool,
    pub favorite: bool,
    /// Set when the module failed; inactive for the session.
    pub failure: Option<String>,
    /// The module's own settings, as `ModuleInfo::settings`.
    pub values: Vec<Value>,
}

impl ModState {
    /// The defaults Java captures after the constructor.
    fn new(m: &ModuleInfo) -> ModState {
        ModState {
            enabled: m.default_on,
            key: m.key,
            hold: m.hold,
            safe_block: m.cheaty,
            safe_ignore: false,
            favorite: false,
            failure: None,
            values: m.settings.iter().map(SettingDef::default_value).collect(),
        }
    }
}

/// Everything the core keeps for one game: modules, globals, config.json.
pub struct State {
    /// Java's `Beta.on()`, decided once at load.
    pub beta: bool,
    /// One per entry of `MODULES` (loaded or not).
    pub modules: Vec<ModState>,
    /// One per entry of `GLOBALS`.
    pub globals: Vec<Value>,
    /// The current profile's name, "" = none.
    pub profile: String,
    /// The workflows (Java's `Bindings.LIST`).
    pub bindings: Vec<Binding>,
    /// HUD positions `[x, y]` of the loaded HUD elements (none yet).
    pub hud: Obj,
    pub safe_override: Override,
    pub pending_import: Option<Obj>,
    dir: Option<PathBuf>,
    on_disk: Obj,
    baseline: Obj,
    last_json: String,
    loaded: bool,
    ticks: u32,
    in_world: bool,
}

thread_local! {
    /// The core's state, loaded on first use. Horizon runs one `Game` per
    /// thread (window, integrated server); each has its own.
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Runs `f` on the state (loading it first). `f` must not call back into
/// `with`/`read`.
pub fn with<R>(host: &mut dyn Host, f: impl FnOnce(&mut State, &mut dyn Host) -> R) -> R {
    STATE.with(|cell| {
        let mut s = cell.borrow_mut();
        let state = s.get_or_insert_with(|| State::load(&*host));
        f(state, host)
    })
}

pub fn read<R>(host: &dyn Host, f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|cell| {
        let mut s = cell.borrow_mut();
        let state = s.get_or_insert_with(|| State::load(host));
        f(state)
    })
}

/// Java's `Modules.byId`: trimmed, case-insensitive, any module of this build.
pub fn module(id: &str) -> Option<&'static ModuleInfo> {
    let id = id.trim();
    MODULES.iter().find(|m| m.id.eq_ignore_ascii_case(id))
}

fn index(m: &ModuleInfo) -> usize {
    MODULES.iter().position(|o| o.id == m.id).unwrap_or(0)
}

/// Java's `Beta.init`: CurseForge never; else a build from source or channel BETA
/// (read raw from config.json).
fn beta_on(root: Option<&Obj>) -> bool {
    if CURSEFORGE {
        return false;
    }
    let channel = root
        .and_then(|r| r.get("update_channel"))
        .and_then(Json::as_string);
    BUILD == 0 || channel.is_some_and(|c| c.eq_ignore_ascii_case("BETA"))
}

/// Java's HUD element positions at their defaults (`[x, y]` per element id).
fn hud_defaults(beta: bool) -> Obj {
    let mut o = Obj::new();
    for e in HUD_ELEMENTS {
        if !beta && module(e.module).is_some_and(|m| m.beta) {
            continue;
        }
        o.insert(
            e.id,
            Json::Arr(vec![Json::Int(e.x.into()), Json::Int(e.y.into())]),
        );
    }
    o
}

/// Reads one built-in setting from JSON into `v` (None where Java throws).
fn read_def(def: &SettingDef, mut v: Value, j: &Json) -> Option<Value> {
    def.from_json(&mut v, j).then_some(v)
}

impl State {
    fn new(beta: bool, dir: Option<PathBuf>) -> State {
        State {
            beta,
            modules: MODULES.iter().map(ModState::new).collect(),
            globals: GLOBALS.iter().map(SettingDef::default_value).collect(),
            profile: String::new(),
            bindings: Vec::new(),
            hud: hud_defaults(beta),
            safe_override: Override::default(),
            pending_import: None,
            dir,
            on_disk: Obj::new(),
            baseline: Obj::new(),
            last_json: String::new(),
            loaded: false,
            ticks: 0,
            in_world: false,
        }
    }

    /// Java's start: `Beta.init` → modules with their defaults → `Config.load`.
    pub fn load(host: &dyn Host) -> State {
        let dir = host.data_dir();
        let file = dir.as_ref().map(|d| d.join("config.json"));
        let root = file.as_deref().and_then(config::read);
        let mut s = State::new(beta_on(root.as_ref()), dir);
        if let Some(root) = &root {
            let mut ok = true;
            for key in [
                "modules",
                "safe_mode",
                "safe_whitelist",
                "gui_key",
                "safe_key",
                "language",
            ]
            .into_iter()
            .chain([
                "update_mode",
                "update_channel",
                "radial_key",
                "profile",
                "bindings",
                "hud",
            ]) {
                let Some(j) = root.get(key) else { continue };
                let good = match (key, j) {
                    ("modules", Json::Obj(o)) => {
                        s.apply_modules(o);
                        true
                    }
                    ("profile", j) => j.as_string().map(|p| s.profile = p).is_some(),
                    ("bindings", Json::Arr(a)) => match workflows::parse_bindings(a) {
                        Some(list) => {
                            s.bindings = list;
                            true
                        }
                        None => false,
                    },
                    ("hud", Json::Obj(h)) => {
                        s.apply_hud(h);
                        true
                    }
                    ("modules" | "bindings" | "hud", _) => false,
                    (key, j) => match GLOBALS.iter().position(|g| g.id == key) {
                        Some(i) => GLOBALS[i].from_json(&mut s.globals[i], j),
                        None => true,
                    },
                };
                if !good {
                    config::log(&format!(
                        "Could not load '{key}' from config.json, using defaults for it"
                    ));
                    ok = false;
                }
            }
            if !ok && let Some(f) = &file {
                let copy = config::set_aside(f, false);
                config::log(&format!(
                    "Parts of config.json could not be loaded, a copy was kept as {copy:?}"
                ));
            }
        }
        s.on_disk = root.unwrap_or_default();
        s.baseline = s.snapshot();
        s.last_json = Json::Obj(config::merge(
            Some(&s.on_disk),
            Some(&s.baseline),
            &s.baseline,
        ))
        .pretty();
        s.loaded = file.is_some();
        s
    }

    /// Java's `Modules.ALL`: beta modules only on the beta channel, cheaty
    /// ones not in the CurseForge edition (the single place that decides it).
    pub fn loaded(&self, m: &ModuleInfo) -> bool {
        (!m.beta || self.beta) && (!m.cheaty || !CURSEFORGE)
    }

    /// `byId` over the loaded modules.
    pub fn loaded_module(&self, id: &str) -> Option<&'static ModuleInfo> {
        module(id).filter(|m| self.loaded(m))
    }

    pub fn state(&self, m: &ModuleInfo) -> &ModState {
        &self.modules[index(m)]
    }

    pub fn state_mut(&mut self, m: &ModuleInfo) -> &mut ModState {
        &mut self.modules[index(m)]
    }

    pub fn german(&self, host: &dyn Host) -> bool {
        let language = LANGUAGES[self.globals[G_LANGUAGE].index().min(LANGUAGES.len() - 1)];
        lang::german(language, host.language_code().as_deref())
    }

    pub fn safe_blocking(&self, host: &dyn Host) -> bool {
        let mode = self.globals[G_SAFE_MODE].index();
        safe::blocking(
            CURSEFORGE,
            mode,
            &host.server(),
            self.globals[G_WHITELIST].text(),
            &self.safe_override,
        )
    }

    /// Java's `SafeMode.toggle`: flips it for this server; the new state.
    pub fn toggle_safe_mode(&mut self, host: &mut dyn Host) -> bool {
        let mode = self.globals[G_SAFE_MODE].index();
        let server = host.server();
        let next = safe::toggle(
            CURSEFORGE,
            mode,
            &server,
            self.globals[G_WHITELIST].text(),
            &mut self.safe_override,
        );
        host.action_bar(&format!(
            "Omnitool Safe Mode: {}",
            if next { "ON" } else { "OFF" }
        ));
        next
    }

    /// Java's `trustCurrentServer`: the address added, or None.
    pub fn trust_server(&mut self, host: &dyn Host) -> Option<String> {
        let (list, ip) = safe::trust(&host.server(), self.globals[G_WHITELIST].text())?;
        self.globals[G_WHITELIST] = Value::Text(list);
        self.safe_override = Override::default();
        Some(ip)
    }

    /// Java's `Module.safeMode()`.
    pub fn module_safe_mode(&self, host: &dyn Host, m: &ModuleInfo) -> bool {
        safe::module_safe_mode(
            CURSEFORGE,
            self.safe_blocking(host),
            self.state(m).safe_ignore,
        )
    }

    /// Java's `isBlocked`.
    #[allow(dead_code)] // used by modules and menus still to come
    pub fn blocked(&self, host: &dyn Host, m: &ModuleInfo) -> bool {
        let s = self.state(m);
        safe::module_blocked(
            CURSEFORGE,
            self.safe_blocking(host),
            s.safe_block,
            s.safe_ignore,
        )
    }

    /// Java's `isActive`: loaded, on, not failed, not blocked.
    pub fn active(&self, host: &dyn Host, m: &ModuleInfo) -> bool {
        let s = self.state(m);
        self.loaded(m) && s.enabled && s.failure.is_none() && !self.blocked(host, m)
    }

    /// `ON`, `ON (blocked by Safe Mode)` or `OFF`.
    fn state_text(&self, host: &dyn Host, m: &ModuleInfo) -> &'static str {
        match (self.state(m).enabled, self.blocked(host, m)) {
            (true, true) => "ON (blocked by Safe Mode)",
            (true, false) => "ON",
            _ => "OFF",
        }
    }

    /// Java's `Config.serializeModules`.
    pub fn serialize_modules(&self) -> Obj {
        let mut modules = Obj::new();
        for (m, s) in MODULES
            .iter()
            .zip(&self.modules)
            .filter(|(m, _)| self.loaded(m))
        {
            let mut o = Obj::new();
            // hold-key modules are only on while the key is held
            o.insert("enabled", Json::Bool(s.enabled && !s.hold));
            o.insert("key", Json::Int(s.key as i64));
            o.insert("hold", Json::Bool(s.hold));
            o.insert("safe_block", Json::Bool(s.safe_block));
            o.insert("safe_ignore", Json::Bool(s.safe_ignore));
            o.insert("favorite", Json::Bool(s.favorite));
            let mut settings = Obj::new();
            for (def, v) in m.settings.iter().zip(&s.values) {
                settings.insert(def.id, def.to_json(v));
            }
            o.insert("settings", Json::Obj(settings));
            modules.insert(m.id, Json::Obj(o));
        }
        modules
    }

    /// Java's `Config.applyModules`: per module key, hold, safe_block,
    /// safe_ignore, favorite, settings, then enabled; a bad field stops that
    /// module, a bad setting only itself.
    pub fn apply_modules(&mut self, modules: &Obj) {
        for m in MODULES {
            if !self.loaded(m) {
                continue;
            }
            let Some(j) = modules.get(m.id) else { continue };
            let done = match j {
                Json::Obj(o) => self.apply_module(m, o),
                _ => None,
            };
            if done.is_none() {
                config::log(&format!("Could not load settings of {}", m.id));
            }
        }
    }

    fn apply_module(&mut self, m: &ModuleInfo, o: &Obj) -> Option<()> {
        let s = self.state_mut(m);
        if let Some(j) = o.get("key") {
            s.key = read_def(&KEY, Value::Key(s.key), j)?.key();
        }
        if let Some(j) = o.get("hold") {
            s.hold = read_def(&HOLD, Value::Bool(s.hold), j)?.on();
        }
        if let Some(j) = o.get("safe_block") {
            s.safe_block = read_def(&SAFE_BLOCK, Value::Bool(s.safe_block), j)?.on();
        }
        if let Some(j) = o.get("safe_ignore") {
            s.safe_ignore = read_def(&SAFE_IGNORE, Value::Bool(s.safe_ignore), j)?.on();
        }
        if let Some(j) = o.get("favorite") {
            s.favorite = j.as_bool()?;
        }
        if let Some(j) = o.get("settings") {
            let settings = j.as_obj()?;
            for (def, v) in m.settings.iter().zip(s.values.iter_mut()) {
                if let Some(j) = settings.get(def.id)
                    && !def.from_json(v, j)
                {
                    config::log(&format!("Could not load setting {}.{}", m.id, def.id));
                }
            }
        }
        if let Some(j) = o.get("enabled") {
            s.enabled = j.as_bool()?;
        }
        Some(())
    }

    /// Java's `applyHud`: positions of the loaded HUD elements.
    pub fn apply_hud(&mut self, hud: &Obj) {
        for (id, pos) in hud.iter() {
            if !self.hud.has(id) {
                continue;
            }
            match pos {
                Json::Arr(a)
                    if a.len() >= 2 && a[0].as_int().is_some() && a[1].as_int().is_some() =>
                {
                    self.hud.insert(id, pos.clone());
                }
                _ => config::log(&format!("Bad HUD position for {id}")),
            }
        }
    }

    /// Java's `Config.snapshot`: everything this build knows.
    pub fn snapshot(&self) -> Obj {
        let mut root = Obj::new();
        root.insert("modules", Json::Obj(self.serialize_modules()));
        for (def, v) in GLOBALS.iter().zip(&self.globals) {
            root.insert(def.id, def.to_json(v));
        }
        root.insert("profile", Json::str(&self.profile));
        root.insert("bindings", self.bindings_json());
        root.insert("hud", Json::Obj(self.hud.clone()));
        root
    }

    fn file(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join("config.json"))
    }

    /// Java's `Config.save`: writes config.json if anything changed, keeping
    /// what this build doesn't know. Nothing before load.
    pub fn save(&mut self) {
        let Some(file) = self.file().filter(|_| self.loaded) else {
            return;
        };
        let now = self.snapshot();
        let out = config::merge(Some(&self.on_disk), Some(&self.baseline), &now);
        let json = Json::Obj(out.clone()).pretty();
        if json == self.last_json && file.exists() {
            self.baseline = now;
            return;
        }
        if config::write(&file, &out) {
            self.on_disk = out;
            self.baseline = now;
            self.last_json = json;
        }
    }

    /// Java's `Config.tick`: saves every 60 ticks and when leaving a world.
    pub fn tick(&mut self, world_loaded: bool) {
        let left = self.in_world && !world_loaded;
        self.in_world = world_loaded;
        self.ticks += 1;
        if left || self.ticks >= AUTOSAVE_TICKS {
            self.ticks = 0;
            self.save();
        }
    }

    fn profile_dir(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join("profiles"))
    }

    /// Java's `Profiles.list`.
    pub fn profiles(&self) -> Vec<String> {
        self.profile_dir()
            .map(|d| config::profiles(&d))
            .unwrap_or_default()
    }

    fn read_profile(&self, name: &str) -> Option<Obj> {
        config::read(&self.profile_dir()?.join(config::profile_file(name)))
    }

    /// Java's `Profiles.save`: the modules into `profiles/<name>.json`,
    /// keeping what the old file had that this build doesn't know.
    pub fn save_profile(&mut self, name: &str) {
        let Some(dir) = self.profile_dir().filter(|_| !name.trim().is_empty()) else {
            return;
        };
        let mut root = Obj::new();
        root.insert("modules", Json::Obj(self.serialize_modules()));
        let file = dir.join(config::profile_file(name.trim()));
        config::write(
            &file,
            &config::merge(config::read(&file).as_ref(), None, &root),
        );
        self.profile = name.trim().to_owned();
        self.save();
    }

    /// Java's `Profiles.load`: applies the profile's modules; action bar text.
    pub fn load_profile(&mut self, host: &mut dyn Host, name: &str) -> bool {
        let modules = self
            .read_profile(name.trim())
            .and_then(|r| r.obj("modules").cloned());
        let Some(modules) = modules else {
            host.action_bar(&format!("Omnitool: profile '{name}' not found"));
            return false;
        };
        self.apply_modules(&modules);
        self.profile = name.trim().to_owned();
        self.save();
        host.action_bar(&format!("Omnitool: profile '{}' loaded", self.profile));
        true
    }

    /// Java's `Profiles.delete`.
    pub fn delete_profile(&mut self, name: &str) {
        if let Some(dir) = self.profile_dir() {
            let _ = std::fs::remove_file(dir.join(config::profile_file(name)));
        }
        if name == self.profile {
            self.profile.clear();
        }
    }

    fn bindings_json(&self) -> Json {
        Json::Arr(self.bindings.iter().map(Binding::to_json).collect())
    }

    /// Java's `Workflows.applySet`: `module.setting=value`; an error text or None.
    pub fn apply_set(&mut self, arg: &str) -> Option<String> {
        let (dot, eq) = (arg.find('.'), arg.find('='));
        let (Some(dot), Some(eq)) = (dot, eq.filter(|&e| dot.is_some_and(|d| e >= d))) else {
            return Some("Format: module.setting=value".into());
        };
        let Some(m) = self.loaded_module(&arg[..dot]) else {
            return Some(format!("Unknown module {}", &arg[..dot]));
        };
        let setting = workflows::jtrim(&arg[dot + 1..eq]);
        self.set_setting(m, setting, workflows::jtrim(&arg[eq + 1..]))
            .err()
    }

    /// Java's `Workflows.setSetting`: Ok(index of the setting) or an error text.
    pub fn set_setting(
        &mut self,
        m: &ModuleInfo,
        setting: &str,
        value: &str,
    ) -> Result<usize, String> {
        let i = m
            .settings
            .iter()
            .position(|d| d.id == setting)
            .ok_or(format!("Unknown setting {setting}"))?;
        match m.settings[i].set_text(&mut self.state_mut(m).values[i], value) {
            Some(err) => Err(err),
            None => Ok(i),
        }
    }

    /// Java's `ShareCode.collect`: everything shareable.
    pub fn collect(&self, with_workflows: bool) -> Obj {
        let mut root = Obj::new();
        root.insert("v", Json::Int(1));
        let mut modules = self.serialize_modules();
        share::strip_private(&mut modules);
        root.insert("modules", Json::Obj(modules));
        root.insert("hud", Json::Obj(self.hud.clone()));
        let mut keys = Obj::new();
        for i in [G_GUI_KEY, G_RADIAL_KEY, G_SAFE_KEY] {
            keys.insert(GLOBALS[i].id, GLOBALS[i].to_json(&self.globals[i]));
        }
        root.insert("keys", Json::Obj(keys));
        root.insert(
            "safe_mode",
            GLOBALS[G_SAFE_MODE].to_json(&self.globals[G_SAFE_MODE]),
        );
        // "layouts" (Omni Overlay) once that module is ported
        if with_workflows {
            root.insert("bindings", self.bindings_json());
        }
        root
    }

    /// Java's `collectProfile`: None if the profile doesn't exist.
    pub fn collect_profile(&self, name: &str) -> Option<Obj> {
        let mut modules = self.read_profile(name)?.obj("modules")?.clone();
        share::strip_private(&mut modules);
        let mut root = Obj::new();
        root.insert("v", Json::Int(1));
        root.insert("profile", Json::str(name));
        root.insert("modules", Json::Obj(modules));
        Some(root)
    }

    /// Java's `ShareCode.preview`: what an import would change, readable.
    pub fn preview(&self, data: &Obj, german: bool) -> Vec<String> {
        let tr = |t: &str| lang::tr(german, t).to_owned();
        let mut out = Vec::new();
        if let Some(p) = data.get("profile") {
            out.push(format!(
                "{}: {}",
                tr("Profile"),
                p.as_string().unwrap_or_default()
            ));
        }
        let cur = self.serialize_modules();
        let mut unknown = 0;
        for (id, v) in data
            .obj("modules")
            .map(|m| m.iter().collect::<Vec<_>>())
            .unwrap_or_default()
        {
            let (Some(m), Json::Obj(inp)) = (self.loaded_module(id), v) else {
                unknown += 1;
                continue;
            };
            let Some(now) = cur.obj(m.id) else { continue };
            let mn = tr(m.name);
            for f in [
                "enabled",
                "favorite",
                "key",
                "hold",
                "safe_block",
                "safe_ignore",
            ] {
                if let Some(iv) = inp.get(f)
                    && Some(iv) != now.get(f)
                {
                    let (a, b) = (show(now.get(f), f == "key"), show(Some(iv), f == "key"));
                    out.push(format!("{mn}: {} {a} -> {b}", tr(field_name(f))));
                }
            }
            let (Some(si), Some(sn)) = (inp.obj("settings"), now.obj("settings")) else {
                continue;
            };
            for def in m.settings {
                if let Some(iv) = si.get(def.id)
                    && Some(iv) != sn.get(def.id)
                {
                    let key = matches!(def.kind, Kind::Key(_));
                    let (a, b) = (show(sn.get(def.id), key), show(Some(iv), key));
                    out.push(format!("{mn}: {} {a} -> {b}", tr(def.name)));
                }
            }
        }
        if let Some(hud) = data.obj("hud") {
            let n = hud
                .iter()
                .filter(|(k, v)| self.hud.get(k).is_some_and(|now| now != *v))
                .count();
            if n > 0 {
                out.push(format!("{}: {n}", tr("HUD positions")));
            }
        }
        if let Some(keys) = data.obj("keys") {
            for i in [G_GUI_KEY, G_RADIAL_KEY, G_SAFE_KEY] {
                let now = GLOBALS[i].to_json(&self.globals[i]);
                if let Some(k) = keys.get(GLOBALS[i].id)
                    && *k != now
                {
                    out.push(format!(
                        "{}: {} -> {}",
                        tr(GLOBALS[i].name),
                        show(Some(&now), true),
                        show(Some(k), true)
                    ));
                }
            }
        }
        let mode = GLOBALS[G_SAFE_MODE].to_json(&self.globals[G_SAFE_MODE]);
        if let Some(sm) = data.get("safe_mode")
            && *sm != mode
        {
            let (a, b) = (
                mode.as_string().unwrap_or_default(),
                sm.as_string().unwrap_or_default(),
            );
            out.push(format!("Safe Mode: {a} -> {b}"));
        }
        if let Some(bindings) = share_bindings(data) {
            for b in &bindings {
                let name = &b.name;
                match by_name(&self.bindings, name) {
                    None => out.push(format!("{}: {name}", tr("New workflow"))),
                    Some(old) if old != b => {
                        out.push(format!("{}: {name}", tr("Workflow replaced")))
                    }
                    Some(_) => {}
                }
            }
        }
        if unknown > 0 {
            out.push(format!(
                "{}: {unknown}",
                tr("Modules not available in this build (skipped)")
            ));
        }
        out
    }

    /// Java's `ShareCode.apply`, then save.
    pub fn apply(&mut self, data: &Obj) {
        if let Some(mods) = data.obj("modules") {
            let mut mods = mods.clone();
            share::strip_private(&mut mods);
            self.apply_modules(&mods);
        }
        if let Some(hud) = data.obj("hud") {
            self.apply_hud(hud);
        }
        if let Some(keys) = data.obj("keys") {
            for i in [G_GUI_KEY, G_RADIAL_KEY, G_SAFE_KEY] {
                if let Some(k) = keys.get(GLOBALS[i].id) {
                    GLOBALS[i].from_json(&mut self.globals[i], k);
                }
            }
        }
        if let Some(sm) = data.get("safe_mode") {
            GLOBALS[G_SAFE_MODE].from_json(&mut self.globals[G_SAFE_MODE], sm);
        }
        if let Some(bindings) = share_bindings(data) {
            for b in bindings {
                self.bindings
                    .retain(|w| w.name.to_lowercase() != b.name.to_lowercase());
                self.bindings.push(b);
            }
        }
        self.save();
    }
}

/// The workflows of a share code (Java's `parseBindings`, migrated).
fn share_bindings(data: &Obj) -> Option<Vec<Binding>> {
    match data.get("bindings") {
        Some(Json::Arr(a)) => workflows::parse_bindings(a),
        _ => None,
    }
}

/// ShareCode.fieldName (translated by the caller).
fn field_name(f: &str) -> &'static str {
    match f {
        "enabled" => "On",
        "favorite" => "Favourite",
        "key" => "Key",
        "hold" => "Hold",
        "safe_block" => "Blocked by Safe Mode",
        _ => "Ignore Safe Mode for this module",
    }
}

/// ShareCode.show: `-` for nothing/unbound, key names, else the value cut to 24.
fn show(v: Option<&Json>, key: bool) -> String {
    let Some(v) = v else { return "-".into() };
    if key
        && v.is_primitive()
        && let Some(k) = v.as_int()
    {
        return if k == NONE { "-".into() } else { key_name(k) };
    }
    let s = if v.is_primitive() {
        v.as_string().unwrap_or_default()
    } else {
        v.compact()
    };
    if s.chars().count() > 24 {
        s.chars().take(22).chain("...".chars()).collect()
    } else {
        s
    }
}

/// A GLFW key's name as Minecraft shows it (Java's `Keys.name`, US layout).
pub fn key_name(code: i32) -> String {
    let named = match code {
        32 => "Space",
        256 => "Escape",
        257 => "Enter",
        258 => "Tab",
        259 => "Backspace",
        260 => "Insert",
        261 => "Delete",
        262 => "Right Arrow",
        263 => "Left Arrow",
        264 => "Down Arrow",
        265 => "Up Arrow",
        266 => "Page Up",
        267 => "Page Down",
        268 => "Home",
        269 => "End",
        280 => "Caps Lock",
        281 => "Scroll Lock",
        282 => "Num Lock",
        283 => "Print Screen",
        284 => "Pause",
        330 => "Keypad Decimal",
        331 => "Keypad /",
        332 => "Keypad *",
        333 => "Keypad -",
        334 => "Keypad +",
        335 => "Keypad Enter",
        336 => "Keypad =",
        340 => "Left Shift",
        341 => "Left Control",
        342 => "Left Alt",
        343 => "Left Win",
        344 => "Right Shift",
        345 => "Right Control",
        346 => "Right Alt",
        347 => "Right Win",
        348 => "Menu",
        _ => "",
    };
    match code {
        _ if !named.is_empty() => named.into(),
        39 | 44..=57 | 59 | 61 | 65..=93 | 96 => (code as u8 as char).to_string(),
        290..=314 => format!("F{}", code - 289),
        320..=329 => format!("Keypad {}", code - 320),
        _ => format!("Key {code}"),
    }
}

/// A GLFW key's vanilla name (`key.keyboard.c`) as `native_key` gives it;
/// None for codes that have no name.
pub fn vanilla_key_name(code: i32) -> Option<String> {
    let fixed = match code {
        32 => "space",
        39 => "apostrophe",
        44 => "comma",
        45 => "minus",
        46 => "period",
        47 => "slash",
        59 => "semicolon",
        61 => "equal",
        91 => "left.bracket",
        92 => "backslash",
        93 => "right.bracket",
        96 => "grave.accent",
        161 => "world.1",
        162 => "world.2",
        256 => "escape",
        257 => "enter",
        258 => "tab",
        259 => "backspace",
        260 => "insert",
        261 => "delete",
        262 => "right",
        263 => "left",
        264 => "down",
        265 => "up",
        266 => "page.up",
        267 => "page.down",
        268 => "home",
        269 => "end",
        280 => "caps.lock",
        281 => "scroll.lock",
        282 => "num.lock",
        283 => "print.screen",
        284 => "pause",
        330 => "keypad.decimal",
        331 => "keypad.divide",
        332 => "keypad.multiply",
        333 => "keypad.subtract",
        334 => "keypad.add",
        335 => "keypad.enter",
        336 => "keypad.equal",
        340 => "left.shift",
        341 => "left.control",
        342 => "left.alt",
        343 => "left.win",
        344 => "right.shift",
        345 => "right.control",
        346 => "right.alt",
        347 => "right.win",
        348 => "menu",
        _ => "",
    };
    let name = match code {
        _ if !fixed.is_empty() => fixed.to_owned(),
        48..=57 => ((code as u8) as char).to_string(),
        65..=90 => ((code as u8 + 32) as char).to_string(),
        290..=314 => format!("f{}", code - 289),
        320..=329 => format!("keypad.{}", code - 320),
        _ => return None,
    };
    Some(format!("key.keyboard.{name}"))
}

/// Java's `Bindings.tick` step 6 for one key event (`native_key`): each
/// loaded module bound to `key` switches on press (toggle, or on when it
/// `hold`s) and off on release when it holds. Presses only count while
/// `playing`; a release always counts, so a hold never sticks.
pub fn key_event(host: &mut dyn Host, key: &str, pressed: bool, playing: bool) {
    // the Settings page waits for a key to bind: nothing else sees it
    if pressed && super::omnitool_menu::capture_key(host, key) {
        return;
    }
    // Java's Bindings.tick: gui_key opens the ClickGui, only while playing
    if pressed && playing {
        let gui = read(&*host, |s| s.globals[G_GUI_KEY].key());
        if gui != NONE && vanilla_key_name(gui).as_deref() == Some(key) {
            super::omnitool_clickgui::request_open();
            return;
        }
    }
    if pressed && !playing {
        // Villager Trades' key also works in the trade screen
        super::omnitool_villager_trades::screen_key(host, key);
        return;
    }
    with(host, |s, _| {
        for (i, m) in MODULES.iter().enumerate() {
            let code = s.modules[i].key;
            let bound = code != NONE && vanilla_key_name(code).as_deref() == Some(key);
            if !bound || !s.loaded(m) {
                continue;
            }
            let st = &mut s.modules[i];
            if pressed {
                st.enabled = st.hold || !st.enabled;
            } else if st.hold {
                st.enabled = false;
            }
        }
    });
    workflow_key(host, key, pressed);
    for m in MODULES.iter().filter(|m| m.on_key.is_some()) {
        if let Some(f) = m.on_key {
            guard(host, m, |h| f(h, key, pressed));
        }
    }
}

// ------------------------------------------------------------ workflows

/// Runners, triggers and the recorder of this thread's game (Java's
/// `Workflows.RUNNING`, `Triggers`, `Recorder` statics).
#[derive(Default)]
struct Runtime {
    runners: Runners,
    triggers: Triggers,
    recorder: Recorder,
    /// Workflows whose key is held.
    down: Vec<u64>,
}

thread_local! {
    static RUNTIME: RefCell<Runtime> = RefCell::new(Runtime::default());
}

fn runtime<R>(f: impl FnOnce(&mut Runtime) -> R) -> R {
    RUNTIME.with(|r| f(&mut r.borrow_mut()))
}

/// Runs `f` on the runners taken out of `RUNTIME`, so a step may start
/// workflows meanwhile (`command:/ot run x`); those join afterwards.
fn with_runners<R>(host: &mut dyn Host, f: impl FnOnce(&mut Runners, &mut HostEnv) -> R) -> R {
    let mut runners = runtime(|r| std::mem::take(&mut r.runners));
    let out = f(&mut runners, &mut HostEnv { host });
    runtime(|r| {
        let added = std::mem::replace(&mut r.runners, runners);
        r.runners.absorb(added);
    });
    out
}

/// The runner's view of the game.
struct HostEnv<'a> {
    host: &'a mut dyn Host,
}

impl Env for HostEnv<'_> {
    fn beta(&self) -> bool {
        read(&*self.host, |s| s.beta)
    }
    fn curseforge(&self) -> bool {
        CURSEFORGE
    }
    fn safe_blocking(&mut self) -> bool {
        let h: &dyn Host = &*self.host;
        read(h, |s| s.safe_blocking(h))
    }
    fn screen_open(&mut self) -> bool {
        self.host.screen_open()
    }
    fn condition(&mut self, raw: &str) -> bool {
        let Some(c) = conditions::parse(raw) else {
            self.host
                .action_bar(&format!("Omnitool: unknown condition \"{raw}\""));
            return false;
        };
        let world = self.host.world();
        read(&*self.host, |s| {
            conditions::test(&c, world.as_ref(), &|id| {
                s.loaded_module(id).map(|m| s.state(m).enabled)
            })
        })
    }
    fn action_bar(&mut self, text: &str) {
        self.host.action_bar(text);
    }
    fn step(&mut self, raw: &str, revert: bool) {
        let verb = workflows::verb(raw);
        let a = workflows::arg(raw).to_owned();
        match verb.as_str() {
            "toggle" | "enable" | "disable" => with(self.host, |s, _| {
                if let Some(m) = s.loaded_module(&a) {
                    let st = s.state_mut(m);
                    st.enabled = match verb.as_str() {
                        "toggle" => !st.enabled,
                        "enable" => !revert,
                        _ => revert,
                    };
                }
            }),
            "safemode" => with(self.host, |s, host| {
                let now = s.safe_blocking(host);
                let want = match a.to_lowercase().as_str() {
                    "on" => !revert,
                    "off" => revert,
                    _ => !now,
                };
                if want != now {
                    s.toggle_safe_mode(host);
                }
            }),
            _ if revert => {}
            // the error text is dropped (only /ot set shows it)
            "set" => {
                with(self.host, |s, _| s.apply_set(&a));
            }
            "profile" => {
                with(self.host, |s, host| s.load_profile(host, &a));
            }
            "chat" => {
                if self.host.player().is_some() && !a.is_empty() {
                    self.host.send_chat(&a);
                }
            }
            "command" => {
                let cmd = a.strip_prefix('/').unwrap_or(&a);
                if self.host.player().is_some() && !cmd.is_empty() {
                    self.host.send_command(cmd);
                }
            }
            // no confirmation, whatever Quick Disconnect says
            "disconnect" => self.host.disconnect(),
            // freecam_control, waypoint, sort, panorama, tripod_*,
            // hotbar_preset, camera_stop: their modules aren't ported yet
            _ => {}
        }
    }
    fn workflow(&mut self, name: &str) -> Option<Vec<String>> {
        read(&*self.host, |s| {
            by_name(&s.bindings, name).map(|b| b.steps.clone())
        })
    }
    fn input(&mut self) -> Option<&mut dyn Input> {
        self.host.input()
    }
}

/// Java's `Workflows.run(b)` for the workflow with this identity.
pub fn run_workflow(host: &mut dyn Host, uid: u64) {
    let steps = read(host, |s| {
        s.bindings
            .iter()
            .find(|b| b.uid == uid)
            .map(|b| b.steps.clone())
    });
    if let Some(steps) = steps {
        with_runners(host, |r, env| r.run(&steps, uid, env));
    }
}

/// Java's `Workflows.stop(b)`: every run of it, also those it started.
pub fn stop_workflow(host: &mut dyn Host, uid: u64) {
    with_runners(host, |r, env| r.stop(uid, env));
}

/// Java's `Workflows.running(b)`.
pub fn workflow_recording(uid: u64) -> bool {
    runtime(|r| r.recorder.recording(uid))
}

pub fn workflow_running(uid: u64) -> bool {
    runtime(|r| r.runners.running(uid))
}

/// Java's `Workflows.VILLAGER_KEY`: the owner of the Villager Trades key's
/// runs (no workflow has this identity).
pub const VILLAGER_KEY: u64 = u64::MAX;

/// Java's `Workflows.villagerKey(coords)`: a second press stops the run,
/// else the `villager:<coords>` step runs on its own.
pub fn villager_key(host: &mut dyn Host, coords: &str) {
    if runtime(|r| r.runners.running(VILLAGER_KEY)) {
        with_runners(host, |r, env| r.stop(VILLAGER_KEY, env));
        host.action_bar("Omnitool: trading stopped");
        return;
    }
    let steps = [format!("villager:{coords}")];
    with_runners(host, |r, env| r.run(&steps, VILLAGER_KEY, env));
}

/// Undo of a HOLD workflow (key released).
fn revert_workflow(host: &mut dyn Host, uid: u64) {
    let steps = read(host, |s| {
        s.bindings
            .iter()
            .find(|b| b.uid == uid)
            .map(|b| b.steps.clone())
    });
    if let Some(steps) = steps {
        workflows::revert(&steps, &mut HostEnv { host });
    }
}

/// A chat or system line was received (for chat triggers); glue.rs reads
/// them from the game each tick.
pub fn chat_received(text: &str) {
    runtime(|r| r.triggers.chat(text));
}

/// Java's `Recorder.start` (the menu's Record button).
pub fn start_recording(uid: u64) {
    runtime(|r| r.recorder.start(uid));
}

/// Java's `Recorder.stop`: appends the recorded steps to the workflow.
pub fn stop_recording(host: &mut dyn Host) {
    let Some((uid, steps)) = runtime(|r| r.recorder.stop()) else {
        return;
    };
    let n = steps.len();
    with(host, |s, _| {
        if let Some(b) = s.bindings.iter_mut().find(|b| b.uid == uid) {
            b.steps.extend(steps);
        }
        s.save();
    });
    let text = format!("{}: {n}", tr(host, "Omnitool: recorded steps"));
    host.action_bar(&text);
}

/// Java's `Recorder.tick`.
fn recorder_tick(host: &mut dyn Host) {
    let Some(uid) = runtime(|r| r.recorder.target) else {
        return;
    };
    let name = read(host, |s| {
        s.bindings
            .iter()
            .find(|b| b.uid == uid)
            .map(|b| b.name.clone())
    });
    let (Some(player), Some(name)) = (host.player(), name) else {
        return stop_recording(host);
    };
    // never record while a screen is open
    if host.screen_open() {
        return;
    }
    let mut keys = 0;
    for (i, k) in workflows::MOVE_KEYS.iter().enumerate() {
        if host.mapping(&format!("key.{k}")).is_some_and(|m| m.0) {
            keys |= 1 << i;
        }
    }
    let sample = Sample {
        keys,
        yaw: player.yaw,
        pitch: player.pitch,
        slot: host.hotbar_slot(),
    };
    let (show, full) = runtime(|r| r.recorder.add(sample));
    if show {
        let n = runtime(|r| r.recorder.samples.len());
        let text = format!(
            "● {} \"{name}\" {} - {}",
            tr(host, "Recording"),
            super::omnitool_recorder::seconds(n),
            tr(host, "press the workflow key or Stop in the menu to finish")
        );
        host.action_bar(&text);
    }
    if full {
        stop_recording(host);
    }
}

/// Java's `Bindings.tick` for workflows: triggers, recorder, runners. Without
/// a player the runners stop (Java's `stopAll` on leaving the world).
fn workflows_tick(host: &mut dyn Host) {
    let world = host.world();
    let fired = read(host, |s| {
        runtime(|r| r.triggers.tick(s.beta, world.as_ref(), &s.bindings))
    });
    for uid in fired {
        run_workflow(host, uid);
    }
    recorder_tick(host);
    if host.player().is_none() {
        if !runtime(|r| r.runners.is_empty()) {
            with_runners(host, |r, env| r.stop_all(env));
        }
        return;
    }
    with_runners(host, |r, env| r.tick(env));
}

/// Workflow keys (Java's `Bindings.tick` edge per binding): a press runs the
/// workflow (or ends its recording), a release undoes a HOLD workflow.
fn workflow_key(host: &mut dyn Host, key: &str, pressed: bool) {
    let bound: Vec<(u64, bool)> = read(host, |s| {
        s.bindings
            .iter()
            .filter(|b| b.key != NONE && vanilla_key_name(b.key).as_deref() == Some(key))
            .map(|b| (b.uid, b.hold()))
            .collect()
    });
    for (uid, hold) in bound {
        let was = runtime(|r| {
            let was = r.down.contains(&uid);
            r.down.retain(|u| *u != uid);
            if pressed {
                r.down.push(uid);
            }
            was
        });
        if pressed && !was {
            if runtime(|r| r.recorder.recording(uid)) {
                stop_recording(host);
            } else {
                run_workflow(host, uid);
            }
        } else if !pressed && was && hold {
            revert_workflow(host, uid);
        }
    }
}

/// `/ot run <workflow>`.
fn run_command(host: &mut dyn Host, name: &str) {
    let found = read(host, |s| {
        by_name(&s.bindings, name).map(|b| (b.uid, b.name.clone()))
    });
    match found {
        Some((uid, n)) => {
            run_workflow(host, uid);
            ok(host, &format!("Running workflow {n}"));
        }
        None => fail(host, &format!("Unknown workflow: {name}")),
    }
}

// ------------------------------------------------------------ module API

/// Whether module `m` is on (its saved switch).
#[allow(dead_code)] // used by modules and menus still to come
pub fn enabled(host: &dyn Host, m: &ModuleInfo) -> bool {
    read(host, |s| s.state(m).enabled)
}

/// Java's `setEnabled` (onEnable/onDisable hooks: once modules have them).
#[allow(dead_code)] // used by modules and menus still to come
pub fn set_enabled(host: &mut dyn Host, m: &ModuleInfo, on: bool) {
    with(host, |s, _| s.state_mut(m).enabled = on);
}

/// Java's `isBlocked`: Safe Mode blocks it here.
#[allow(dead_code)] // used by modules and menus still to come
pub fn blocked(host: &dyn Host, m: &ModuleInfo) -> bool {
    read(host, |s| s.blocked(host, m))
}

/// Java's `Module.safeMode()`: fair-variant modules check this.
#[allow(dead_code)] // used by modules and menus still to come
pub fn safe_mode(host: &dyn Host, m: &ModuleInfo) -> bool {
    read(host, |s| s.module_safe_mode(host, m))
}

/// Java's `isActive`. Module logic checks this.
pub fn active(host: &dyn Host, m: &ModuleInfo) -> bool {
    read(host, |s| s.active(host, m))
}

/// The value of module `m`'s setting `id` (its default if unknown).
#[allow(dead_code)] // used by modules and menus still to come
pub fn value(host: &dyn Host, m: &ModuleInfo, id: &str) -> Value {
    let i = m.settings.iter().position(|d| d.id == id);
    read(host, |s| i.map(|i| s.state(m).values[i].clone())).unwrap_or(Value::Bool(false))
}

/// Java's `Lang.tr` with the current language.
#[allow(dead_code)] // used by modules and menus still to come
pub fn tr(host: &dyn Host, english: &str) -> String {
    let german = read(host, |s| s.german(host));
    lang::tr(german, english).to_owned()
}

/// Java's `Modules.guard`: runs `f` unless the module failed; a panic marks
/// it failed for the session (its saved switch stays) instead of ending the game.
#[allow(dead_code)] // used by modules and menus still to come
pub fn guard(host: &mut dyn Host, m: &ModuleInfo, f: impl FnOnce(&mut dyn Host)) {
    if read(host, |s| s.state(m).failure.is_some()) {
        return;
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut *host)));
    if let Err(e) = result {
        let msg = e
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| e.downcast_ref::<String>().cloned());
        let failure = match msg {
            Some(msg) => format!("panic: {msg}"),
            None => "panic".to_owned(),
        };
        with(host, |s, _| s.state_mut(m).failure = Some(failure));
        config::log(&format!(
            "Module {} failed and was disabled for this session",
            m.id
        ));
        host.action_bar(&format!("Omnitool: {} disabled (error, see log)", m.name));
    }
}

/// Once per tick of the window's game: autosave.
pub fn tick(host: &mut dyn Host) {
    let world = host.player().is_some();
    with(host, |s, _| s.tick(world));
    if world {
        for m in MODULES.iter().filter(|m| m.tick.is_some()) {
            if read(host, |s| s.loaded(m))
                && let Some(f) = m.tick
            {
                guard(host, m, f);
            }
        }
    }
    workflows_tick(host);
}

/// Java's `Modules.renderHud`: the boxes of the active modules, in order.
pub fn hud_frame(host: &mut dyn Host) -> Vec<HudBox> {
    let mut boxes = Vec::new();
    if host.player().is_none() {
        return boxes;
    }
    for m in MODULES.iter().filter(|m| m.hud.is_some()) {
        if !active(host, m) {
            continue;
        }
        // Omni Overlay's layout decides when another module's element draws.
        let shown = m.id == super::omnitool_omni_overlay::MODULE.id
            || HUD_ELEMENTS
                .iter()
                .filter(|e| e.module == m.id)
                .all(|e| super::omnitool_omni_overlay::shows_hud(host, e.id));
        if !shown {
            continue;
        }
        if let Some(f) = m.hud {
            guard(host, m, |h| boxes.extend(f(h)));
        }
        if let Some(f) = m.hud_right {
            guard(host, m, |h| boxes.extend(f(h)));
        }
    }
    boxes
}

/// Logs an error line (Java's `Omnitool.LOGGER.error`).
pub fn log_error(msg: &str) {
    config::log(msg);
}

/// Java's `OmniCommand.ok`: gold tag, white text.
pub fn ok(host: &mut dyn Host, msg: &str) {
    host.chat(&format!("§6[Omnitool] §f{msg}"));
}

/// Java's `OmniCommand.fail` (`sendError`: red).
pub fn fail(host: &mut dyn Host, msg: &str) {
    host.chat(&format!("§c[Omnitool] {msg}"));
}

/// Java's `sendError` without the tag (red).
pub fn error(host: &mut dyn Host, msg: &str) {
    host.chat(&format!("§c{msg}"));
}

// ------------------------------------------------------------ /ot

/// A command typed in chat (without its slash): `/omnitool`, `/ot`, then each
/// module's. Whether one of them ran it.
pub fn command(host: &mut dyn Host, line: &str) -> bool {
    let (name, args) = line.split_once(' ').unwrap_or((line, ""));
    if name == "omnitool" || name == "ot" {
        omni(host, args);
        return true;
    }
    let commands: Vec<Command> = read(host, |s| {
        MODULES
            .iter()
            .filter(|m| s.loaded(m))
            .filter_map(|m| m.command)
            .collect()
    });
    commands.into_iter().any(|run| run(host, name, args))
}

/// The first word and the rest (greedy, leading spaces trimmed).
fn word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim_start()),
        None => (s, ""),
    }
}

/// Java's `unquote`: trimmed, one pair of surrounding quotes removed.
fn unquote(s: &str) -> &str {
    let s = s.trim();
    if s.len() > 1 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// `/ot` and its subcommands (Java's `OmniCommand`, the parts ported so far).
fn omni(host: &mut dyn Host, args: &str) {
    let (sub, rest) = word(args);
    let beta = read(host, |s| s.beta);
    match sub {
        // Java opens the menu; the native mod has no menu yet, so its help.
        "" | "menu" | "help" => help(host),
        "list" => list(host),
        "toggle" | "on" | "off" => match word(rest) {
            (id, "") if !id.is_empty() => switch(host, sub, id),
            _ => fail(host, &format!("Usage: /ot {sub} <module>")),
        },
        "set" => set(host, rest),
        "hotbar" if beta => super::omnitool_hotbar_presets::ot_hotbar(host, rest),
        "hotbar" => fail(
            host,
            "Hotbar presets are a beta feature (update channel BETA)",
        ),
        "chat" if beta => super::omnitool_chat_tweaks::ot_chat(host, rest),
        "schem" | "schematic" => super::omnitool_schematics::ot(host, rest),
        "deaths" => super::omnitool_death_coords::print_last(host, 10),
        "waypoint" | "waypoints" => super::omnitool_waypoints::ot(host, rest),
        "run" if !rest.trim().is_empty() => run_command(host, unquote(rest)),
        "run" => fail(host, "Usage: /ot run <workflow>"),
        "safemode" => match rest.trim() {
            "on" => safemode(host, Some(true)),
            "off" => safemode(host, Some(false)),
            "toggle" => safemode(host, None),
            _ => fail(host, "Usage: /ot safemode on|off|toggle"),
        },
        "profile" if !rest.trim().is_empty() => profile_command(host, rest),
        "profile" => fail(
            host,
            "Usage: /ot profile <name> | list | save <name> | delete <name>",
        ),
        "trust" => trust(host),
        "layout" => super::omnitool_omni_overlay::ot_layout(host, rest),
        "workflow" | "workflows" => workflow_command(host, rest),
        "export" if beta => export(host, rest),
        "import" if beta => import(host, rest.trim()),
        _ => fail(host, &format!("Unknown command: /ot {sub} - see /ot help")),
    }
}

/// The help lines of the commands ported so far, as Java writes them.
pub fn help_lines(beta: bool) -> Vec<&'static str> {
    let mut lines = vec![
        "/ot list - all modules (green = on) and workflows",
        "/ot toggle|on|off <module>",
        "/ot run <workflow>",
        "/ot set <module> <setting> <value>",
        "/ot safemode on|off|toggle",
        "/ot profile <name> | list | save <name> | delete <name>",
        "/ot trust - add this server to the Safe Mode whitelist",
        "/ot layout list|use|new|rename|delete|reset|entries|vis|move|side|json|import - Omni Overlay layouts",
        "/ot workflow list|show|new|rename|delete|add|set|del|clear|steps|mode|drop|coords|trigger|record|stop - edit workflows",
    ];
    if beta {
        lines.push("/ot export [workflows | profile <name>] - share code to the clipboard");
        lines.push("/ot import [confirm] - preview / apply a share code from the clipboard");
    }
    if beta {
        lines.push("/ot chat search <text> | log | filter list|add|remove");
    }
    lines.push("/ot deaths - your last deaths with coordinates");
    lines
}

fn help(host: &mut dyn Host) {
    let beta = read(host, |s| s.beta);
    host.chat("§6Omnitool");
    for line in help_lines(beta) {
        host.chat(&format!("§7 {line}"));
    }
}

/// One line per category: its modules, green when active, grey when not;
/// then the workflows, if any.
fn list(host: &mut dyn Host) {
    let lines = read(host, |s| {
        let mut lines: Vec<String> = Category::ALL
            .iter()
            .map(|&cat| {
                let names: Vec<String> = MODULES
                    .iter()
                    .filter(|m| m.category == cat && s.loaded(m))
                    .map(|m| format!("{}{}§r", if s.active(host, m) { "§a" } else { "§7" }, m.id))
                    .collect();
                format!("§6{}: §r{}", cat.title(), names.join(", "))
            })
            .collect();
        let workflows: Vec<String> = s.bindings.iter().map(|b| b.name.clone()).collect();
        if !workflows.is_empty() {
            lines.push(format!("§6Workflows: §r{}", workflows.join(", ")));
        }
        lines
    });
    for line in lines {
        host.chat(&line);
    }
}

fn switch(host: &mut dyn Host, verb: &str, id: &str) {
    let msg = with(host, |s, host| {
        let m = s.loaded_module(id)?;
        let st = s.state_mut(m);
        st.enabled = match verb {
            "on" => true,
            "off" => false,
            _ => !st.enabled,
        };
        s.save();
        Some(format!("{}: {}", m.name, s.state_text(host, m)))
    });
    match msg {
        Some(msg) => ok(host, &msg),
        None => fail(host, &format!("Unknown module: {id}")),
    }
}

/// `/ot set <module> <setting> <value>` (Java's `Workflows.setSetting`).
fn set(host: &mut dyn Host, args: &str) {
    let (id, rest) = word(args);
    let (setting, value) = word(rest);
    if value.is_empty() {
        return fail(host, "Usage: /ot set <module> <setting> <value>");
    }
    let result = with(host, |s, _| -> Result<String, String> {
        let m = s
            .loaded_module(id)
            .ok_or_else(|| "Unknown module".to_owned())?;
        let i = s.set_setting(m, setting, value)?;
        let def = &m.settings[i];
        s.save();
        Ok(format!(
            "{}: {setting} = {}",
            m.name,
            def.to_json(&s.state(m).values[i]).compact()
        ))
    });
    match result {
        Ok(msg) => ok(host, &msg),
        Err(e) => fail(host, &e),
    }
}

/// `/ot safemode on|off|toggle`: for the current server.
fn safemode(host: &mut dyn Host, want: Option<bool>) {
    let on = with(host, |s, host| {
        let now = s.safe_blocking(host);
        let on = want.unwrap_or(!now);
        if on != now {
            s.toggle_safe_mode(host);
        }
        on
    });
    ok(
        host,
        &format!(
            "Safe Mode {} for this server",
            if on { "ON" } else { "OFF" }
        ),
    );
}

/// `/ot profile <name>`, `list`, `save <name>`, `delete <name>`. A profile
/// named like one of the words is loaded with `/ot profile load <name>`.
fn profile_command(host: &mut dyn Host, rest: &str) {
    let (sub, arg) = word(rest);
    let name = unquote(arg);
    match sub {
        "list" => {
            let list = read(host, |s| s.profiles());
            if list.is_empty() {
                host.chat("§7 No profiles saved");
            } else {
                ok(host, &format!("Profiles: {}", list.join(", ")));
            }
        }
        "save" if !name.is_empty() => {
            let saved = with(host, |s, _| {
                if s.profile_dir().is_none() {
                    return false;
                }
                s.save_profile(name);
                true
            });
            if saved {
                ok(host, &format!("Profile {name} saved"));
            } else {
                fail(host, "No config folder to save the profile to");
            }
        }
        "delete" if !name.is_empty() => {
            let known = with(host, |s, _| {
                let known = s.profiles().iter().any(|p| p == name);
                if known {
                    s.delete_profile(name);
                    s.save();
                }
                known
            });
            if known {
                ok(host, &format!("Profile {name} deleted"));
            } else {
                fail(host, &format!("Unknown profile: {name}"));
            }
        }
        "save" | "delete" => fail(host, &format!("Usage: /ot profile {sub} <name>")),
        "load" if !name.is_empty() => profile(host, name),
        _ => profile(host, unquote(rest)),
    }
}

/// `/ot trust`: the current server joins the Safe Mode whitelist.
fn trust(host: &mut dyn Host) {
    let ip = with(host, |s, host| {
        let ip = s.trust_server(host);
        if ip.is_some() {
            s.save();
        }
        ip
    });
    match ip {
        Some(ip) => ok(host, &format!("Trusted {ip} (Safe Mode whitelist)")),
        None => fail(host, "Not on a multiplayer server"),
    }
}

fn profile(host: &mut dyn Host, name: &str) {
    let known = with(host, |s, host| {
        if !s.profiles().iter().any(|p| p == name) {
            return false;
        }
        s.load_profile(host, name);
        true
    });
    if known {
        ok(host, &format!("Profile {name} loaded"));
    } else {
        fail(host, &format!("Unknown profile: {name}"));
    }
}

/// Where share codes go while the host has no clipboard.
const SHARE_FILE: &str = "share-code.txt";

fn export(host: &mut dyn Host, args: &str) {
    let (what, rest) = word(args);
    let data = with(host, |s, _| match what {
        "" => Ok(s.collect(false)),
        "workflows" => Ok(s.collect(true)),
        "profile" if !rest.trim().is_empty() => {
            let n = unquote(rest);
            s.collect_profile(n).ok_or(format!("Unknown profile: {n}"))
        }
        _ => Err("Usage: /ot export [workflows | profile <name>]".to_owned()),
    });
    let data = match data {
        Ok(d) => d,
        Err(e) => return fail(host, &e),
    };
    let code = share::encode(&Json::Obj(data));
    let n = code.chars().count();
    if host.set_clipboard(&code) {
        return ok(
            host,
            &format!("Share code copied to the clipboard ({n} characters)"),
        );
    }
    match host.data_dir().map(|d| d.join(SHARE_FILE)) {
        Some(file) if std::fs::write(&file, &code).is_ok() => ok(
            host,
            &format!(
                "No clipboard: share code saved to {} ({n} characters)",
                file.display()
            ),
        ),
        _ => fail(host, "No clipboard to copy the share code to"),
    }
}

fn import(host: &mut dyn Host, args: &str) {
    if args == "confirm" {
        let done = with(host, |s, _| match s.pending_import.take() {
            Some(data) => {
                s.apply(&data);
                true
            }
            None => false,
        });
        return if done {
            ok(host, "Settings imported")
        } else {
            fail(host, "Nothing to import - run /ot import first")
        };
    }
    let text = host
        .clipboard()
        .or_else(|| std::fs::read_to_string(host.data_dir()?.join(SHARE_FILE)).ok());
    let data = match share::decode(text.as_deref()) {
        Ok(d) => d,
        Err(e) => return fail(host, e),
    };
    let changes = with(host, |s, host| {
        let changes = s.preview(&data, s.german(host));
        if !changes.is_empty() {
            s.pending_import = Some(data);
        }
        changes
    });
    if changes.is_empty() {
        return ok(
            host,
            "The share code matches your settings - nothing to change",
        );
    }
    ok(host, &format!("{} changes:", changes.len()));
    for c in changes.iter().take(15) {
        host.chat(&format!("§7 {c}"));
    }
    if changes.len() > 15 {
        host.chat(&format!("§7 ... +{}", changes.len() - 15));
    }
    ok(host, "Type /ot import confirm to apply");
}

/// Copies a share code (ClickGui button); the notice text.
pub fn share_export(host: &mut dyn Host, with_workflows: bool) -> String {
    let data = with(host, |s, _| s.collect(with_workflows));
    let code = share::encode(&Json::Obj(data));
    let n = code.chars().count();
    if host.set_clipboard(&code) {
        format!("Share code copied to the clipboard ({n})")
    } else {
        "No clipboard available".to_owned()
    }
}

/// Decodes the clipboard share code and keeps it pending; the change list
/// (empty = nothing to change) or the error text.
pub fn share_import_preview(host: &mut dyn Host) -> Result<Vec<String>, String> {
    let text = host.clipboard();
    let data = share::decode(text.as_deref()).map_err(str::to_owned)?;
    Ok(with(host, |s, host| {
        let changes = s.preview(&data, s.german(host));
        s.pending_import = if changes.is_empty() { None } else { Some(data) };
        changes
    }))
}

/// Applies the pending import; false when there is none.
pub fn share_import_apply(host: &mut dyn Host) -> bool {
    with(host, |s, _| match s.pending_import.take() {
        Some(data) => {
            s.apply(&data);
            s.save();
            true
        }
        None => false,
    })
}

// ------------------------------------------------------------ /ot workflow

const WORKFLOW_USAGE: &str = "Usage: /ot workflow list | show <name> | new <name> | rename <name> <new name> | delete <name> | add <name> <step> | set <name> <n> <step> | del <name> <n> | clear <name> | steps | mode <name> press|hold | drop <name> <n> on|off | coords <name> <n> [<x> <y> <z>] | trigger <name> [<trigger> [arg]] | record <name>|stop | stop <name>";

/// A workflow name at the start of `s`: quoted (may hold spaces) or one
/// word; and the rest.
fn name_arg(s: &str) -> (&str, &str) {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix('"')
        && let Some(end) = inner.find('"')
    {
        return (&inner[..end], inner[end + 1..].trim());
    }
    word(s)
}

/// A step as typed, checked against the step catalog and its argument kind.
fn check_step(s: &State, raw: &str) -> Result<String, String> {
    let raw = workflows::jtrim(raw);
    let verb = workflows::verb(raw);
    let step =
        workflows::step_of(raw).ok_or(format!("Unknown step: {verb} (see /ot workflow steps)"))?;
    if !step.available(s.beta, CURSEFORGE) {
        return Err(format!("Step {verb} is not available in this build"));
    }
    let arg = workflows::arg(raw);
    match step.arg {
        workflows::Arg::Module if s.loaded_module(arg).is_none() => {
            Err(format!("Unknown module: {arg}"))
        }
        workflows::Arg::Workflow if by_name(&s.bindings, arg).is_none() => {
            Err(format!("Unknown workflow: {arg}"))
        }
        workflows::Arg::None if !arg.is_empty() => Err(format!("Step {verb} takes no argument")),
        _ => Ok(raw.to_owned()),
    }
}

/// `/ot workflow ...`: the workflow editor of Java's menu as commands.
fn workflow_command(host: &mut dyn Host, args: &str) {
    let (sub, rest) = word(args);
    let (name, tail) = name_arg(rest);
    let result: Result<Vec<String>, String> = match sub {
        "" => Err(WORKFLOW_USAGE.to_owned()),
        "list" => Ok(read(host, |s| {
            if s.bindings.is_empty() {
                return vec!["§7 No workflows".to_owned()];
            }
            s.bindings
                .iter()
                .map(|b| {
                    let trigger = if b.trigger == workflows::Trigger::None {
                        String::new()
                    } else {
                        format!(", {}", b.trigger.label())
                    };
                    format!(
                        "§7 {} ({} steps{trigger}{})",
                        b.name,
                        b.steps.len(),
                        if workflow_running(b.uid) {
                            ", running"
                        } else {
                            ""
                        }
                    )
                })
                .collect()
        })),
        "steps" => Ok(read(host, |s| {
            workflows::STEPS
                .iter()
                .filter(|st| st.available(s.beta, CURSEFORGE))
                .map(|st| {
                    let arg = match st.arg {
                        workflows::Arg::None => String::new(),
                        a => format!(":<{}>", a.name().to_lowercase()),
                    };
                    format!("§7 {}{arg}", st.id)
                })
                .collect()
        })),
        "new" => {
            let name = unquote(rest);
            with(host, |s, _| {
                if name.is_empty() {
                    return Err("Usage: /ot workflow new <name>".to_owned());
                }
                if by_name(&s.bindings, name).is_some() {
                    return Err(format!("Workflow {name} already exists"));
                }
                s.bindings.push(Binding::new(name));
                s.save();
                Ok(vec![format!("Workflow {name} created")])
            })
        }
        "show" | "rename" | "delete" | "add" | "set" | "del" | "clear" | "mode" | "trigger"
        | "record" | "stop" | "drop" | "coords" => workflow_edit(host, sub, name, tail, rest),
        _ => Err(WORKFLOW_USAGE.to_owned()),
    };
    match result {
        // lines starting with a colour code are shown as they are
        Ok(lines) => {
            for l in lines {
                if l.starts_with('§') {
                    host.chat(&l);
                } else {
                    ok(host, &l);
                }
            }
        }
        Err(e) => fail(host, &e),
    }
}

fn workflow_edit(
    host: &mut dyn Host,
    sub: &str,
    name: &str,
    tail: &str,
    rest: &str,
) -> Result<Vec<String>, String> {
    if sub == "record" && name == "stop" {
        stop_recording(host);
        return Ok(vec!["Recording stopped".to_owned()]);
    }
    // `delete`, `show`, `clear`, `record`, `stop` take the whole rest as name
    let whole = matches!(sub, "show" | "delete" | "clear" | "record" | "stop");
    let name = if whole { unquote(rest) } else { name };
    let usage = |u: &str| Err(format!("Usage: /ot workflow {u}"));
    let uid = read(host, |s| by_name(&s.bindings, name).map(|b| b.uid));
    let Some(uid) = uid else {
        return Err(if name.is_empty() {
            WORKFLOW_USAGE.to_owned()
        } else {
            format!("Unknown workflow: {name}")
        });
    };
    match sub {
        "record" => {
            start_recording(uid);
            return Ok(vec![format!(
                "Recording into {name} - finish with /ot workflow record stop"
            )]);
        }
        "stop" => {
            stop_workflow(host, uid);
            return Ok(vec![format!("Stopped workflow {name}")]);
        }
        _ => {}
    }
    with(host, |s, _| {
        let check = |s: &State, step: &str| check_step(s, step);
        let index_of = |s: &State| s.bindings.iter().position(|b| b.uid == uid).unwrap();
        let i = index_of(s);
        let out = match sub {
            "show" => {
                let b = &s.bindings[i];
                let mut lines = vec![format!(
                    "§6{}: §r{}, {}",
                    b.name,
                    if b.hold() { "hold" } else { "press" },
                    b.trigger.label()
                )];
                if !b.trigger_arg.is_empty() {
                    lines.push(format!("§7 trigger argument: {}", b.trigger_arg));
                }
                for (n, step) in b.steps.iter().enumerate() {
                    lines.push(format!("§7 {}. {step}", n + 1));
                }
                if b.steps.is_empty() {
                    lines.push("§7 no steps".to_owned());
                }
                return Ok(lines);
            }
            "rename" => {
                let new = unquote(tail);
                if new.is_empty() {
                    return usage("rename <name> <new name>");
                }
                if by_name(&s.bindings, new).is_some_and(|b| b.uid != uid) {
                    return Err(format!("Workflow {new} already exists"));
                }
                s.bindings[i].name = new.to_owned();
                format!("Workflow {name} renamed to {new}")
            }
            "delete" => {
                s.bindings.remove(i);
                format!("Workflow {name} deleted")
            }
            "clear" => {
                s.bindings[i].steps.clear();
                format!("Workflow {name} cleared")
            }
            "add" => {
                if tail.is_empty() {
                    return usage("add <name> <step>");
                }
                let step = check(s, tail)?;
                s.bindings[i].steps.push(step.clone());
                format!("{name}: step {} = {step}", s.bindings[i].steps.len())
            }
            "set" | "del" => {
                let (n, step) = word(tail);
                let len = s.bindings[i].steps.len();
                let Some(n) = n.parse::<usize>().ok().filter(|n| (1..=len).contains(n)) else {
                    return Err(format!("Step number 1-{len} needed"));
                };
                if sub == "del" {
                    s.bindings[i].steps.remove(n - 1);
                    format!("{name}: step {n} removed")
                } else {
                    let step = check(s, step)?;
                    s.bindings[i].steps[n - 1] = step.clone();
                    format!("{name}: step {n} = {step}")
                }
            }
            "drop" | "coords" => {
                let (n, value) = word(tail);
                let len = s.bindings[i].steps.len();
                let Some(n) = n.parse::<usize>().ok().filter(|n| (1..=len).contains(n)) else {
                    return Err(format!("Step number 1-{len} needed"));
                };
                let step = s.bindings[i].steps[n - 1].clone();
                let (verb, arg) = (workflows::verb(&step), workflows::arg(&step).to_owned());
                let allowed: &[&str] = if sub == "drop" {
                    &["craft", "trade"]
                } else {
                    &["trade", "villager"]
                };
                if !allowed.contains(&verb.as_str()) {
                    return Err(format!("Step {n} ({verb}) has no {sub} option"));
                }
                let new_arg = if sub == "drop" {
                    match value {
                        "on" | "off" => workflows::with_drop(&arg, value == "on"),
                        _ => return usage("drop <name> <n> on|off"),
                    }
                } else {
                    let rest = super::omnitool_trading::without_coords(&arg);
                    super::omnitool_trading::with_coords(&rest, value)
                };
                let new_step = format!("{verb}:{new_arg}");
                s.bindings[i].steps[n - 1] = new_step.clone();
                format!("{name}: step {n} = {new_step}")
            }
            "mode" => {
                let mode = match tail {
                    "press" => workflows::Mode::Press,
                    "hold" => workflows::Mode::Hold,
                    _ => return usage("mode <name> press|hold"),
                };
                s.bindings[i].mode = Some(mode);
                format!("{name}: {tail}")
            }
            _ => {
                // trigger
                let (t, arg) = word(tail);
                if t.is_empty() {
                    let all: Vec<String> = workflows::Trigger::ALL
                        .iter()
                        .map(|t| {
                            let hint = t.hint();
                            if hint.is_empty() {
                                format!("§7 {} - {}", t.name(), t.label())
                            } else {
                                format!("§7 {} - {} ({hint})", t.name(), t.label())
                            }
                        })
                        .collect();
                    return Ok(all);
                }
                let trigger = workflows::Trigger::by_name(&t.to_uppercase()).ok_or(format!(
                    "Unknown trigger: {t} (see /ot workflow trigger {name})"
                ))?;
                let b = &mut s.bindings[i];
                b.trigger = trigger;
                b.trigger_arg = if trigger.hint().is_empty() {
                    String::new()
                } else {
                    arg.to_owned()
                };
                format!("{name}: {}", trigger.label())
            }
        };
        s.save();
        Ok(vec![out])
    })
}

/// The buttons of Omnitool's page in Options > Mods: the ClickGui's
/// current screen (menu.rs).
pub fn settings_labels(host: &dyn Host) -> Vec<String> {
    super::omnitool_menu::labels(host)
}

/// Runs the module's action setting `setting` (a click on its button).
/// False when the module is not loaded or has no handler.
pub fn run_action(host: &mut dyn Host, id: &str, setting: &str) -> bool {
    let Some(m) = read(host, |s| s.loaded_module(id)) else {
        return false;
    };
    let Some(f) = m.on_action else { return false };
    guard(host, m, |h| f(h, setting));
    true
}

/// Button `button` of that page.
pub fn settings_click(host: &mut dyn Host, button: usize) {
    super::omnitool_menu::page_click(host, button);
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::omnitool_config::tests::temp_dir;

    /// A host in memory: its chat and action bar lines, player, folder,
    /// clipboard, language and server.
    #[derive(Default)]
    pub struct Fake {
        pub lines: Vec<String>,
        pub bar: Vec<String>,
        pub player: Option<Player>,
        pub dir: Option<PathBuf>,
        pub clip: Option<String>,
        pub language: Option<String>,
        /// None: singleplayer.
        pub server: Option<Server>,
        pub now: u64,
        pub worn: HudWorn,
        pub effects: Vec<HudEffect>,
        pub inventory: HudInventory,
        pub mappings: Vec<(String, bool, String)>,
        pub sounds: u32,
        pub world: Option<World>,
        /// Chat lines and commands sent to the server (commands with `/`).
        pub sent: Vec<String>,
        pub screen: bool,
        pub disconnects: u32,
        pub window: WindowState,
        pub fullbright: bool,
        pub fog_removed: bool,
        pub view: Option<View>,
        pub shift: bool,
        pub alt: bool,
        pub ctrl: bool,
        pub craft: Option<crate::omnitool_craft_calc::CraftData>,
        /// Panels shown: title and recipes.
        pub panels: Vec<(String, Vec<crate::omnitool_craft_calc::PanelRecipe>)>,
        pub placed: Vec<String>,
    }

    impl Host for Fake {
        fn craft_data(&mut self) -> Option<crate::omnitool_craft_calc::CraftData> {
            self.craft.clone()
        }
        fn craft_panel(
            &mut self,
            title: &str,
            recipes: &[crate::omnitool_craft_calc::PanelRecipe],
        ) {
            self.panels.push((title.to_owned(), recipes.to_vec()));
        }
        fn craft_place(&mut self, handle: &str) -> Result<(), String> {
            self.placed.push(handle.to_owned());
            Ok(())
        }
        fn shift_down(&self) -> bool {
            self.shift
        }
        fn alt_down(&self) -> bool {
            self.alt
        }
        fn ctrl_down(&self) -> bool {
            self.ctrl
        }
        fn view(&self) -> Option<View> {
            self.view
        }
        fn chat(&mut self, line: &str) {
            self.lines.push(line.to_owned());
        }
        fn player(&self) -> Option<Player> {
            self.player.clone()
        }
        fn data_dir(&self) -> Option<PathBuf> {
            self.dir.clone()
        }
        fn action_bar(&mut self, text: &str) {
            self.bar.push(text.to_owned());
        }
        fn clipboard(&mut self) -> Option<String> {
            self.clip.clone()
        }
        fn set_clipboard(&mut self, text: &str) -> bool {
            self.clip = Some(text.to_owned());
            true
        }
        fn language_code(&self) -> Option<String> {
            self.language.clone()
        }
        fn server(&self) -> Server {
            self.server.clone().unwrap_or(Server::Local)
        }
        fn now_ms(&self) -> u64 {
            self.now
        }
        fn worn(&self) -> HudWorn {
            self.worn.clone()
        }
        fn effects(&self) -> Vec<HudEffect> {
            self.effects.clone()
        }
        fn inventory(&self) -> HudInventory {
            self.inventory.clone()
        }
        fn mapping(&mut self, name: &str) -> Option<(bool, String)> {
            let m = self.mappings.iter().find(|m| m.0 == name)?;
            Some((m.1, m.2.clone()))
        }
        fn play_warning_sound(&mut self) {
            self.sounds += 1;
        }
        fn world(&self) -> Option<World> {
            self.world.clone()
        }
        fn screen_open(&self) -> bool {
            self.screen
        }
        fn window_state(&self) -> WindowState {
            self.window
        }
        fn disconnect(&mut self) {
            self.disconnects += 1;
        }
        fn set_fullbright(&mut self, on: bool) {
            self.fullbright = on;
        }
        fn set_distance_fog_removed(&mut self, removed: bool) {
            self.fog_removed = removed;
        }
        fn send_chat(&mut self, text: &str) {
            self.sent.push(text.to_owned());
        }
        fn send_command(&mut self, command: &str) {
            self.sent.push(format!("/{command}"));
        }
    }

    /// Forgets the state (as a restart does).
    pub fn restart() {
        STATE.with(|s| *s.borrow_mut() = None);
        RUNTIME.with(|r| *r.borrow_mut() = Runtime::default());
    }

    fn run(host: &mut Fake, line: &str) -> Vec<String> {
        host.lines.clear();
        assert!(command(host, line), "{line} not run");
        host.lines.clone()
    }

    fn with_dir() -> Fake {
        Fake {
            dir: Some(temp_dir()),
            ..Fake::default()
        }
    }

    fn calc() -> &'static ModuleInfo {
        module("calculator").unwrap()
    }

    #[test]
    fn ids_are_unique_and_lowercase() {
        for (i, m) in MODULES.iter().enumerate() {
            assert_eq!(m.id, m.id.to_lowercase());
            assert!(
                MODULES[i + 1..].iter().all(|o| o.id != m.id),
                "{} twice",
                m.id
            );
        }
    }

    #[test]
    fn help_and_bare_ot() {
        let mut h = Fake::default();
        let lines = run(&mut h, "ot help");
        assert_eq!(lines[0], "§6Omnitool");
        assert_eq!(
            lines[1],
            "§7 /ot list - all modules (green = on) and workflows"
        );
        assert_eq!(lines.len(), 1 + help_lines(true).len());
        assert_eq!(run(&mut h, "ot"), lines);
        assert_eq!(run(&mut h, "omnitool menu"), lines);
    }

    #[test]
    fn list_prints_every_category() {
        let mut h = Fake::default();
        let lines = run(&mut h, "ot list");
        assert_eq!(lines.len(), 6);
        assert_eq!(
            lines[0],
            "§6Render: §r§7fullbright§r, §7no_fog§r, §7clear_water§r, §7no_hurt_cam§r, §7no_pumpkin_overlay§r, §7low_fire§r, §7custom_crosshair§r, §7light_overlay§r, §7chunk_borders§r, §7block_outline§r"
        );
        assert_eq!(
            lines[5],
            "§6Misc: §r§acalculator§r, §adeath_coords§r, §aquick_disconnect§r, §7dynamic_fps§r, §7villager_trades§r, §7inventory_sort§r, §amouse_tweaks§r, §achat_tweaks§r, §achat_log§r, §atooltips§r, §ashulker_tooltip§r"
        );
        set_enabled(&mut h, calc(), false);
        assert_eq!(
            run(&mut h, "ot list")[5],
            "§6Misc: §r§7calculator§r, §adeath_coords§r, §aquick_disconnect§r, §7dynamic_fps§r, §7villager_trades§r, §7inventory_sort§r, §amouse_tweaks§r, §achat_tweaks§r, §achat_log§r, §atooltips§r, §ashulker_tooltip§r"
        );
    }

    #[test]
    fn toggle_on_off() {
        let mut h = Fake::default();
        assert_eq!(
            run(&mut h, "ot off Calculator"),
            ["§6[Omnitool] §fCalculator: OFF"]
        );
        assert!(!enabled(&h, calc()));
        assert_eq!(
            run(&mut h, "ot toggle calculator"),
            ["§6[Omnitool] §fCalculator: ON"]
        );
        assert_eq!(
            run(&mut h, "ot on calculator"),
            ["§6[Omnitool] §fCalculator: ON"]
        );
        assert_eq!(
            run(&mut h, "ot on nope"),
            ["§c[Omnitool] Unknown module: nope"]
        );
        assert_eq!(
            run(&mut h, "ot toggle"),
            ["§c[Omnitool] Usage: /ot toggle <module>"]
        );
        assert_eq!(
            run(&mut h, "ot fly"),
            ["§c[Omnitool] Unknown command: /ot fly - see /ot help"]
        );
    }

    #[test]
    fn settings_page_is_the_menu() {
        restart();
        let mut h = Fake::default();
        assert_eq!(settings_labels(&h)[0], "Safe Mode OFF (Auto)");
        settings_click(&mut h, 99);
        settings_click(&mut h, 2);
        assert_eq!(settings_labels(&h)[0], "< Back");
        crate::omnitool_menu::reset();
    }

    #[test]
    fn other_commands_pass_through() {
        let mut h = Fake::default();
        assert!(!command(&mut h, "gamemode creative"));
        assert!(!command(&mut h, "otx"));
        assert!(h.lines.is_empty());
    }

    fn in_world() -> Fake {
        Fake {
            dir: Some(temp_dir()),
            player: Some(Player {
                x: 0.0,
                y: 64.0,
                z: 0.0,
                yaw: 0.0,
                pitch: 0.0,
                dimension: "minecraft:overworld".into(),
            }),
            world: Some(World {
                health: 5.0,
                hunger: 20,
                y: 64.0,
                holding: None,
                inventory: Vec::new(),
                inventory_full: false,
                dimension: "minecraft:overworld".into(),
                server: Server::Remote("mc.example.com".into()),
                world_name: String::new(),
                sneaking: false,
                dead: false,
                day_time: 1000,
                screen_open: false,
            }),
            ..Fake::default()
        }
    }

    fn write_config(h: &Fake, json: &str) {
        std::fs::write(h.dir.as_ref().unwrap().join("config.json"), json).unwrap();
    }

    #[test]
    fn workflows_load_migrate_and_save() {
        let mut h = with_dir();
        write_config(
            &h,
            r#"{"bindings":[{"name":"","actions":"toggle:calculator, chat:hi"},
                {"name":"Hold","key":66,"mode":"HOLD","steps":["disable:calculator"],"trigger":"NONE","triggerArg":""}]}"#,
        );
        restart();
        let names = read(&h, |s| {
            s.bindings
                .iter()
                .map(|b| b.name.clone())
                .collect::<Vec<_>>()
        });
        assert_eq!(names, ["Workflow 1", "Hold"]);
        // like Java, the file keeps the old form until the workflows change
        with(&mut h, |s, _| {
            s.bindings[1].steps.push("chat:x".into());
            s.save();
        });
        let saved = config::read(&h.dir.clone().unwrap().join("config.json")).unwrap();
        let first = &saved.get("bindings").unwrap();
        let Json::Arr(list) = first else { panic!() };
        assert_eq!(
            list[0].compact(),
            r#"{"name":"Workflow 1","key":-1,"mode":"PRESS","steps":["toggle:calculator","chat:hi"],"trigger":"NONE","triggerArg":""}"#
        );
        // a broken workflow section: defaults and a copy aside
        write_config(&h, r#"{"bindings":[{"key":"b"}]}"#);
        restart();
        assert!(read(&h, |s| s.bindings.is_empty()));
    }

    #[test]
    fn ot_run_steps_and_conditions() {
        let mut h = in_world();
        write_config(
            &h,
            r#"{"bindings":[{"name":"Greet me","steps":["chat:hi","if:health < 10","command:/heal","else","chat:fine","end","wait:0.1","chat:later","toggle:calculator"]},
                {"name":"Other","steps":["chat:/notacommand","if:raining","end"]}]}"#,
        );
        restart();
        assert_eq!(
            run(&mut h, "ot run greet ME"),
            ["§6[Omnitool] §fRunning workflow Greet me"]
        );
        assert_eq!(h.sent, ["hi", "/heal"]);
        tick(&mut h);
        assert_eq!(h.sent.len(), 2);
        tick(&mut h);
        assert_eq!(h.sent, ["hi", "/heal", "later"]);
        assert!(!enabled(&h, calc()));
        assert_eq!(
            run(&mut h, "ot run \"other\""),
            ["§6[Omnitool] §fRunning workflow Other"]
        );
        assert_eq!(h.sent.last().unwrap(), "/notacommand");
        assert_eq!(
            h.bar.last().unwrap(),
            "Omnitool: unknown condition \"raining\""
        );
        assert_eq!(
            run(&mut h, "ot run nope"),
            ["§c[Omnitool] Unknown workflow: nope"]
        );
        assert_eq!(
            run(&mut h, "ot run"),
            ["§c[Omnitool] Usage: /ot run <workflow>"]
        );
        // /ot set shares Java's applySet checks
        with(&mut h, |s, _| {
            assert_eq!(s.apply_set("calculator.live=false"), None);
            assert_eq!(
                s.apply_set("calculator live"),
                Some("Format: module.setting=value".into())
            );
            assert_eq!(s.apply_set("nope.x=1"), Some("Unknown module nope".into()));
        });
        assert_eq!(value(&h, calc(), "live"), Value::Bool(false));
    }

    #[test]
    fn workflow_keys_hold_and_leaving_the_world() {
        let mut h = in_world();
        write_config(
            &h,
            r#"{"bindings":[{"name":"Hold","key":66,"mode":"HOLD","steps":["disable:calculator","wait:5","chat:x"]}]}"#,
        );
        restart();
        assert!(enabled(&h, calc()));
        key_event(&mut h, "key.keyboard.b", true, true);
        assert!(!enabled(&h, calc()));
        // a repeated press while held starts nothing new
        key_event(&mut h, "key.keyboard.b", true, true);
        assert!(runtime(|r| r
            .runners
            .running(read(&h, |s| s.bindings[0].uid))));
        key_event(&mut h, "key.keyboard.b", false, true);
        assert!(enabled(&h, calc()));
        // presses while a screen is open don't count
        key_event(&mut h, "key.keyboard.b", true, false);
        assert!(enabled(&h, calc()));
        h.player = None;
        tick(&mut h);
        assert!(runtime(|r| r.runners.is_empty()));
    }

    #[test]
    fn chat_trigger_runs_the_workflow() {
        let mut h = in_world();
        write_config(
            &h,
            r#"{"bindings":[{"name":"Reply","steps":["chat:pong"],"trigger":"CHAT","triggerArg":"Ping"}]}"#,
        );
        restart();
        tick(&mut h);
        chat_received("<Bob> ping!");
        tick(&mut h);
        assert_eq!(h.sent, ["pong"]);
    }

    #[test]
    fn recorder_appends_on_stop() {
        let mut h = in_world();
        h.mappings.push(("key.forward".into(), true, "W".into()));
        write_config(
            &h,
            r#"{"bindings":[{"name":"Rec","key":66,"steps":["chat:a"]}]}"#,
        );
        restart();
        let uid = read(&h, |s| s.bindings[0].uid);
        start_recording(uid);
        for _ in 0..4 {
            tick(&mut h);
        }
        assert!(h.bar[0].starts_with("● Recording \"Rec\" 0.05s - press"));
        // the workflow key ends the recording instead of running it
        key_event(&mut h, "key.keyboard.b", true, true);
        let steps = read(&h, |s| s.bindings[0].steps.clone());
        assert_eq!(steps, ["chat:a", "look:0.0 0.0", "move:forward 0.2s"]);
        assert_eq!(h.bar.last().unwrap(), "Omnitool: recorded steps: 2");
        assert!(h.sent.is_empty());
    }

    #[test]
    fn set_command() {
        let mut h = Fake::default();
        assert_eq!(
            run(&mut h, "ot set calculator live false"),
            ["§6[Omnitool] §fCalculator: live = false"]
        );
        assert_eq!(value(&h, calc(), "live"), Value::Bool(false));
        assert_eq!(
            run(&mut h, "ot set nope live x"),
            ["§c[Omnitool] Unknown module"]
        );
        assert_eq!(
            run(&mut h, "ot set calculator nope x"),
            ["§c[Omnitool] Unknown setting nope"]
        );
        assert_eq!(
            run(&mut h, "ot set calculator live"),
            ["§c[Omnitool] Usage: /ot set <module> <setting> <value>"]
        );
        // Java's quirk: "yes" is false; unchanged and not "false" -> invalid
        assert_eq!(
            run(&mut h, "ot set calculator live yes"),
            ["§c[Omnitool] Invalid value yes"]
        );
    }

    #[test]
    fn config_roundtrip_keeps_unknown_entries() {
        let mut h = with_dir();
        let dir = h.dir.clone().unwrap();
        let file = dir.join("config.json");
        std::fs::write(
            &file,
            r#"{"future": 1, "safe_mode": "ON", "language": "KLINGON", "gui_key": 82,
                "modules": {"calculator": {"enabled": false, "key": 70, "settings": {"live": false, "x": 2, "tab": "maybe"}},
                            "zoom": {"enabled": true}},
                "hud": {"gone": [1, 2]}}"#,
        )
        .unwrap();
        assert!(!enabled(&h, calc()));
        read(&h, |s| {
            assert_eq!(s.state(calc()).key, 70);
            assert_eq!(s.globals[G_SAFE_MODE], Value::Enum(1));
            assert_eq!(s.globals[G_GUI_KEY], Value::Key(82));
        });
        run(&mut h, "ot on calculator");
        let saved = config::parse(&file).unwrap();
        let o = |p: &str| -> Json {
            let mut cur = Json::Obj(saved.clone());
            for k in p.split('.') {
                cur = cur.as_obj().unwrap().get(k).unwrap().clone();
            }
            cur
        };
        assert_eq!(o("future"), Json::Int(1));
        // top-level keys: the current value wins (Java's keepUnknown), unlike module fields
        assert_eq!(o("language"), Json::str("AUTO"));
        assert_eq!(o("modules.calculator.enabled"), Json::Bool(true));
        assert_eq!(o("modules.calculator.key"), Json::Int(70));
        assert_eq!(o("modules.calculator.settings.x"), Json::Int(2));
        // an unchanged module setting keeps the file's value
        assert_eq!(o("modules.calculator.settings.tab"), Json::str("maybe"));
        assert_eq!(o("modules.zoom.enabled"), Json::Bool(true));
        assert_eq!(o("hud.gone"), Json::parse("[1,2]").unwrap());
        assert_eq!(o("safe_whitelist"), Json::str("localhost"));
        // a restart reads it back
        restart();
        assert!(enabled(&h, calc()));
        assert!(config::backup_of(&file).exists());
    }

    #[test]
    fn partly_broken_config_is_copied_aside() {
        let mut h = with_dir();
        let dir = h.dir.clone().unwrap();
        std::fs::write(
            dir.join("config.json"),
            r#"{"gui_key": "abc", "safe_mode": "OFF"}"#,
        )
        .unwrap();
        read(&h, |s| {
            assert_eq!(s.globals[G_GUI_KEY], Value::Key(344));
            assert_eq!(s.globals[G_SAFE_MODE], Value::Enum(0));
        });
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into())
            .collect();
        assert_eq!(
            names
                .iter()
                .filter(|n| n.starts_with("config.json.broken-"))
                .count(),
            1
        );
        // no change, no write; after one, the top-level key gets the current
        // value (as in Java)
        with(&mut h, |s, _| s.save());
        let saved = config::parse(&dir.join("config.json")).unwrap();
        assert_eq!(saved.get("gui_key"), Some(&Json::str("abc")));
        run(&mut h, "ot off calculator");
        let saved = config::parse(&dir.join("config.json")).unwrap();
        assert_eq!(saved.get("gui_key"), Some(&Json::Int(344)));
    }

    #[test]
    fn no_folder_saves_nothing() {
        let mut h = Fake::default();
        run(&mut h, "ot off calculator");
        restart();
        assert!(enabled(&h, calc()));
    }

    #[test]
    fn autosave_every_60_ticks_and_on_leaving_a_world() {
        let mut h = with_dir();
        let file = h.dir.clone().unwrap().join("config.json");
        for _ in 0..59 {
            tick(&mut h);
        }
        assert!(!file.exists());
        tick(&mut h);
        assert!(file.exists());
        std::fs::remove_file(&file).unwrap();
        h.player = Some(Player {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            dimension: "overworld".into(),
        });
        tick(&mut h);
        assert!(!file.exists());
        h.player = None;
        tick(&mut h);
        assert!(file.exists());
    }

    #[test]
    fn profiles() {
        let mut h = with_dir();
        with(&mut h, |s, _| {
            s.save_profile("  ");
            assert!(s.profiles().is_empty());
            s.state_mut(calc()).enabled = false;
            s.save_profile(" Mining ");
            assert_eq!(s.profile, "Mining");
            s.state_mut(calc()).enabled = true;
        });
        assert_eq!(
            run(&mut h, "ot profile \"Mining\""),
            ["§6[Omnitool] §fProfile Mining loaded"]
        );
        assert_eq!(h.bar, ["Omnitool: profile 'Mining' loaded"]);
        assert!(!enabled(&h, calc()));
        assert_eq!(
            run(&mut h, "ot profile PvP"),
            ["§c[Omnitool] Unknown profile: PvP"]
        );
        with(&mut h, |s, host| {
            assert!(!s.load_profile(host, "PvP"));
            s.delete_profile("Mining");
            assert_eq!(s.profile, "");
            assert!(s.profiles().is_empty());
        });
        assert_eq!(h.bar[1], "Omnitool: profile 'PvP' not found");
    }

    #[test]
    fn profile_commands_save_list_delete() {
        let mut h = with_dir();
        assert_eq!(run(&mut h, "ot profile list"), ["§7 No profiles saved"]);
        assert_eq!(
            run(&mut h, "ot profile save \"Farm Run\""),
            ["§6[Omnitool] §fProfile Farm Run saved"]
        );
        assert_eq!(read(&h, |s| s.profile.clone()), "Farm Run");
        assert_eq!(
            run(&mut h, "ot profile list"),
            ["§6[Omnitool] §fProfiles: Farm Run"]
        );
        assert_eq!(
            run(&mut h, "ot profile save"),
            ["§c[Omnitool] Usage: /ot profile save <name>"]
        );
        assert_eq!(
            run(&mut h, "ot profile load Farm Run"),
            ["§6[Omnitool] §fProfile Farm Run loaded"]
        );
        assert_eq!(
            run(&mut h, "ot profile delete nope"),
            ["§c[Omnitool] Unknown profile: nope"]
        );
        assert_eq!(
            run(&mut h, "ot profile delete \"Farm Run\""),
            ["§6[Omnitool] §fProfile Farm Run deleted"]
        );
        assert!(read(&h, |s| s.profiles().is_empty()));
    }

    #[test]
    fn workflow_commands_edit_workflows() {
        let mut h = with_dir();
        with(&mut h, |s, _| s.beta = true);
        let wf = |h: &mut Fake, args: &str| run(h, &format!("ot workflow {args}"));
        assert_eq!(wf(&mut h, "list"), ["§7 No workflows"]);
        assert_eq!(
            wf(&mut h, "new \"Farm Run\""),
            ["§6[Omnitool] §fWorkflow Farm Run created"]
        );
        assert_eq!(
            wf(&mut h, "new farm run"),
            ["§c[Omnitool] Workflow farm run already exists"]
        );
        assert_eq!(
            wf(&mut h, "add \"Farm Run\" toggle:calculator"),
            ["§6[Omnitool] §fFarm Run: step 1 = toggle:calculator"]
        );
        wf(&mut h, "add \"Farm Run\" chat:hello there");
        assert_eq!(
            wf(&mut h, "add \"Farm Run\" toggle:nothing"),
            ["§c[Omnitool] Unknown module: nothing"]
        );
        assert_eq!(
            wf(&mut h, "add \"Farm Run\" bogus:1"),
            ["§c[Omnitool] Unknown step: bogus (see /ot workflow steps)"]
        );
        assert_eq!(
            wf(&mut h, "add \"Farm Run\" sort:1"),
            ["§c[Omnitool] Step sort takes no argument"]
        );
        assert_eq!(
            wf(&mut h, "set \"Farm Run\" 2 chat:bye"),
            ["§6[Omnitool] §fFarm Run: step 2 = chat:bye"]
        );
        assert_eq!(
            wf(&mut h, "set \"Farm Run\" 9 chat:bye"),
            ["§c[Omnitool] Step number 1-2 needed"]
        );
        assert_eq!(
            wf(&mut h, "mode \"Farm Run\" hold"),
            ["§6[Omnitool] §fFarm Run: hold"]
        );
        assert_eq!(
            wf(&mut h, "trigger \"Farm Run\" interval 30"),
            ["§6[Omnitool] §fFarm Run: Every ... seconds"]
        );
        assert_eq!(
            wf(&mut h, "show Farm Run"),
            [
                "§6Farm Run: §rhold, Every ... seconds",
                "§7 trigger argument: 30",
                "§7 1. toggle:calculator",
                "§7 2. chat:bye"
            ]
        );
        assert_eq!(
            wf(&mut h, "list"),
            ["§7 Farm Run (2 steps, Every ... seconds)"]
        );
        // the options of craft / trade steps
        wf(&mut h, "add \"Farm Run\" craft:stick 16");
        assert_eq!(
            wf(&mut h, "drop \"Farm Run\" 3 on"),
            ["§6[Omnitool] §fFarm Run: step 3 = craft:stick 16 drop"]
        );
        assert_eq!(
            wf(&mut h, "drop \"Farm Run\" 1 on"),
            ["§c[Omnitool] Step 1 (toggle) has no drop option"]
        );
        wf(&mut h, "add \"Farm Run\" villager:librarian");
        assert_eq!(
            wf(&mut h, "coords \"Farm Run\" 4 1 64 -3"),
            ["§6[Omnitool] §fFarm Run: step 4 = villager:librarian at 1 64 -3"]
        );
        assert_eq!(
            wf(&mut h, "coords \"Farm Run\" 4"),
            ["§6[Omnitool] §fFarm Run: step 4 = villager:librarian"]
        );
        // persisted
        let saved = read(&h, |s| s.bindings[0].steps.clone());
        assert_eq!(saved.len(), 4);
        assert_eq!(
            wf(&mut h, "del \"Farm Run\" 1"),
            ["§6[Omnitool] §fFarm Run: step 1 removed"]
        );
        assert_eq!(
            wf(&mut h, "rename \"Farm Run\" Harvest"),
            ["§6[Omnitool] §fWorkflow Farm Run renamed to Harvest"]
        );
        assert_eq!(
            wf(&mut h, "clear Harvest"),
            ["§6[Omnitool] §fWorkflow Harvest cleared"]
        );
        // record and stop reach the runtime
        wf(&mut h, "record Harvest");
        assert!(runtime(|r| r.recorder.target.is_some()));
        wf(&mut h, "record stop");
        assert!(runtime(|r| r.recorder.target.is_none()));
        assert_eq!(
            wf(&mut h, "delete Harvest"),
            ["§6[Omnitool] §fWorkflow Harvest deleted"]
        );
        assert_eq!(
            wf(&mut h, "delete Harvest"),
            ["§c[Omnitool] Unknown workflow: Harvest"]
        );
        assert!(read(&h, |s| s.bindings.is_empty()));
        assert!(wf(&mut h, "steps").len() > 30);
    }

    #[test]
    fn trust_command() {
        let mut h = Fake {
            server: Some(Server::Remote("Play.Example.com".into())),
            ..Fake::default()
        };
        assert_eq!(
            run(&mut h, "ot trust"),
            ["§6[Omnitool] §fTrusted play.example.com (Safe Mode whitelist)"]
        );
        assert!(!read(&h, |s| s.safe_blocking(&h)));
        let mut single = Fake::default();
        assert_eq!(
            run(&mut single, "ot trust"),
            ["§c[Omnitool] Not on a multiplayer server"]
        );
    }

    #[test]
    fn safe_mode_commands_and_blocking() {
        let mut h = Fake {
            server: Some(Server::Remote("play.example.com".into())),
            ..Fake::default()
        };
        assert!(read(&h, |s| s.safe_blocking(&h)));
        assert_eq!(
            run(&mut h, "ot safemode off"),
            ["§6[Omnitool] §fSafe Mode OFF for this server"]
        );
        assert_eq!(h.bar, ["Omnitool Safe Mode: OFF"]);
        assert!(!read(&h, |s| s.safe_blocking(&h)));
        // already off: no toggle
        run(&mut h, "ot safemode off");
        assert_eq!(h.bar.len(), 1);
        run(&mut h, "ot safemode toggle");
        assert!(read(&h, |s| s.safe_blocking(&h)));
        // a cheaty module is blocked, stays enabled
        with(&mut h, |s, _| s.state_mut(calc()).safe_block = true);
        assert!(blocked(&h, calc()));
        assert!(!active(&h, calc()));
        assert_eq!(
            run(&mut h, "ot on calculator"),
            ["§6[Omnitool] §fCalculator: ON (blocked by Safe Mode)"]
        );
        with(&mut h, |s, _| s.state_mut(calc()).safe_ignore = true);
        assert!(!safe_mode(&h, calc()));
        assert!(active(&h, calc()));
        assert_eq!(
            run(&mut h, "ot safemode"),
            ["§c[Omnitool] Usage: /ot safemode on|off|toggle"]
        );
        // trusting the server
        assert_eq!(
            with(&mut h, |s, host| s.trust_server(host)),
            Some("play.example.com".into())
        );
        assert_eq!(
            read(&h, |s| s.globals[G_WHITELIST].clone()),
            Value::Text("localhost, play.example.com".into())
        );
    }

    #[test]
    fn language_auto_follows_the_game() {
        let mut h = Fake::default();
        assert_eq!(tr(&h, "Settings"), "Settings");
        h.language = Some("de_de".into());
        assert_eq!(tr(&h, "Settings"), "Einstellungen");
        with(&mut h, |s, _| s.globals[G_LANGUAGE] = Value::Enum(1));
        assert_eq!(tr(&h, "Settings"), "Settings");
    }

    #[test]
    fn export_and_import() {
        let mut h = with_dir();
        let out = run(&mut h, "ot export");
        let code = h.clip.clone().unwrap();
        assert_eq!(
            out,
            [format!(
                "§6[Omnitool] §fShare code copied to the clipboard ({} characters)",
                code.len()
            )]
        );
        assert_eq!(
            run(&mut h, "ot import"),
            ["§6[Omnitool] §fThe share code matches your settings - nothing to change"]
        );
        assert_eq!(
            run(&mut h, "ot import confirm"),
            ["§c[Omnitool] Nothing to import - run /ot import first"]
        );
        let data = share::decode(Some(&code)).unwrap();
        assert!(data.has("keys") && data.has("safe_mode") && !data.has("bindings"));
        // a friend's code: calculator off, another gui key, unknown module, workflow
        h.clip = Some(share::encode(
            &Json::parse(
                r#"{"v":1,"modules":{"calculator":{"enabled":false,"settings":{"live":false}},"nonexistent":{}},
                    "keys":{"gui_key":82},"safe_mode":"OFF","bindings":[{"name":"Login","steps":[]}]}"#,
            )
            .unwrap(),
        ));
        assert_eq!(
            run(&mut h, "ot import"),
            [
                "§6[Omnitool] §f6 changes:",
                "§7 Calculator: On true -> false",
                "§7 Calculator: Show result while typing in chat true -> false",
                "§7 Open Omnitool menu: Right Shift -> R",
                "§7 Safe Mode: AUTO -> OFF",
                "§7 New workflow: Login",
                "§7 Modules not available in this build (skipped): 1",
                "§6[Omnitool] §fType /ot import confirm to apply",
            ]
        );
        assert_eq!(
            run(&mut h, "ot import confirm"),
            ["§6[Omnitool] §fSettings imported"]
        );
        assert!(!enabled(&h, calc()));
        assert_eq!(read(&h, |s| s.globals[G_GUI_KEY].clone()), Value::Key(82));
        assert!(
            run(&mut h, "ot list")
                .last()
                .unwrap()
                .contains("Workflows: §rLogin")
        );
        run(&mut h, "ot export workflows");
        assert!(share::decode(h.clip.as_deref()).unwrap().has("bindings"));
        h.clip = Some("hello".into());
        assert_eq!(
            run(&mut h, "ot import"),
            ["§c[Omnitool] No Omnitool share code (must start with OMNI1:)"]
        );
        assert_eq!(
            run(&mut h, "ot export profile x"),
            ["§c[Omnitool] Unknown profile: x"]
        );
        with(&mut h, |s, _| s.save_profile("x"));
        run(&mut h, "ot export profile x");
        let p = share::decode(h.clip.as_deref()).unwrap();
        assert_eq!(p.get("profile"), Some(&Json::str("x")));
    }

    #[test]
    fn preview_cuts_long_values_and_translates() {
        assert_eq!(
            show(Some(&Json::str(&"a".repeat(30))), false),
            format!("{}...", "a".repeat(22))
        );
        assert_eq!(show(None, false), "-");
        assert_eq!(show(Some(&Json::Int(-1)), true), "-");
        assert_eq!(key_name(344), "Right Shift");
        assert_eq!(key_name(290), "F1");
        let h = Fake::default();
        let data = Json::parse(r#"{"profile":"PvP","hud":{"x":[1,2]}}"#).unwrap();
        let lines = read(&h, |s| s.preview(data.as_obj().unwrap(), true));
        assert_eq!(lines, ["Profil: PvP"]);
    }

    #[test]
    fn guard_isolates_a_panicking_module() {
        let mut h = Fake::default();
        std::panic::set_hook(Box::new(|_| {}));
        guard(&mut h, calc(), |_| panic!("boom"));
        let _ = std::panic::take_hook();
        assert!(!active(&h, calc()));
        assert!(enabled(&h, calc()));
        assert_eq!(h.bar, ["Omnitool: Calculator disabled (error, see log)"]);
        let mut ran = false;
        guard(&mut h, calc(), |_| ran = true);
        assert!(!ran);
    }

    #[test]
    fn edition_and_cheaty_modules() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/core-edition.json"
        );
        let fx: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let cheaty: Vec<&str> = fx["cheaty_modules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for m in MODULES {
            assert_eq!(m.cheaty, cheaty.contains(&m.id), "{}", m.id);
            // safe_block defaults to cheaty
            assert_eq!(ModState::new(m).safe_block, m.cheaty);
        }
        const { assert!(!CURSEFORGE) };
        assert!(beta_on(None));
    }
}
