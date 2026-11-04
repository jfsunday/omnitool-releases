// ClickGui "phase B" (docs/spec/omnitool/clickgui.md): Omnitool's own screen
// with the mouse. Immediate mode like Java's ClickGui: every frame draws the
// panel into a list of ops and rebuilds the click list (`hits`); input runs
// against the hits of the last frame. No game types: glue.rs feeds input
// (`push`), calls `frame` from `native_hud` and draws the ops. Added to the
// game as `game::omnitool_clickgui`; also a module of this crate.

use super::omnitool_clickgui_flow::{self as flow, Dropdown, Pick};
use super::omnitool_core::{self as core, Category, GLOBALS, Host, State};
use super::omnitool_hotbar_presets as hotbar;
use super::omnitool_menu as menu;
use super::omnitool_registry::MODULES;
use super::omnitool_safe::Server;
use super::omnitool_setting::{self as setting, Kind, NONE, SettingDef, Value};
use super::omnitool_trading as trading;
use super::omnitool_waypoints as wps;
use super::omnitool_workflows::{self as workflows, Arg, Mode, Trigger};
use std::cell::RefCell;

// ------------------------------------------------------------ colours (ClickGui.java:42-46)

pub const BG: u32 = 0xF010_1318;
pub const SIDEBAR: u32 = 0xFF16_1A21;
pub const ROW: u32 = 0xFF1E_232C;
pub const ROW_HOVER: u32 = 0xFF28_3040;
pub const ACCENT: u32 = 0xFF3F_A9F5;
pub const TEXT: u32 = 0xFFE6_E6E6;
pub const DIM: u32 = 0xFF8A_93A3;
pub const ON: u32 = 0xFF3F_CF6A;
pub const OFF: u32 = 0xFF4A_505C;
pub const WARN: u32 = 0xFFFF_A040;
pub const BAD: u32 = 0xFFFF_5555;
pub const STAR: u32 = 0xFFFF_C940;
pub const PANEL: u32 = 0xFF18_1C24;
pub const BTN: u32 = 0xFF2A_3140;
pub const BTN_HOVER: u32 = 0xFF34_405A;
pub const BORDER: u32 = 0xFF3A_4458;
pub const ROW_H: i32 = 14;

/// Pixel icons (ClickGui.java:49-54): `#` = one pixel.
const ICON_STAR: [&str; 7] = [
    "...#...", "..###..", "#######", ".#####.", "..###..", ".##.##.", "##...##",
];
const ICON_RIGHT: [&str; 5] = ["#...", "##..", "###.", "##..", "#..."];
const ICON_DOWN: [&str; 3] = ["#####", ".###.", "..#.."];
const ICON_X: [&str; 5] = ["#...#", ".#.#.", "..#..", ".#.#.", "#...#"];
const ICON_RESET: [&str; 6] = [".###.#", "#...##", "#..###", "#.....", "#....#", ".####."];

// ------------------------------------------------------------ ops and input

/// One drawing step in GUI pixels. Rects are `[x1, y1, x2, y2]`, x2/y2
/// exclusive. `Clip(Some)` starts a scissor, `Clip(None)` ends it.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Fill([i32; 4], u32),
    Text {
        text: String,
        x: i32,
        y: i32,
        argb: u32,
        shadow: bool,
    },
    Clip(Option<[i32; 4]>),
}

/// Input the screen takes, in the order it came.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    /// A mouse button went down (0 left, 1 right, 2 middle).
    Click(i32),
    Release(i32),
    /// Wheel, positive = up.
    Scroll(f64),
    /// A key went down (GLFW code), with Ctrl held or not.
    Key(i32, bool),
    /// A typed character.
    Char(char),
}

pub const KEY_ESCAPE: i32 = 256;
pub const KEY_ENTER: i32 = 257;
pub const KEY_BACKSPACE: i32 = 259;
pub const KEY_V: i32 = 86;

// ------------------------------------------------------------ pure layout (fixtures)

/// The panel and content bounds (spec 2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub px: i32,
    pub py: i32,
    pub pw: i32,
    pub ph: i32,
    pub left: i32,
    pub right: i32,
    pub top: i32,
    pub bottom: i32,
}

pub fn layout(w: i32, h: i32) -> Layout {
    let pw = (w - 16).min(480);
    let ph = h - 16;
    let px = (w - pw) / 2;
    let py = 8;
    Layout {
        px,
        py,
        pw,
        ph,
        left: px + 98,
        right: px + pw - 8,
        top: py + 22,
        bottom: py + ph - 16,
    }
}

/// Java's `fitText`: cut from the end until `text...` fits.
pub fn fit_text(width: &dyn Fn(&str) -> i32, s: &str, max: i32) -> String {
    if width(s) <= max {
        return s.to_owned();
    }
    let mut chars: Vec<char> = s.chars().collect();
    while chars.len() > 1 {
        let cut: String = chars.iter().collect();
        if width(&format!("{cut}...")) <= max {
            break;
        }
        chars.pop();
    }
    let cut: String = chars.iter().collect();
    format!("{cut}...")
}

/// Java's `paragraph` line breaking: greedy at spaces; a word longer than
/// the line stays on its own line.
pub fn wrap(width: &dyn Fn(&str) -> i32, text: &str, max: i32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && width(&candidate) > max {
            lines.push(std::mem::take(&mut line));
            line = word.to_owned();
        } else {
            line = candidate;
        }
    }
    lines.push(line);
    lines
}

/// The scroll bar (spec 2.6): `(max_scroll, bar_h, bar_y)`.
pub fn scrollbar(view_top: i32, bottom: i32, content: i32, scroll: i32) -> (i32, i32, i32) {
    let view = bottom - view_top;
    let max_scroll = (content - view).max(0);
    if max_scroll == 0 {
        return (0, 0, view_top);
    }
    let bar_h = (view * view / content).max(20);
    let s = i64::from(scroll.min(max_scroll));
    let bar_y = view_top + (i64::from(view - bar_h) * s / i64::from(max_scroll)) as i32;
    (max_scroll, bar_h, bar_y)
}

/// One frame of the scroll animation (spec 2.5); `dt` None = first frame.
pub fn scroll_step(anim: f64, target: f64, dt: Option<f64>) -> f64 {
    let dt = dt.map_or(1.0, |d| d.min(0.1));
    let next = anim + (target - anim) * (1.0 - (-dt * 18.0).exp());
    if (target - next).abs() < 0.5 {
        target
    } else {
        next
    }
}

/// Java's `Math.round` for the shown scroll.
pub fn round(x: f64) -> i32 {
    (x + 0.5).floor() as i32
}

/// The slider knob's x (spec 4.5).
pub fn slider_knob(x1: i32, x2: i32, fraction: f64) -> i32 {
    x1 + (f64::from(x2 - x1) * fraction.clamp(0.0, 1.0)) as i32
}

/// The slider's fraction at the mouse (not clamped; `setFraction` clamps).
pub fn slider_fraction(x1: i32, x2: i32, mouse_x: i32) -> f64 {
    f64::from(mouse_x - x1) / f64::from(x2 - x1)
}

/// Search (spec 4.1): None = no match; else whether name/id/description/
/// replaces matched and which settings (indices) matched by name.
pub fn search_match(
    q: &str,
    tr: &dyn Fn(&str) -> String,
    id: &str,
    name: &str,
    description: &str,
    replaces: &str,
    settings: &[&str],
) -> Option<(bool, Vec<usize>)> {
    let has = |text: &str| text.to_lowercase().contains(q) || tr(text).to_lowercase().contains(q);
    let name_match = has(name)
        || id.contains(q)
        || has(description)
        || (!replaces.is_empty() && replaces.to_lowercase().contains(q));
    let hits: Vec<usize> = settings
        .iter()
        .enumerate()
        .filter(|(_, s)| has(s))
        .map(|(i, _)| i)
        .collect();
    (name_match || !hits.is_empty()).then_some((name_match, hits))
}

// ------------------------------------------------------------ tabs

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Favourites,
    Category(Category),
    Replacements,
    Workflows,
    Profiles,
    Waypoints,
    Inventory,
    Settings,
}

impl Tab {
    pub fn title(self) -> &'static str {
        match self {
            Tab::Favourites => "Favourites",
            Tab::Category(c) => c.title(),
            Tab::Replacements => "Mod replacements",
            Tab::Workflows => "Workflows",
            Tab::Profiles => "Profiles",
            Tab::Waypoints => "Waypoints",
            Tab::Inventory => "Inventory",
            Tab::Settings => "Settings",
        }
    }

    pub fn is_module_tab(self) -> bool {
        matches!(self, Tab::Favourites | Tab::Category(_) | Tab::Replacements)
    }
}

/// The sidebar in order with the extra gap before each entry (spec 2.4).
pub fn sidebar(beta: bool) -> Vec<(Tab, i32)> {
    let mut out = vec![(Tab::Favourites, 0)];
    out.extend(Category::ALL.iter().map(|&c| (Tab::Category(c), 0)));
    out.push((Tab::Replacements, 3));
    out.push((Tab::Workflows, 6));
    out.push((Tab::Profiles, 0));
    out.push((Tab::Waypoints, 0));
    if beta {
        out.push((Tab::Inventory, 0));
    }
    out.push((Tab::Settings, 0));
    out
}

// ------------------------------------------------------------ state

/// What survives closing (Java's statics; not saved).
struct Keep {
    tab: Tab,
    last_module_tab: Tab,
    expanded: Vec<&'static str>,
    advanced: Vec<&'static str>,
    scrolls: Vec<(Tab, i32)>,
    /// The workflow open in the editor (-1 = none).
    open_wf: i32,
}

impl Default for Keep {
    fn default() -> Keep {
        Keep {
            tab: Tab::Category(Category::Render),
            last_module_tab: Tab::Category(Category::Render),
            expanded: Vec::new(),
            advanced: Vec::new(),
            scrolls: Vec::new(),
            open_wf: 0,
        }
    }
}

impl Keep {
    fn scroll(&self, tab: Tab) -> i32 {
        self.scrolls.iter().find(|s| s.0 == tab).map_or(0, |s| s.1)
    }
    fn set_scroll(&mut self, tab: Tab, v: i32) {
        match self.scrolls.iter_mut().find(|s| s.0 == tab) {
            Some(s) => s.1 = v,
            None => self.scrolls.push((tab, v)),
        }
    }
}

/// Java's `TextField`: one line, the cursor always at the end.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Field {
    pub text: String,
}

impl Field {
    /// `key`: Backspace and Ctrl+V; true when the text changed or the key
    /// belongs to the field.
    fn key(&mut self, host: &mut dyn Host, code: i32, ctrl: bool) -> bool {
        if code == KEY_BACKSPACE {
            self.text.pop();
            return true;
        }
        if ctrl && code == KEY_V {
            if let Some(clip) = host.clipboard() {
                self.text.push_str(&clip.replace('\n', " "));
            }
            return true;
        }
        false
    }
}

/// Whose setting a row edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Module(usize),
    Global,
}

/// One editable value: a module's or global setting (index into its
/// `settings`/`GLOBALS`), or one of a module's built-in four.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Own(Owner, usize),
    Key(usize),
    Hold(usize),
    SafeBlock(usize),
    SafeIgnore(usize),
}

impl Slot {
    fn def(self) -> SettingDef {
        match self {
            Slot::Own(Owner::Module(i), j) => MODULES[i].settings[j],
            Slot::Own(Owner::Global, j) => GLOBALS[j],
            Slot::Key(_) => core::KEY,
            Slot::Hold(_) => core::HOLD,
            Slot::SafeBlock(_) => core::SAFE_BLOCK,
            Slot::SafeIgnore(_) => core::SAFE_IGNORE,
        }
    }

    fn get(self, s: &State) -> Value {
        match self {
            Slot::Own(Owner::Module(i), j) => s.modules[i].values[j].clone(),
            Slot::Own(Owner::Global, j) => s.globals[j].clone(),
            Slot::Key(i) => Value::Key(s.modules[i].key),
            Slot::Hold(i) => Value::Bool(s.modules[i].hold),
            Slot::SafeBlock(i) => Value::Bool(s.modules[i].safe_block),
            Slot::SafeIgnore(i) => Value::Bool(s.modules[i].safe_ignore),
        }
    }

    /// Java's `set` (numbers snap to the step).
    fn put(self, s: &mut State, v: Value) {
        let def = self.def();
        match self {
            Slot::Own(Owner::Module(i), j) => def.set(&mut s.modules[i].values[j], v),
            Slot::Own(Owner::Global, j) => def.set(&mut s.globals[j], v),
            Slot::Key(i) => s.modules[i].key = v.key(),
            Slot::Hold(i) => s.modules[i].hold = v.on(),
            Slot::SafeBlock(i) => s.modules[i].safe_block = v.on(),
            Slot::SafeIgnore(i) => s.modules[i].safe_ignore = v.on(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FieldId {
    Search,
    Setting(Slot),
    ProfileName,
    Named(NameKey),
}

/// Text fields of the editor tabs (their text lives in `Gui::named`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NameKey {
    WfName(u64),
    WfArg(u64, usize),
    WfCoords(u64, usize),
    TrigArg(u64),
    WpAdd,
    WpName(usize),
    HotbarName,
}

/// What a click does.
#[derive(Clone, Debug, PartialEq)]
enum Action {
    SafeMode,
    Tab(Tab),
    ClearSearch,
    Focus(FieldId),
    /// Module header (index into MODULES).
    Header(usize),
    Toggle(usize),
    Favourite(usize),
    Expand(usize),
    Advanced(usize),
    /// Bool: switch; Enum: cycle; Key: listen; Color: open the channels.
    Setting(Slot),
    EnumBack(Slot),
    /// Number or colour channel slider between x1 and x2.
    Slider(Slot, i32, i32, Option<u32>),
    Reset(Slot),
    Run(Slot),
    ProfileSave,
    ProfileLoad(String),
    ProfileDelete(String),
    TrustServer,
    SaveConfig,
    OpenWf(i32),
    WfRun(u64),
    WfStop(u64),
    WfKey(u64),
    WfDelete(u64),
    WfNew,
    WfRecord(u64),
    StepMove(u64, usize, i32),
    StepRemove(u64, usize),
    DropToggle(u64, usize),
    Drop(Box<Dropdown>),
    WpAdd,
    WpDelete(usize),
    HotbarSave,
    HotbarLoad(String),
    HotbarDelete(String),
    ShareCopy(bool),
    ShareImport,
    ShareApply,
    ShareCancel,
}

/// A slider being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Drag {
    slot: Slot,
    x1: i32,
    x2: i32,
    channel: Option<u32>,
}

#[derive(Clone, Debug)]
struct Hit {
    rect: [i32; 4],
    clip: bool,
    action: Action,
}

/// The open screen (Java's ClickGui instance).
#[derive(Default)]
pub struct Gui {
    search: Field,
    focused: Option<FieldId>,
    anim: Vec<(Tab, f64)>,
    hits: Vec<Hit>,
    clip_top: i32,
    bottom: i32,
    last_frame: Option<u64>,
    inbox: Vec<Input>,
    close: bool,
    fields: Vec<(Slot, Field)>,
    open_colors: Vec<Slot>,
    drag: Option<Drag>,
    listening: Option<Slot>,
    profile_name: Field,
    mx: i32,
    named: Vec<(NameKey, Field)>,
    dropdown: Option<Dropdown>,
    listening_wf: Option<u64>,
    confirm_delete: Option<u64>,
    /// Share / hotbar status lines of the tabs.
    notice: String,
    hotbar_status: String,
    import_preview: Vec<String>,
    screen: (i32, i32),
}

thread_local! {
    static KEEP: RefCell<Keep> = RefCell::new(Keep::default());
    static OPEN: RefCell<Option<Gui>> = const { RefCell::new(None) };
    static REQUEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn keep<R>(f: impl FnOnce(&mut Keep) -> R) -> R {
    KEEP.with(|k| f(&mut k.borrow_mut()))
}

/// Asks to open the screen (gui_key, `/ot`); glue opens it when playing.
pub fn request_open() {
    REQUEST.set(true);
}

/// Takes the open request (once).
pub fn take_request() -> bool {
    REQUEST.replace(false)
}

pub fn open() {
    OPEN.with(|o| *o.borrow_mut() = Some(Gui::default()));
}

pub fn is_open() -> bool {
    OPEN.with(|o| o.borrow().is_some())
}

/// Closes without saving (glue saves through `close`).
pub fn close(host: &mut dyn Host) {
    if OPEN.with(|o| o.borrow_mut().take()).is_some() {
        // Java's onClose: Config.save()
        core::with(host, |s, _| s.save());
    }
}

/// Queues input for the next frame; false when the screen is closed (the
/// game keeps the event).
pub fn push(input: Input) -> bool {
    OPEN.with(|o| match o.borrow_mut().as_mut() {
        Some(g) => {
            g.inbox.push(input);
            true
        }
        None => false,
    })
}

/// What a frame produced.
pub struct Frame {
    pub ops: Vec<Op>,
    /// The screen asked to close (Esc): glue gives the cursor back.
    pub close: bool,
}

/// One frame: the queued input against the last frame's hits, then the
/// panel at `w x h` GUI pixels with the mouse at `(mx, my)`.
pub fn frame(host: &mut dyn Host, w: i32, h: i32, mx: i32, my: i32) -> Frame {
    let Some(mut gui) = OPEN.with(|o| o.borrow_mut().take()) else {
        return Frame {
            ops: Vec::new(),
            close: false,
        };
    };
    for input in std::mem::take(&mut gui.inbox) {
        gui.input(host, input, mx, my);
    }
    // Java's mouseDragged: the held slider follows the mouse
    if let Some(d) = gui.drag {
        drag_to(host, d, mx);
    }
    if gui.close {
        core::with(host, |s, _| s.save());
        return Frame {
            ops: Vec::new(),
            close: true,
        };
    }
    let ops = gui.render(host, w, h, mx, my);
    OPEN.with(|o| *o.borrow_mut() = Some(gui));
    Frame { ops, close: false }
}

// ------------------------------------------------------------ drawing context

struct Ctx<'a> {
    host: &'a dyn Host,
    german: bool,
    ops: Vec<Op>,
    hits: Vec<Hit>,
    mx: i32,
    my: i32,
    clipping: bool,
    hover: Option<String>,
    l: Layout,
}

impl Ctx<'_> {
    fn width(&self, s: &str) -> i32 {
        self.host.text_width(s).round() as i32
    }
    fn t(&self, s: &str) -> String {
        menu::t(self.german, s)
    }
    fn fill(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, argb: u32) {
        self.ops.push(Op::Fill([x1, y1, x2, y2], argb));
    }
    fn outline(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, argb: u32) {
        self.fill(x1, y1, x2, y1 + 1, argb);
        self.fill(x1, y2 - 1, x2, y2, argb);
        self.fill(x1, y1, x1 + 1, y2, argb);
        self.fill(x2 - 1, y1, x2, y2, argb);
    }
    fn text(&mut self, s: &str, x: i32, y: i32, argb: u32) {
        self.ops.push(Op::Text {
            text: s.to_owned(),
            x,
            y,
            argb,
            shadow: false,
        });
    }
    fn shadowed(&mut self, s: &str, x: i32, y: i32, argb: u32) {
        self.ops.push(Op::Text {
            text: s.to_owned(),
            x,
            y,
            argb,
            shadow: true,
        });
    }
    fn icon(&mut self, icon: &[&str], x: i32, y: i32, argb: u32) {
        for (r, row) in icon.iter().enumerate() {
            let bytes = row.as_bytes();
            let mut c = 0;
            while c < bytes.len() {
                if bytes[c] == b'#' {
                    let run = c;
                    while c < bytes.len() && bytes[c] == b'#' {
                        c += 1;
                    }
                    let y1 = y + r as i32;
                    self.fill(x + run as i32, y1, x + c as i32, y1 + 1, argb);
                } else {
                    c += 1;
                }
            }
        }
    }
    fn inside(&self, x1: i32, y1: i32, x2: i32, y2: i32) -> bool {
        self.mx >= x1 && self.mx < x2 && self.my >= y1 && self.my < y2
    }
    fn hit(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, action: Action) {
        let clip = self.clipping;
        self.hits.push(Hit {
            rect: [x1, y1, x2, y2],
            clip,
            action,
        });
    }
    fn fit(&self, s: &str, max: i32) -> String {
        fit_text(&|t: &str| self.width(t), s, max)
    }
    fn visible(&self, y: i32, h: i32, view_top: i32) -> bool {
        y + h > view_top && y < self.l.bottom
    }
    /// Java's `paragraph`: wrapped DIM text, 11 px a line.
    fn paragraph(&mut self, y: i32, view_top: i32, text: &str) -> i32 {
        let text = self.t(text);
        let max = self.l.right - self.l.left - 4;
        let lines = wrap(&|t: &str| self.width(t), &text, max);
        let mut y = y;
        for line in lines {
            if self.visible(y, 11, view_top) {
                let left = self.l.left;
                self.text(&line, left, y + 1, DIM);
            }
            y += 11;
        }
        y
    }
    /// Java's `button` (14 high); true when hovered.
    fn button(&mut self, x: i32, y: i32, w: i32, label: &str, action: Action) {
        let label = self.t(label);
        let label = self.fit(&label, w - 4);
        let hov = self.inside(x, y, x + w, y + 14);
        self.fill(x, y, x + w, y + 14, if hov { ACCENT } else { BORDER });
        self.fill(
            x + 1,
            y + 1,
            x + w - 1,
            y + 13,
            if hov { BTN_HOVER } else { BTN },
        );
        let tw = self.width(&label);
        self.text(&label, x + (w - tw) / 2, y + 3, TEXT);
        self.hit(x, y, x + w, y + 14, action);
    }
    /// Java's `slider` (spec 4.5).
    fn slider(&mut self, x1: i32, x2: i32, y: i32, fraction: f64, action: Action) {
        let ty = y + 6;
        self.fill(x1, ty, x2, ty + 2, 0xFF40_4652);
        let kx = slider_knob(x1, x2, fraction);
        self.fill(x1, ty, kx, ty + 2, ACCENT);
        let hov = self.inside(x1 - 3, y, x2 + 3, y + 14);
        self.fill(
            kx - 2,
            y + 3,
            kx + 2,
            y + 11,
            if hov { 0xFFFF_FFFF } else { 0xFFD0_D4DC },
        );
        self.hit(x1 - 3, y, x2 + 3, y + 14, action);
    }
    /// Java's `TextField.render` in `[x1, y1, x2, y2]`; `now` blinks the cursor.
    fn field(&mut self, field: &Field, r: [i32; 4], focused: bool, placeholder: &str, now: u64) {
        let [x1, y1, x2, y2] = r;
        self.fill(
            x1,
            y1,
            x2,
            y2,
            if focused { 0xFF50_5868 } else { 0xFF30_3540 },
        );
        self.fill(x1 + 1, y1 + 1, x2 - 1, y2 - 1, 0xFF15_181E);
        let ty = y1 + (y2 - y1 - 8) / 2;
        if field.text.is_empty() && !focused {
            let p = self.t(placeholder);
            self.text(&p, x1 + 3, ty, 0xFF70_7070);
            return;
        }
        let mut shown: Vec<char> = field.text.chars().collect();
        while !shown.is_empty() && self.width(&shown.iter().collect::<String>()) > x2 - x1 - 8 {
            shown.remove(0);
        }
        let mut s: String = shown.into_iter().collect();
        if focused && (now / 500).is_multiple_of(2) {
            s.push('_');
        }
        self.text(&s, x1 + 3, ty, 0xFFE0_E0E0);
    }
}

// ------------------------------------------------------------ rendering

impl Gui {
    fn render(&mut self, host: &mut dyn Host, w: i32, h: i32, mx: i32, my: i32) -> Vec<Op> {
        let now = host.now_ms();
        let dt = self
            .last_frame
            .map(|t| now.saturating_sub(t) as f64 / 1000.0);
        self.last_frame = Some(now);
        let (german, beta, safe_text, blocking) = core::read(&*host, |s| {
            let german = s.german(&*host);
            (
                german,
                s.beta,
                menu::safe_mode_label(s, &*host, german),
                s.safe_blocking(&*host),
            )
        });
        let l = layout(w, h);
        let mut c = Ctx {
            host: &*host,
            german,
            ops: Vec::new(),
            hits: Vec::new(),
            mx,
            my,
            clipping: false,
            hover: None,
            l,
        };
        let (px, py, pw, ph) = (l.px, l.py, l.pw, l.ph);
        c.fill(0, 0, w, h, 0x7000_0000);
        c.fill(px, py, px + pw, py + ph, BG);
        c.outline(px, py, px + pw, py + ph, 0xFF2A_3140);
        // header
        c.shadowed("Omnitool", px + 8, py + 6, ACCENT);
        let sx2 = px + pw - 6;
        let sx1 = sx2 - c.width(&safe_text) - 10;
        let hover = c.inside(sx1, py + 3, sx2, py + 16);
        let fill = match (blocking, hover) {
            (true, false) => 0xFF4A_3018,
            (true, true) => 0xFF6A_4520,
            (false, false) => 0xFF2A_3140,
            (false, true) => 0xFF34_405A,
        };
        c.fill(sx1, py + 3, sx2, py + 16, fill);
        c.text(
            &safe_text,
            sx1 + 5,
            py + 6,
            if blocking { WARN } else { DIM },
        );
        c.hit(sx1, py + 3, sx2, py + 16, Action::SafeMode);
        if hover {
            c.hover = Some(c.t(if blocking {
                "Click: turn Safe Mode off for this server until you leave it."
            } else {
                "Click: turn Safe Mode on for this server until you leave it."
            }));
        }
        // sidebar
        let tab = keep(|k| k.tab);
        c.fill(px + 1, py + 18, px + 92, py + ph - 1, SIDEBAR);
        let sx = px + 4;
        let mut sy = py + 22;
        for (t, gap) in sidebar(beta) {
            sy += gap;
            let active = t == tab;
            let hov = c.inside(sx, sy, sx + 86, sy + 14);
            if active || hov {
                c.fill(
                    sx,
                    sy,
                    sx + 86,
                    sy + 14,
                    if active { 0xFF24_3246 } else { 0xFF1E_2430 },
                );
            }
            if active {
                c.fill(sx, sy, sx + 2, sy + 14, ACCENT);
            }
            let label = c.t(t.title());
            c.text(&label, sx + 6, sy + 3, if active { TEXT } else { DIM });
            c.hit(sx, sy, sx + 86, sy + 14, Action::Tab(t));
            sy += 15;
        }
        // content
        let mut y = l.top;
        if tab.is_module_tab() {
            let focused = self.focused == Some(FieldId::Search);
            c.field(
                &self.search,
                [l.left, l.top, l.right, l.top + 14],
                focused,
                "Type to search modules and settings...",
                now,
            );
            c.hit(
                l.left,
                l.top,
                l.right,
                l.top + 14,
                Action::Focus(FieldId::Search),
            );
            if !self.search.text.is_empty() {
                let hov = c.inside(l.right - 12, l.top, l.right, l.top + 14);
                c.icon(
                    &ICON_X,
                    l.right - 9,
                    l.top + 3,
                    if hov { TEXT } else { DIM },
                );
                c.hit(
                    l.right - 12,
                    l.top,
                    l.right,
                    l.top + 14,
                    Action::ClearSearch,
                );
            }
            y = l.top + 18;
        }
        let view_top = y;
        let target = keep(|k| k.scroll(tab));
        let anim = match self.anim.iter_mut().find(|a| a.0 == tab) {
            Some(a) => {
                a.1 = scroll_step(a.1, f64::from(target), dt);
                a.1
            }
            None => {
                self.anim.push((tab, f64::from(target)));
                f64::from(target)
            }
        };
        let scroll = round(anim);
        c.ops
            .push(Op::Clip(Some([l.left, view_top, l.right, l.bottom])));
        c.clipping = true;
        let start = y - scroll;
        let end = match tab {
            Tab::Favourites | Tab::Category(_) | Tab::Replacements => {
                self.modules(&mut c, tab, start, view_top)
            }
            Tab::Settings => self.settings_tab(&mut c, start, view_top, now),
            Tab::Profiles => self.profiles_tab(&mut c, start, view_top, now),
            Tab::Workflows => self.workflows_tab(&mut c, start, view_top, now),
            Tab::Waypoints => self.waypoints_tab(&mut c, start, view_top, now),
            Tab::Inventory => self.inventory_tab(&mut c, start, view_top, now),
        };
        c.ops.push(Op::Clip(None));
        c.clipping = false;
        let content = end - start;
        let (max_scroll, bar_h, bar_y) = scrollbar(view_top, l.bottom, content, scroll);
        if target > max_scroll {
            keep(|k| k.set_scroll(tab, max_scroll));
        }
        if max_scroll > 0 {
            let hov = c.inside(l.left, view_top, l.right + 6, l.bottom);
            c.fill(l.right + 2, view_top, l.right + 4, l.bottom, 0x30FF_FFFF);
            c.fill(
                l.right + 2,
                bar_y,
                l.right + 4,
                bar_y + bar_h,
                if hov { 0xFF6A_7488 } else { 0xFF50_5868 },
            );
        }
        // footer
        let listening = self.listening.is_some() || self.listening_wf.is_some();
        let footer = if listening {
            Some(c.t("Press a key... (Esc = unbind)"))
        } else {
            c.hover.clone()
        };
        let footer = footer.unwrap_or_else(|| {
            c.t("Click = on/off, right-click or arrow = settings, star = radial menu. Just type to search.")
        });
        let footer = c.fit(&footer, pw - (l.left - px) - 8);
        c.text(
            &footer,
            l.left,
            py + ph - 12,
            if listening { WARN } else { DIM },
        );
        self.screen = (w, h);
        if let Some(d) = self.dropdown.clone() {
            draw_dropdown(&mut c, &d, w, h);
        }
        self.hits = c.hits;
        self.clip_top = view_top;
        self.bottom = l.bottom;
        c.ops
    }

    /// The module tabs (spec 4.1, 4.2).
    fn modules(&mut self, c: &mut Ctx<'_>, tab: Tab, mut y: i32, view_top: i32) -> i32 {
        let raw = self.search.text.trim().to_owned();
        let q = raw.to_lowercase();
        if q.is_empty() {
            let text = match tab {
                Tab::Replacements => c.t("These modules replace other mods. If you use the module, remove the named mod from your mods folder - both together can conflict."),
                Tab::Favourites => {
                    let key = core::read(c.host, |s| s.globals[core::G_RADIAL_KEY].key());
                    let name = if key == super::omnitool_setting::NONE {
                        c.t("none")
                    } else {
                        core::key_name(key)
                    };
                    format!(
                        "{} {} {name}",
                        c.t("Favourites appear in the radial menu: hold the radial key, point at a module and release. Click the star next to a module to add it."),
                        c.t("Key:")
                    )
                }
                _ => c.t("Click a module to switch it on/off (saved). Right-click or the arrow shows its settings. Keys are optional."),
            };
            y = c.paragraph(y, view_top, &text) + 4;
        }
        let german = c.german;
        let rows: Vec<(usize, bool, Vec<usize>)> = core::read(c.host, |s| {
            let tr = |t: &str| menu::t(german, t);
            MODULES
                .iter()
                .enumerate()
                .filter(|(_, m)| s.loaded(m))
                .filter_map(|(i, m)| {
                    if q.is_empty() {
                        let shown = match tab {
                            Tab::Replacements => !m.replaces.is_empty(),
                            Tab::Favourites => s.modules[i].favorite,
                            Tab::Category(cat) => m.category == cat,
                            _ => false,
                        };
                        return shown.then_some((i, true, Vec::new()));
                    }
                    let names: Vec<&str> = m.settings.iter().map(|d| d.name).collect();
                    search_match(&q, &tr, m.id, m.name, m.description, m.replaces, &names)
                        .map(|(name_match, hits)| (i, name_match, hits))
                })
                .collect()
        });
        if rows.is_empty() && !q.is_empty() {
            let text = format!("{} \"{raw}\"", c.t("Nothing found for"));
            return c.paragraph(y, view_top, &text);
        }
        let now = c.host.now_ms();
        for (i, name_match, hits) in rows {
            let m = &MODULES[i];
            let expanded = keep(|k| k.expanded.contains(&m.id));
            let only_settings = !name_match;
            if c.visible(y, 16, view_top) {
                self.header(c, i, y, expanded || only_settings);
            }
            y += 18;
            if only_settings {
                for j in hits {
                    y = self.setting_row(c, Slot::Own(Owner::Module(i), j), y, view_top, now);
                }
                y += 4;
            } else if expanded {
                for j in 0..m.settings.len() {
                    y = self.setting_row(c, Slot::Own(Owner::Module(i), j), y, view_top, now);
                }
                let advanced = keep(|k| k.advanced.contains(&m.id));
                if c.visible(y, ROW_H, view_top) {
                    self.advanced_row(c, i, y, advanced);
                }
                y += ROW_H;
                if advanced {
                    let bound = core::read(c.host, |s| s.modules[i].key != NONE);
                    if bound {
                        y = self.setting_row(c, Slot::Hold(i), y, view_top, now);
                    }
                    y = self.setting_row(c, Slot::SafeBlock(i), y, view_top, now);
                    if m.fair_variant {
                        y = self.setting_row(c, Slot::SafeIgnore(i), y, view_top, now);
                    }
                }
                y += 4;
            }
        }
        y
    }

    /// The `Advanced` row (spec 4.3).
    fn advanced_row(&mut self, c: &mut Ctx<'_>, i: usize, y: i32, open: bool) {
        let (left, right) = (c.l.left, c.l.right);
        c.fill(left, y, right, y + 14, PANEL);
        if c.inside(left, y, right, y + 14) {
            c.fill(left, y, right, y + 14, 0x18FF_FFFF);
        }
        if open {
            c.icon(&ICON_DOWN, left + 10, y + 6, DIM);
        } else {
            c.icon(&ICON_RIGHT, left + 11, y + 5, DIM);
        }
        let label = c.t("Advanced");
        c.text(&label, left + 19, y + 3, DIM);
        if !open {
            let m = &MODULES[i];
            let hold = core::read(c.host, |s| s.modules[i].hold);
            let info = if m.cheaty {
                c.t("blocked by Safe Mode")
            } else if hold {
                c.t("Hold")
            } else {
                String::new()
            };
            if !info.is_empty() {
                let x2 = right - 16;
                let w = c.width(&info);
                c.text(&info, x2 - w, y + 3, 0xFF6A_7282);
            }
        }
        c.hit(left, y, right, y + 14, Action::Advanced(i));
    }

    /// One setting row (spec 4.4); returns the next y.
    fn setting_row(&mut self, c: &mut Ctx<'_>, slot: Slot, y: i32, view_top: i32, now: u64) -> i32 {
        let def = slot.def();
        let (left, right) = (c.l.left, c.l.right);
        let (x1, x2) = (left + 10, right - 16);
        let value = core::read(c.host, |s| slot.get(s));
        let changed = !def.is_default(&value);
        let name = c.t(def.name);
        match def.kind {
            Kind::Text(_) => {
                let h = ROW_H * 2 + 2;
                if c.visible(y, h, view_top) {
                    c.fill(left, y, right, y + h, PANEL);
                    c.text(&name, x1, y + 3, DIM);
                    if !self.fields.iter().any(|f| f.0 == slot) {
                        let text = value.text().to_owned();
                        self.fields.push((slot, Field { text }));
                    }
                    let field = self
                        .fields
                        .iter()
                        .find(|f| f.0 == slot)
                        .map(|f| f.1.clone())
                        .unwrap_or_default();
                    let focused = self.focused == Some(FieldId::Setting(slot));
                    c.field(&field, [x1, y + 14, x2, y + 28], focused, "", now);
                    c.hit(
                        x1,
                        y + 14,
                        x2,
                        y + 28,
                        Action::Focus(FieldId::Setting(slot)),
                    );
                    if changed {
                        changed_marker(c, slot, y, h);
                    }
                }
                return y + h;
            }
            Kind::Action => {
                if c.visible(y, ROW_H, view_top) {
                    c.fill(left, y, right, y + ROW_H, PANEL);
                    c.button(x1, y, 160.min(x2 - x1), def.name, Action::Run(slot));
                }
                return y + ROW_H;
            }
            _ => {}
        }
        let open_color = matches!(def.kind, Kind::Color(_)) && self.open_colors.contains(&slot);
        let total = if open_color { 70 } else { ROW_H };
        if !c.visible(y, total, view_top) {
            return y + total;
        }
        c.fill(left, y, right, y + total, PANEL);
        let row_hover = c.inside(left, y, right, y + ROW_H);
        if row_hover {
            c.fill(left, y, right, y + ROW_H, 0x18FF_FFFF);
        }
        c.text(&name, x1, y + 3, TEXT);
        match (def.kind, &value) {
            (Kind::Bool(_), v) => {
                let bx = x2 - 18;
                let on = v.on();
                c.fill(bx, y + 3, bx + 18, y + 11, if on { ON } else { OFF });
                let k = if on { bx + 11 } else { bx + 1 };
                c.fill(k, y + 4, k + 6, y + 10, 0xFFF0_F0F0);
                c.hit(left, y, x2, y + 14, Action::Setting(slot));
            }
            (Kind::Number { min, max, .. }, v) => {
                let sx1 = (x1 + c.width(&name) + 8).max((x1 + x2) / 2);
                let sx2 = x2 - 34;
                let shown = def.display(v);
                let w = c.width(&shown);
                c.text(&shown, x2 - w, y + 3, ACCENT);
                let fraction = if max > min {
                    (v.num() - min) / (max - min)
                } else {
                    0.0
                };
                c.slider(sx1, sx2, y, fraction, Action::Slider(slot, sx1, sx2, None));
            }
            (Kind::Enum { .. }, v) => {
                let shown = c.t(&def.display(v));
                let vx = x2 - 8 - c.width(&shown);
                let back = c.inside(vx - 12, y, vx - 1, y + 14);
                let fwd = c.inside(x2 - 7, y, x2 + 1, y + 14);
                c.text("<", vx - 9, y + 3, if back { TEXT } else { DIM });
                c.text(&shown, vx, y + 3, ACCENT);
                c.text(">", x2 - 5, y + 3, if fwd { TEXT } else { DIM });
                c.hit(left, y, x2 + 1, y + 14, Action::Setting(slot));
                c.hit(vx - 12, y, vx - 1, y + 14, Action::EnumBack(slot));
            }
            (Kind::Key(_), v) => {
                let listening = self.listening == Some(slot);
                let code = v.key();
                let text = if listening {
                    c.t("Press a key...")
                } else if code != NONE {
                    core::key_name(code)
                } else {
                    c.t("+ Key")
                };
                let cx1 = x2 - c.width(&text) - 8;
                let hov = c.inside(cx1, y + 1, x2, y + 13);
                if listening || code != NONE {
                    c.fill(cx1, y + 1, x2, y + 13, if hov { BORDER } else { BTN });
                } else {
                    c.outline(cx1, y + 1, x2, y + 13, if hov { ACCENT } else { BORDER });
                }
                let color = if listening {
                    WARN
                } else if code != NONE {
                    TEXT
                } else {
                    DIM
                };
                c.text(&text, cx1 + 4, y + 3, color);
                c.hit(left, y, x2, y + 14, Action::Setting(slot));
            }
            (Kind::Color(_), Value::Color(argb)) => {
                let argb = *argb as u32;
                c.fill(x2 - 25, y + 1, x2 + 1, y + 13, BORDER);
                c.fill(x2 - 24, y + 2, x2 - 12, y + 12, 0xFF00_0000 | argb);
                c.fill(x2 - 12, y + 2, x2, y + 12, 0xFF00_0000);
                c.fill(x2 - 12, y + 2, x2, y + 12, argb);
                c.hit(left, y, x2, y + 14, Action::Setting(slot));
                if open_color {
                    for (n, (label, shift)) in
                        [("Red", 16), ("Green", 8), ("Blue", 0), ("Alpha", 24)]
                            .into_iter()
                            .enumerate()
                    {
                        let ry = y + 14 * (n as i32 + 1);
                        let label = c.t(label);
                        c.text(&label, x1 + 8, ry + 3, DIM);
                        let ch = setting::channel(argb as i32, shift);
                        c.slider(
                            x1 + 50,
                            x2 - 30,
                            ry,
                            f64::from(ch) / 255.0,
                            Action::Slider(slot, x1 + 50, x2 - 30, Some(shift)),
                        );
                        let num = ch.to_string();
                        let w = c.width(&num);
                        c.text(&num, x2 - w, ry + 3, ACCENT);
                    }
                }
            }
            _ => {}
        }
        if changed {
            changed_marker(c, slot, y, total);
        }
        y + total
    }

    /// One module header (spec 4.2), 16 px high on an 18 px grid.
    fn header(&mut self, c: &mut Ctx<'_>, i: usize, y: i32, open: bool) {
        let m = &MODULES[i];
        let (left, right) = (c.l.left, c.l.right);
        let y2 = y + 16;
        let (enabled, favorite, failure, blocked, key, hold) = core::read(c.host, |s: &State| {
            let st = &s.modules[i];
            (
                st.enabled,
                st.favorite,
                st.failure.clone(),
                s.blocked(c.host, m),
                st.key,
                st.hold,
            )
        });
        let bad = failure.is_some() || blocked;
        let row_hover = c.inside(left, y, right, y2);
        c.fill(left, y, right, y2, if row_hover { ROW_HOVER } else { ROW });
        if enabled {
            c.fill(left, y, left + 2, y2, if bad { BAD } else { ON });
        }
        let star_hover = c.inside(left + 3, y, left + 16, y2);
        let star = if favorite {
            STAR
        } else if star_hover {
            0xFF9A_A3B3
        } else if row_hover {
            0xFF5A_6272
        } else {
            0xFF3E_4452
        };
        c.icon(&ICON_STAR, left + 6, y + 4, star);
        let name = c.t(m.name);
        c.text(
            &name,
            left + 19,
            y + 5,
            if enabled { TEXT } else { 0xFFB8_BEC8 },
        );
        // switch
        let sx = right - 32;
        c.fill(sx, y + 3, sx + 28, y + 14, if enabled { ON } else { OFF });
        let k = if enabled { sx + 17 } else { sx + 1 };
        c.fill(k, y + 4, k + 10, y + 13, 0xFFF0_F0F0);
        // arrow
        let ax = right - 45;
        let arrow_hover = c.inside(ax - 3, y, right - 34, y2);
        let ac = if arrow_hover { TEXT } else { DIM };
        if open {
            c.icon(&ICON_DOWN, ax, y + 7, ac);
        } else {
            c.icon(&ICON_RIGHT, ax + 1, y + 5, ac);
        }
        // key chip
        let chip_x2 = ax - 6;
        let listening = self.listening == Some(Slot::Key(i));
        let chip = if listening {
            Some(c.t("Press a key..."))
        } else if key != NONE {
            let prefix = if hold {
                format!("{} ", c.t("Hold"))
            } else {
                String::new()
            };
            Some(prefix + &core::key_name(key))
        } else if row_hover {
            Some(c.t("+ Key"))
        } else {
            None
        };
        let mut chip_x1 = chip_x2;
        let mut chip_hover = false;
        if let Some(text) = &chip {
            chip_x1 = chip_x2 - c.width(text) - 8;
            chip_hover = c.inside(chip_x1, y + 2, chip_x2, y2 - 1);
            if listening || key != NONE {
                c.fill(
                    chip_x1,
                    y + 2,
                    chip_x2,
                    y2 - 1,
                    if chip_hover { BORDER } else { BTN },
                );
                c.text(
                    text,
                    chip_x1 + 4,
                    y + 5,
                    if listening { WARN } else { TEXT },
                );
            } else {
                c.outline(
                    chip_x1,
                    y + 2,
                    chip_x2,
                    y2 - 1,
                    if chip_hover { ACCENT } else { BORDER },
                );
                c.text(text, chip_x1 + 4, y + 5, DIM);
            }
        }
        // badge between the name and the chip
        let tx = left + 19 + c.width(&name) + 6;
        let badge = if failure.is_some() {
            Some((c.t("error - disabled"), BAD))
        } else if enabled && blocked {
            Some((c.t("blocked by Safe Mode"), BAD))
        } else if !m.replaces.is_empty() {
            Some((format!("{} {}", c.t("replaces"), m.replaces), 0xFF7F_A7C9))
        } else {
            None
        };
        let mut badge_hover = false;
        if let Some((text, color)) = &badge
            && chip_x1 - 4 - tx > 20
        {
            let shown = c.fit(text, chip_x1 - 4 - tx);
            let w = c.width(&shown);
            c.text(&shown, tx, y + 5, *color);
            badge_hover = *color != BAD && c.inside(tx, y, tx + w, y2);
        }
        if row_hover && c.hover.is_none() {
            c.hover = Some(match &failure {
                Some(f) => format!("{} {f}", c.t("Error:")),
                None => {
                    let tip = c.t(m.description);
                    if m.cheaty {
                        format!("{tip}  {}", c.t("(blocked when Safe Mode is active)"))
                    } else {
                        tip
                    }
                }
            });
        }
        if badge_hover {
            c.hover = Some(format!(
                "{} {} {}",
                c.t("Replaces"),
                m.replaces,
                "- remove that mod when you use this module."
            ));
        }
        if chip_hover {
            c.hover = Some(c.t(
                "Click: set a shortcut key. Right-click: remove it. Hold mode is under Advanced.",
            ));
        }
        if star_hover {
            c.hover = Some(c.t(if favorite {
                "Remove from favourites (radial menu)"
            } else {
                "Add to favourites (radial menu)"
            }));
        }
        if arrow_hover {
            c.hover = Some(c.t(if open {
                "Hide settings"
            } else {
                "Show settings"
            }));
        }
        c.hit(left, y, right, y2, Action::Header(i));
        c.hit(left + 3, y, left + 16, y2, Action::Favourite(i));
        c.hit(ax - 3, y, right - 34, y2, Action::Expand(i));
        if chip.is_some() {
            c.hit(
                chip_x1,
                y + 2,
                chip_x2,
                y2 - 1,
                Action::Setting(Slot::Key(i)),
            );
        }
        c.hit(right - 34, y, right, y2, Action::Toggle(i));
    }

    /// The Settings tab (spec 4.10, without the updater status, HUD editor,
    /// Omni Overlay button and share codes: not ported yet).
    fn settings_tab(&mut self, c: &mut Ctx<'_>, mut y: i32, view_top: i32, now: u64) -> i32 {
        y = c.paragraph(y, view_top, "Safe Mode blocks every module where \"Blocked by Safe Mode\" is checked (by default: Autoclicker, Auto Tool). You can change that per module. Click the Safe Mode badge at the top (or use the key below) to switch it on/off for the current server without editing the whitelist.");
        y = c.paragraph(
            y,
            view_top,
            "Auto = blocked on multiplayer servers that are not in the whitelist.",
        ) + 2;
        let g = |j| Slot::Own(Owner::Global, j);
        for j in [
            core::G_LANGUAGE,
            core::G_SAFE_MODE,
            core::G_WHITELIST,
            core::G_SAFE_KEY,
        ] {
            y = self.setting_row(c, g(j), y, view_top, now);
        }
        if matches!(c.host.server(), Server::Remote(_)) {
            if c.visible(y, 18, view_top) {
                let left = c.l.left;
                c.button(
                    left + 10,
                    y + 1,
                    160,
                    "Trust this server (whitelist)",
                    Action::TrustServer,
                );
            }
            y += 18;
        }
        for j in [core::G_GUI_KEY, core::G_RADIAL_KEY] {
            y = self.setting_row(c, g(j), y, view_top, now);
        }
        y = c.paragraph(y, view_top, "Radial menu: hold the key, point at a favourite module and release to switch it. Tap the key to keep the wheel open and click instead.") + 6;
        y = c.paragraph(y, view_top, "Updates: AUTO downloads new versions in the background, NOTIFY/OFF only when you press the button. The new version is active after the next restart. Channel BETA = newest test builds plus beta features (e.g. new modules, workflow macro steps); switching back to STABLE installs the stable build again. Beta features appear or disappear after a restart.");
        for (j, d) in GLOBALS.iter().enumerate() {
            if d.id == "update_mode" || d.id == "update_channel" {
                y = self.setting_row(c, g(j), y, view_top, now);
            }
        }
        y += 6;
        if c.visible(y, 14, view_top) {
            let left = c.l.left;
            c.button(left + 126, y, 100, "Save config", Action::SaveConfig);
        }
        y += 20;
        if core::read(c.host, |s| s.beta) {
            y = self.share_block(c, y, view_top);
        }
        y
    }

    /// The Profiles tab (spec 4.7, without the beta Share button).
    fn profiles_tab(&mut self, c: &mut Ctx<'_>, mut y: i32, view_top: i32, now: u64) -> i32 {
        y = c.paragraph(y, view_top, "A profile stores all module states and settings. Load it here, with a workflow step or with /ot profile <name>.");
        let (current, list) = core::read(c.host, |s| (s.profile.clone(), s.profiles()));
        let name = if current.is_empty() {
            c.t("none")
        } else {
            current.clone()
        };
        let line = format!("{} {name}", c.t("Current profile:"));
        y = c.paragraph(y, view_top, &line) + 4;
        let (left, right) = (c.l.left, c.l.right);
        if c.visible(y, 14, view_top) {
            let focused = self.focused == Some(FieldId::ProfileName);
            let field = self.profile_name.clone();
            c.field(
                &field,
                [left, y, right - 110, y + 14],
                focused,
                "Profile name",
                now,
            );
            c.hit(
                left,
                y,
                right - 110,
                y + 14,
                Action::Focus(FieldId::ProfileName),
            );
            c.button(right - 106, y, 106, "Save current", Action::ProfileSave);
        }
        y += 22;
        for p in list {
            if c.visible(y, 15, view_top) {
                c.fill(left, y, right, y + 15, ROW);
                c.text(
                    &p,
                    left + 4,
                    y + 4,
                    if p == current { ACCENT } else { TEXT },
                );
                c.button(right - 110, y, 50, "Load", Action::ProfileLoad(p.clone()));
                c.button(
                    right - 56,
                    y,
                    56,
                    "Delete",
                    Action::ProfileDelete(p.clone()),
                );
            }
            y += 17;
        }
        y
    }

    // -------------------------------------------------------- input

    fn input(&mut self, host: &mut dyn Host, input: Input, mx: i32, my: i32) {
        self.mx = mx;
        match input {
            Input::Click(button) => {
                self.click(host, mx, my, button);
            }
            Input::Release(button) => {
                // Java's mouseReleased: any button ends the drag
                if button >= 0 {
                    self.drag = None;
                }
            }
            Input::Scroll(v) if self.dropdown.is_some() => {
                let h = self.screen.1;
                if let Some(d) = self.dropdown.as_mut() {
                    d.wheel(v, h);
                }
            }
            Input::Scroll(v) => {
                let tab = keep(|k| k.tab);
                keep(|k| {
                    let s = k.scroll(tab);
                    k.set_scroll(tab, (s - (v * 24.0) as i32).max(0));
                });
            }
            Input::Key(code, ctrl) => {
                if !self.key(host, code, ctrl) && code == KEY_ESCAPE {
                    self.close = true;
                }
            }
            Input::Char(ch) => self.char_typed(host, ch),
        }
    }

    /// Java's `mouseClicked`: the last registered hit wins.
    fn click(&mut self, host: &mut dyn Host, mx: i32, my: i32, button: i32) -> bool {
        if self.listening.take().is_some() {
            return true;
        }
        if let Some(uid) = self.listening_wf.take() {
            // the key button swallows the click: right = unbind
            let _ = uid;
            return true;
        }
        if let Some(d) = self.dropdown.take() {
            // spec 2.8: the click is consumed, header rows keep it open
            match d.index_at(mx, my, self.screen.1) {
                Some(None) => self.dropdown = Some(d),
                Some(Some(i)) => self.pick(host, &d, i),
                None => {}
            }
            return true;
        }
        self.focused = None;
        let hit = self.hits.iter().rev().find(|h| {
            let [x1, y1, x2, y2] = h.rect;
            let inside = mx >= x1 && mx < x2 && my >= y1 && my < y2;
            inside && !(h.clip && (my < self.clip_top || my >= self.bottom))
        });
        let Some(action) = hit.map(|h| h.action.clone()) else {
            return false;
        };
        self.act(host, action, button);
        true
    }

    fn act(&mut self, host: &mut dyn Host, action: Action, button: i32) {
        match action {
            Action::SafeMode => {
                core::with(host, |s, h| {
                    s.toggle_safe_mode(h);
                });
            }
            Action::Tab(t) => {
                keep(|k| {
                    k.tab = t;
                    if t.is_module_tab() {
                        k.last_module_tab = t;
                    }
                });
            }
            Action::ClearSearch => self.set_search(String::new()),
            Action::Focus(f) => {
                self.focused = Some(f);
                if button == 1 && f == FieldId::Search {
                    // Java's type(""): onChange without changing the text
                    let text = self.search.text.clone();
                    self.set_search(text);
                }
            }
            Action::Header(i) if button == 1 => toggle_expanded(i),
            Action::Header(i) | Action::Toggle(i) => {
                core::with(host, |s, _| {
                    let st = &mut s.modules[i];
                    st.enabled = !st.enabled;
                });
            }
            Action::Favourite(i) => {
                core::with(host, |s, _| {
                    let st = &mut s.modules[i];
                    st.favorite = !st.favorite;
                });
            }
            Action::Expand(i) => toggle_expanded(i),
            Action::Advanced(i) => {
                let id = MODULES[i].id;
                keep(|k| toggle(&mut k.advanced, id));
            }
            Action::Setting(slot) => {
                let def = slot.def();
                match def.kind {
                    Kind::Key(_) if button == 1 => {
                        core::with(host, |s, _| slot.put(s, Value::Key(NONE)));
                    }
                    Kind::Key(_) => self.listening = Some(slot),
                    Kind::Color(_) => toggle(&mut self.open_colors, slot),
                    Kind::Enum { values, .. } => {
                        let dir = if button == 1 { -1 } else { 1 };
                        core::with(host, |s, _| {
                            let i = slot.get(s).index();
                            slot.put(s, Value::Enum(setting::cycle(i, values.len(), dir)));
                        });
                    }
                    Kind::Bool(_) => {
                        core::with(host, |s, _| {
                            let on = slot.get(s).on();
                            slot.put(s, Value::Bool(!on));
                        });
                    }
                    _ => {}
                }
            }
            Action::EnumBack(slot) => {
                if let Kind::Enum { values, .. } = slot.def().kind {
                    core::with(host, |s, _| {
                        let i = slot.get(s).index();
                        slot.put(s, Value::Enum(setting::cycle(i, values.len(), -1)));
                    });
                }
            }
            Action::Slider(slot, x1, x2, channel) => {
                let d = Drag {
                    slot,
                    x1,
                    x2,
                    channel,
                };
                self.drag = Some(d);
                drag_to(host, d, self.mx);
            }
            Action::Reset(slot) => {
                let def = slot.def();
                core::with(host, |s, _| slot.put(s, def.default_value()));
                self.fields.retain(|f| f.0 != slot);
            }
            Action::Run(slot) => {
                if let Slot::Own(Owner::Module(i), j) = slot {
                    let m = &MODULES[i];
                    if let Some(f) = m.on_action {
                        let id = m.settings[j].id;
                        core::guard(host, m, |h| f(h, id));
                    }
                }
            }
            Action::ProfileSave => {
                let name = self.profile_name.text.trim().to_owned();
                if !name.is_empty() {
                    core::with(host, |s, _| s.save_profile(&name));
                }
                self.profile_name.text.clear();
            }
            Action::ProfileLoad(name) => {
                core::with(host, |s, h| s.load_profile(h, &name));
                self.fields.clear();
            }
            Action::ProfileDelete(name) => core::with(host, |s, _| s.delete_profile(&name)),
            Action::TrustServer => {
                core::with(host, |s, h| {
                    s.trust_server(h);
                    s.save();
                });
                self.fields
                    .retain(|f| f.0 != Slot::Own(Owner::Global, core::G_WHITELIST));
            }
            Action::SaveConfig => core::with(host, |s, _| s.save()),
            other => self.act_tabs(host, other, button),
        }
    }

    fn set_search(&mut self, text: String) {
        self.search.text = text;
        keep(|k| {
            let tab = k.tab;
            k.set_scroll(tab, 0);
        });
    }

    /// Java's `keyPressed`; false = not handled (Esc then closes).
    fn key(&mut self, host: &mut dyn Host, code: i32, ctrl: bool) -> bool {
        if let Some(slot) = self.listening.take() {
            let code = if code == KEY_ESCAPE { NONE } else { code };
            core::with(host, |s, _| slot.put(s, Value::Key(code)));
            return true;
        }
        if let Some(uid) = self.listening_wf.take() {
            let code = if code == KEY_ESCAPE { NONE } else { code };
            core::with(host, |s, _| {
                if let Some(b) = s.bindings.iter_mut().find(|b| b.uid == uid) {
                    b.key = code;
                }
            });
            return true;
        }
        if self.focused.is_none() && code == KEY_ESCAPE && self.dropdown.take().is_some() {
            return true;
        }
        if let Some(f) = self.focused {
            match f {
                FieldId::Named(k) => {
                    if code == KEY_ESCAPE || code == KEY_ENTER {
                        self.focused = None;
                    } else if let Some(field) = self.named.iter_mut().find(|x| x.0 == k)
                        && field.1.key(host, code, ctrl)
                    {
                        let text = field.1.text.clone();
                        apply_named(host, k, &text);
                    }
                }
                FieldId::Setting(slot) => {
                    if code == KEY_ESCAPE || code == KEY_ENTER {
                        self.focused = None;
                    } else if let Some(field) = self.fields.iter_mut().find(|x| x.0 == slot)
                        && field.1.key(host, code, ctrl)
                    {
                        let text = field.1.text.clone();
                        core::with(host, |s, _| slot.put(s, Value::Text(text)));
                    }
                }
                FieldId::ProfileName => {
                    if code == KEY_ESCAPE || code == KEY_ENTER {
                        self.focused = None;
                    } else {
                        self.profile_name.key(host, code, ctrl);
                    }
                }
                FieldId::Search => {
                    if code == KEY_ESCAPE && !self.search.text.is_empty() {
                        self.set_search(String::new());
                    } else if code == KEY_ESCAPE || code == KEY_ENTER {
                        self.focused = None;
                    } else if self.search.key(host, code, ctrl) {
                        let text = self.search.text.clone();
                        self.set_search(text);
                    }
                }
            }
            return true;
        }
        let module_tab = keep(|k| k.tab.is_module_tab());
        if code == KEY_BACKSPACE && !self.search.text.is_empty() && module_tab {
            self.focused = Some(FieldId::Search);
            self.search.text.pop();
            let text = self.search.text.clone();
            self.set_search(text);
            return true;
        }
        false
    }

    /// Java's `charTyped`: typing without focus searches.
    fn char_typed(&mut self, host: &mut dyn Host, ch: char) {
        if self.listening.is_some() || self.listening_wf.is_some() {
            return;
        }
        if self.dropdown.is_some() {
            return;
        }
        if ch.is_whitespace() && self.focused.is_none() {
            return;
        }
        if ch.is_control() {
            return;
        }
        if self.focused.is_none() {
            keep(|k| {
                if !k.tab.is_module_tab() {
                    k.tab = k.last_module_tab;
                }
            });
            self.focused = Some(FieldId::Search);
        }
        match self.focused {
            Some(FieldId::Search) | None => {
                let mut text = self.search.text.clone();
                text.push(ch);
                self.set_search(text);
            }
            Some(FieldId::Setting(slot)) => {
                if let Some(field) = self.fields.iter_mut().find(|x| x.0 == slot) {
                    field.1.text.push(ch);
                    let text = field.1.text.clone();
                    core::with(host, |s, _| slot.put(s, Value::Text(text)));
                }
            }
            Some(FieldId::ProfileName) => self.profile_name.text.push(ch),
            Some(FieldId::Named(k)) => {
                if let Some(field) = self.named.iter_mut().find(|x| x.0 == k) {
                    field.1.text.push(ch);
                    let text = field.1.text.clone();
                    apply_named(host, k, &text);
                }
            }
        }
    }
}

/// Moves the dragged slider to `mouse_x` (`setFraction` / `setChannel`).
fn drag_to(host: &mut dyn Host, d: Drag, mouse_x: i32) {
    let f = slider_fraction(d.x1, d.x2, mouse_x);
    core::with(host, |s, _| {
        match (d.channel, d.slot.def().kind, d.slot.get(s)) {
            (Some(shift), _, Value::Color(argb)) => {
                let v = setting::java_round(f.clamp(0.0, 1.0) * 255.0) as i32;
                d.slot
                    .put(s, Value::Color(setting::set_channel(argb, shift, v)));
            }
            (None, Kind::Number { min, max, step, .. }, _) => {
                d.slot
                    .put(s, Value::Num(setting::from_fraction(f, min, max, step)));
            }
            _ => {}
        }
    });
}

/// Java's `changedMarker`: the blue bar and the reset icon.
fn changed_marker(c: &mut Ctx<'_>, slot: Slot, y: i32, h: i32) {
    let (left, right) = (c.l.left, c.l.right);
    c.fill(left + 4, y + 2, left + 6, y + h - 2, ACCENT);
    let hov = c.inside(right - 14, y, right, y + 14);
    c.icon(&ICON_RESET, right - 11, y + 4, if hov { TEXT } else { DIM });
    if hov {
        c.hover = Some(c.t("Changed - click to reset to the default"));
    }
    c.hit(right - 14, y, right, y + 14, Action::Reset(slot));
}

fn toggle<T: PartialEq>(list: &mut Vec<T>, item: T) {
    if let Some(p) = list.iter().position(|e| *e == item) {
        list.remove(p);
    } else {
        list.push(item);
    }
}

fn toggle_expanded(i: usize) {
    let id = MODULES[i].id;
    keep(|k| {
        if let Some(p) = k.expanded.iter().position(|e| *e == id) {
            k.expanded.remove(p);
        } else {
            k.expanded.push(id);
        }
    });
}

// ------------------------------------------------------------ editor tabs

const ICON_UP: [&str; 3] = ["..#..", ".###.", "#####"];

const WORKFLOW_INTRO: &str = "A workflow is a named list of steps. Run it with its key (optional), the Run button or /ot run <name>. Hold = steps are undone when the key is released. Movement steps (walk, look, turn, jump, clicks) finish before the next step starts and are blocked by Safe Mode.";
const HOTBAR_INTRO: &str = "Hotbar presets: save the items of your hotbar under a name and restore the layout later - matching items are pulled from your inventory. Also with /ot hotbar save|load <name> and the workflow step \"Load hotbar preset\".";
const SHARE_INTRO: &str = "Share settings: copy a share code (OMNI1:...) and send it to a friend or paste it on another PC. Not included: Safe Mode whitelist, update channel, language, waypoints, world seed. Workflows only with the second button - check them for passwords (e.g. /login) first.";

/// A workflow as the editor sees it.
#[derive(Clone)]
struct WfView {
    uid: u64,
    name: String,
    key: i32,
    hold: bool,
    steps: Vec<String>,
    trigger: Trigger,
    trigger_arg: String,
}

/// Writes a text field back to the model (Java's `onChange`).
fn apply_named(host: &mut dyn Host, key: NameKey, text: &str) {
    match key {
        NameKey::WfName(uid) => edit_wf(host, uid, |b| b.name = text.to_owned()),
        NameKey::TrigArg(uid) => edit_wf(host, uid, |b| b.trigger_arg = text.to_owned()),
        NameKey::WfArg(uid, i) => edit_wf(host, uid, |b| {
            let Some(old) = b.steps.get(i).cloned() else {
                return;
            };
            let verb = workflows::verb(&old);
            let arg = workflows::arg(&old);
            let mut new = text.to_owned();
            if matches!(verb.as_str(), "craft" | "trade") {
                new = workflows::with_drop(&new, workflows::has_drop(arg));
            }
            if verb == "trade" {
                new = trading::with_coords(&new, &trading::coords(arg));
            }
            b.steps[i] = format!("{verb}:{new}");
        }),
        NameKey::WfCoords(uid, i) => edit_wf(host, uid, |b| {
            let Some(old) = b.steps.get(i).cloned() else {
                return;
            };
            let verb = workflows::verb(&old);
            let rest = trading::without_coords(workflows::arg(&old));
            b.steps[i] = format!("{verb}:{}", trading::with_coords(&rest, text));
        }),
        NameKey::WpName(i) => wps::rename(&*host, i, text),
        NameKey::WpAdd | NameKey::HotbarName => {}
    }
}

fn edit_wf(host: &mut dyn Host, uid: u64, f: impl FnOnce(&mut workflows::Binding)) {
    core::with(host, |s, _| {
        if let Some(b) = s.bindings.iter_mut().find(|b| b.uid == uid) {
            f(b);
        }
    });
}

/// Java's `Dropdown.render` (spec 2.8), drawn over everything.
fn draw_dropdown(c: &mut Ctx<'_>, d: &Dropdown, sw: i32, sh: i32) {
    let rows = d.rows(sh);
    let top = d.top(sh);
    let (x, w) = (d.x, d.w);
    let hs = flow::DROP_ROW;
    c.fill(
        x - 1,
        top - 1,
        x + w + 1,
        top + rows as i32 * hs + 1,
        0xFF3F_A9F5,
    );
    c.fill(x, top, x + w, top + rows as i32 * hs, 0xFF16_1A21);
    let indent = if d.headers.is_empty() { 4 } else { 10 };
    for r in 0..rows {
        let i = r + d.scroll;
        let Some(label) = d.labels.get(i) else { break };
        let ry = top + r as i32 * hs;
        if d.headers.contains(&i) {
            c.fill(x, ry, x + w, ry + hs, 0xFF20_2634);
            c.text(label, x + 4, ry + 2, ACCENT);
            continue;
        }
        let hov = c.inside(x, ry, x + w, ry + hs);
        if hov {
            c.fill(x, ry, x + w, ry + hs, ROW_HOVER);
        }
        let shown = c.fit(label, w - indent - 4);
        c.text(&shown, x + indent, ry + 2, TEXT);
        if hov && let Some(hint) = d.hints.get(i).filter(|h| !h.is_empty()) {
            let hint = c.t(hint);
            let (hw, hx) = flow::hint_box(c.width(&hint), x, w, sw);
            c.fill(hx, ry, hx + hw, ry + hs, 0xF020_2634);
            c.outline(hx, ry, hx + hw, ry + hs, BORDER);
            let shown = c.fit(&hint, hw - 8);
            c.text(&shown, hx + 4, ry + 2, DIM);
        }
    }
    if d.labels.len() > rows {
        let (bh, off) = flow::dropdown_bar(d.labels.len(), rows, d.scroll);
        c.fill(x + w - 2, top + off, x + w, top + off + bh, 0xFF50_5868);
    }
}

impl Ctx<'_> {
    /// A button that opens a list: label plus the arrow.
    fn dd_button(&mut self, x: i32, y: i32, w: i32, label: &str, action: Action) {
        let hov = self.inside(x, y, x + w, y + 14);
        self.fill(x, y, x + w, y + 14, if hov { ACCENT } else { BORDER });
        self.fill(
            x + 1,
            y + 1,
            x + w - 1,
            y + 13,
            if hov { BTN_HOVER } else { BTN },
        );
        let shown = self.fit(label, w - 16);
        self.text(&shown, x + 4, y + 3, TEXT);
        self.icon(&ICON_DOWN, x + w - 10, y + 5, DIM);
        self.hit(x, y, x + w, y + 14, action);
    }

    /// A plain filled button with its own colours (Delete, Run, Record).
    fn tint_button(
        &mut self,
        r: [i32; 4],
        fill: u32,
        hover: u32,
        label: &str,
        text: u32,
        action: Action,
    ) {
        let [x1, y1, x2, y2] = r;
        let hov = self.inside(x1, y1, x2, y2);
        self.fill(x1, y1, x2, y2, if hov { hover } else { fill });
        let label = self.t(label);
        let tw = self.width(&label);
        self.text(
            &label,
            x1 + (x2 - x1 - tw) / 2,
            y1 + (y2 - y1 - 8) / 2 + 1,
            text,
        );
        self.hit(x1, y1, x2, y2, action);
    }
}

fn wf_field(k: NameKey) -> bool {
    matches!(
        k,
        NameKey::WfName(_) | NameKey::WfArg(..) | NameKey::WfCoords(..) | NameKey::TrigArg(_)
    )
}

impl Gui {
    #[allow(clippy::too_many_arguments)]
    fn named_field(
        &mut self,
        c: &mut Ctx<'_>,
        key: NameKey,
        init: &str,
        r: [i32; 4],
        placeholder: &str,
        now: u64,
        bad: bool,
    ) {
        if !self.named.iter().any(|f| f.0 == key) {
            self.named.push((
                key,
                Field {
                    text: init.to_owned(),
                },
            ));
        }
        let field = self.named.iter().find(|f| f.0 == key).unwrap().1.clone();
        let focused = self.focused == Some(FieldId::Named(key));
        c.field(&field, r, focused, placeholder, now);
        if bad {
            c.outline(r[0], r[1], r[2], r[3], BAD);
        }
        c.hit(r[0], r[1], r[2], r[3], Action::Focus(FieldId::Named(key)));
    }

    /// Discards the cached workflow fields (they are rebuilt from the model).
    fn clear_wf_fields(&mut self) {
        self.named.retain(|f| !wf_field(f.0));
        if matches!(self.focused, Some(FieldId::Named(k)) if wf_field(k)) {
            self.focused = None;
        }
    }

    // -------------------------------------------------------- workflows

    /// The Workflows tab (spec 4.6; without learned recipes and the
    /// villager Target button).
    fn workflows_tab(&mut self, c: &mut Ctx<'_>, mut y: i32, view_top: i32, now: u64) -> i32 {
        y = c.paragraph(y, view_top, WORKFLOW_INTRO) + 4;
        let (list, beta, profiles) = core::read(c.host, |s| {
            (
                s.bindings
                    .iter()
                    .map(|b| WfView {
                        uid: b.uid,
                        name: b.name.clone(),
                        key: b.key,
                        hold: b.hold(),
                        steps: b.steps.clone(),
                        trigger: b.trigger,
                        trigger_arg: b.trigger_arg.clone(),
                    })
                    .collect::<Vec<_>>(),
                s.beta,
                s.profiles(),
            )
        });
        let open = keep(|k| k.open_wf);
        let left = c.l.left;
        for (i, b) in list.iter().enumerate() {
            let is_open = open == i as i32;
            if c.visible(y, 20, view_top) {
                self.wf_header(c, b, i, is_open, y, now);
            }
            y += 22;
            if !is_open {
                continue;
            }
            if c.visible(y, 14, view_top) {
                self.wf_options(c, b, y);
            }
            y += 18;
            if beta {
                if c.visible(y, 14, view_top) {
                    self.wf_trigger(c, b, y, now);
                }
                y += 18;
            }
            let (_, levels) = workflows::blocks(&b.steps);
            for (n, step) in b.steps.iter().enumerate() {
                if c.visible(y, 15, view_top) {
                    self.step_row(c, &list, b, n, step, levels[n], y, &profiles, now, beta);
                }
                y += 16;
            }
            if b.steps.is_empty() {
                if c.visible(y, 14, view_top) {
                    let t = c.t("No steps yet - add the first one.");
                    c.text(&t, left + 12, y + 2, DIM);
                }
                y += 14;
            }
            if c.visible(y, 14, view_top) {
                let mut d = new_dropdown(left + 8, y + 14, 180, Pick::NewStep(b.uid, Vec::new()));
                let ids = picker(c, beta, &mut d);
                d.target = Pick::NewStep(b.uid, ids);
                c.button(left + 8, y, 90, "+ Add step", Action::Drop(Box::new(d)));
            }
            y += 26;
        }
        if c.visible(y, 14, view_top) {
            c.button(left, y, 120, "+ New workflow", Action::WfNew);
        }
        y + 20
    }

    fn wf_header(&mut self, c: &mut Ctx<'_>, b: &WfView, i: usize, open: bool, y: i32, now: u64) {
        let (left, right) = (c.l.left, c.l.right);
        let hov = c.inside(left, y, right, y + 20);
        c.fill(
            left,
            y,
            right,
            y + 20,
            if open {
                0xFF22_2937
            } else if hov {
                ROW_HOVER
            } else {
                ROW
            },
        );
        if open {
            c.fill(left, y, left + 2, y + 20, ACCENT);
            c.icon(&ICON_DOWN, left + 7, y + 9, TEXT);
        } else {
            c.icon(&ICON_RIGHT, left + 8, y + 8, DIM);
        }
        c.hit(left, y, right, y + 20, Action::OpenWf(i as i32));
        let run_x = right - 50;
        if open {
            self.named_field(
                c,
                NameKey::WfName(b.uid),
                &b.name,
                [left + 18, y + 3, (left + 180).min(run_x - 6), y + 17],
                "Name",
                now,
                false,
            );
        } else {
            let name = if b.name.trim().is_empty() {
                c.t("(unnamed)")
            } else {
                b.name.clone()
            };
            let title = c.fit(&name, 150);
            c.text(&title, left + 18, y + 6, TEXT);
            let n = b.steps.len();
            let mut sum = format!("{n} {}", c.t(if n == 1 { "step" } else { "steps" }));
            if b.key != NONE {
                sum += &format!("  ·  {}", core::key_name(b.key));
            }
            if b.trigger != Trigger::None {
                sum += &format!("  ·  {}", c.t(b.trigger.label()));
            }
            let x = left + 24 + c.width(&title);
            let sum = c.fit(&sum, run_x - 6 - x);
            c.text(&sum, x, y + 6, DIM);
        }
        let running = core::workflow_running(b.uid);
        let r = [run_x, y + 3, run_x + 46, y + 17];
        let (fill, hover, label, action, tip) = if running {
            (
                0xFF5A_2828,
                0xFF7A_3030,
                "Stop",
                Action::WfStop(b.uid),
                "Stops the workflow",
            )
        } else {
            (
                0xFF24_5C38,
                0xFF2F_7A48,
                "Run",
                Action::WfRun(b.uid),
                "Runs the workflow now",
            )
        };
        if c.inside(r[0], r[1], r[2], r[3]) {
            c.hover = Some(c.t(tip));
        }
        c.tint_button(r, fill, hover, label, TEXT, action);
    }

    fn wf_options(&mut self, c: &mut Ctx<'_>, b: &WfView, y: i32) {
        let (left, right) = (c.l.left, c.l.right);
        let x = left + 12;
        let t = c.t("Key:");
        c.text(&t, x, y + 3, DIM);
        let kx = x + c.width(&t) + 6;
        let label = if self.listening_wf == Some(b.uid) {
            "...".to_owned()
        } else if b.key == NONE {
            "+ Key".to_owned()
        } else {
            core::key_name(b.key)
        };
        if c.inside(kx, y, kx + 70, y + 14) {
            c.hover = Some(c.t("Click and press a key. Right-click removes it."));
        }
        c.button(kx, y, 70, &label, Action::WfKey(b.uid));
        let mx = kx + 76;
        let mut d = new_dropdown(mx, y + 14, 120, Pick::Mode(b.uid));
        d.labels = vec![c.t("On press"), c.t("While held")];
        if c.inside(mx, y, mx + 84, y + 14) {
            c.hover = Some(c.t(
                "While held: toggles, enable/disable and Safe Mode are undone when you release the key",
            ));
        }
        let shown = c.t(if b.hold { "While held" } else { "On press" });
        c.dd_button(mx, y, 84, &shown, Action::Drop(Box::new(d)));
        let confirm = self.confirm_delete == Some(b.uid);
        let w = if confirm { 80 } else { 50 };
        c.tint_button(
            [right - w, y, right, y + 14],
            if confirm { 0xFF8A_2E2E } else { BTN },
            if confirm { 0xFFB0_3A3A } else { 0xFF5A_2828 },
            if confirm { "Really delete?" } else { "Delete" },
            if confirm { TEXT } else { 0xFFFF_8080 },
            Action::WfDelete(b.uid),
        );
    }

    fn wf_trigger(&mut self, c: &mut Ctx<'_>, b: &WfView, y: i32, now: u64) {
        let right = c.l.right;
        let x = c.l.left + 12;
        let t = c.t("Trigger:");
        c.text(&t, x, y + 3, DIM);
        let bx = x + c.width(&t) + 6;
        let mut d = new_dropdown(bx, y + 14, 160, Pick::Trigger(b.uid));
        d.labels = Trigger::ALL.iter().map(|t| c.t(t.label())).collect();
        let shown = c.t(b.trigger.label());
        c.dd_button(bx, y, 130, &shown, Action::Drop(Box::new(d)));
        let hint = b.trigger.hint();
        if !hint.is_empty() {
            let ph = c.t(hint);
            self.named_field(
                c,
                NameKey::TrigArg(b.uid),
                &b.trigger_arg,
                [bx + 134, y, right - 62 - 4, y + 13],
                &ph,
                now,
                false,
            );
        }
        let rec = core::workflow_recording(b.uid);
        let rx = right - 62;
        c.tint_button(
            [rx, y, right, y + 14],
            if rec { 0xFF8A_2E2E } else { BTN },
            if rec { 0xFFB0_3A3A } else { BTN_HOVER },
            if rec { "Stop rec" } else { "Record" },
            TEXT,
            Action::WfRecord(b.uid),
        );
        c.fill(
            rx + 5,
            y + 5,
            rx + 9,
            y + 9,
            if rec { TEXT } else { 0xFFFF_5555 },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn step_row(
        &mut self,
        c: &mut Ctx<'_>,
        all: &[WfView],
        b: &WfView,
        n: usize,
        step: &str,
        level: i32,
        y: i32,
        profiles: &[String],
        now: u64,
        beta: bool,
    ) {
        let right = c.l.right;
        let x0 = c.l.left + 8 + 10 * level.min(6);
        c.fill(x0, y, right, y + 15, ROW);
        let verb = workflows::verb(step);
        let info = workflows::step_of(step);
        let color = match info {
            Some(st) => flow::GROUPS[flow::group(st.id)].1,
            None => BAD,
        };
        c.fill(x0, y, x0 + 2, y + 15, color);
        let num = (n + 1).to_string();
        let nx = x0 + 6 + (12 - c.width(&num)) / 2;
        c.text(&num, nx, y + 4, DIM);
        // type dropdown
        let tx = x0 + 22;
        let type_label = match info {
            Some(st) => c.t(flow::label_hint(st.id).0),
            None => verb.clone(),
        };
        let mut d = new_dropdown(tx, y + 14, 180, Pick::StepType(b.uid, n, Vec::new()));
        let ids = picker(c, beta, &mut d);
        d.target = Pick::StepType(b.uid, n, ids);
        c.dd_button(tx, y + 1, 104, &type_label, Action::Drop(Box::new(d)));
        // argument
        let ax = tx + 108;
        let ax2 = right - 46;
        let arg = workflows::arg(step).to_owned();
        let uid = b.uid;
        let list_button =
            |c: &mut Ctx<'_>, w: i32, shown: String, labels: Vec<String>, values: Vec<String>| {
                let mut d = new_dropdown(ax, y + 14, w.max(150), Pick::StepArg(uid, n, values));
                d.labels = labels;
                c.dd_button(ax, y + 1, w, &shown, Action::Drop(Box::new(d)));
            };
        match info.map(|s| s.arg) {
            Some(Arg::Module) => {
                let shown = MODULES
                    .iter()
                    .find(|m| m.id == arg)
                    .map_or(arg.clone(), |m| c.t(m.name));
                let labels = MODULES.iter().map(|m| c.t(m.name)).collect();
                let values = MODULES.iter().map(|m| m.id.to_owned()).collect();
                list_button(c, 150.min(ax2 - ax), shown, labels, values);
            }
            Some(a @ (Arg::Profile | Arg::Workflow)) => {
                let values: Vec<String> = if a == Arg::Profile {
                    profiles.to_vec()
                } else {
                    all.iter()
                        .filter(|o| o.uid != b.uid)
                        .map(|o| o.name.clone())
                        .collect()
                };
                let shown = if arg.is_empty() {
                    c.t("(choose)")
                } else {
                    arg.clone()
                };
                list_button(c, 150.min(ax2 - ax), shown, values.clone(), values);
            }
            Some(Arg::OnOff) => {
                let shown = c.t(match arg.as_str() {
                    "on" => "On",
                    "off" => "Off",
                    _ => "Toggle",
                });
                let labels = vec![c.t("Toggle"), c.t("On"), c.t("Off")];
                let values = vec!["toggle".to_owned(), "on".to_owned(), "off".to_owned()];
                list_button(c, 80, shown, labels, values);
            }
            Some(Arg::Text) => {
                let mut fx = ax;
                let drop_step = matches!(verb.as_str(), "craft" | "trade");
                if drop_step {
                    let on = workflows::has_drop(&arg);
                    if c.inside(fx, y + 1, fx + 52, y + 15) {
                        c.hover = Some(
                            c.t("Inventory full: on = drop the results and continue, off = stop"),
                        );
                    }
                    c.button(
                        fx,
                        y + 1,
                        52,
                        if on { "Drop: on" } else { "Drop: off" },
                        Action::DropToggle(b.uid, n),
                    );
                    fx += 56;
                }
                let mut field_end = ax2;
                if matches!(verb.as_str(), "trade" | "villager") {
                    let text = trading::coords(&arg);
                    let ph = c.t("x y z (empty = nearest)");
                    let bad = !text.is_empty() && !flow::coords_valid(&text);
                    let cx = if verb == "villager" {
                        fx
                    } else {
                        field_end = ax2 - 124;
                        ax2 - 120
                    };
                    self.named_field(
                        c,
                        NameKey::WfCoords(b.uid, n),
                        &text,
                        [cx, y + 1, ax2, y + 14],
                        &ph,
                        now,
                        bad,
                    );
                }
                if verb != "villager" {
                    let init = if drop_step {
                        trading::without_coords(&workflows::with_drop(&arg, false))
                    } else {
                        arg.clone()
                    };
                    let ph = c.t(flow::label_hint(&verb).1);
                    let bad = verb == "if"
                        && !arg.is_empty()
                        && super::omnitool_conditions::parse(&arg).is_none();
                    self.named_field(
                        c,
                        NameKey::WfArg(b.uid, n),
                        &init,
                        [fx, y + 1, field_end.max(fx + 20), y + 14],
                        &ph,
                        now,
                        bad,
                    );
                }
            }
            _ => {}
        }
        // move up / down / remove
        let bx = right - 43;
        let buttons: [(&[&str], u32, Action, &str); 3] = [
            (&ICON_UP, TEXT, Action::StepMove(b.uid, n, -1), "Move up"),
            (&ICON_DOWN, TEXT, Action::StepMove(b.uid, n, 1), "Move down"),
            (
                &ICON_X,
                0xFFFF_8080,
                Action::StepRemove(b.uid, n),
                "Remove step",
            ),
        ];
        for (k, (icon, col, act, tip)) in buttons.into_iter().enumerate() {
            let x = bx + 14 * k as i32;
            let hov = c.inside(x, y + 1, x + 13, y + 14);
            c.fill(x, y + 1, x + 13, y + 14, if hov { BTN_HOVER } else { BTN });
            c.icon(icon, x + 4, y + 5, col);
            if hov {
                c.hover = Some(c.t(tip));
            }
            c.hit(x, y + 1, x + 13, y + 14, act);
        }
    }

    // -------------------------------------------------------- waypoints

    fn waypoints_tab(&mut self, c: &mut Ctx<'_>, mut y: i32, view_top: i32, now: u64) -> i32 {
        if c.host.player().is_none() {
            return c.paragraph(y, view_top, "Join a world to manage waypoints.");
        }
        let on = core::read(c.host, |s| {
            MODULES
                .iter()
                .position(|m| m.id == "waypoints")
                .is_some_and(|i| s.modules[i].enabled)
        });
        if !on {
            y = c.paragraph(
                y,
                view_top,
                "The Waypoints module is off - enable it in the World tab to see them.",
            ) + 2;
        }
        let (left, right) = (c.l.left, c.l.right);
        if c.visible(y, 14, view_top) {
            self.named_field(
                c,
                NameKey::WpAdd,
                "",
                [left, y, right - 110, y + 14],
                "Name",
                now,
                false,
            );
            c.button(right - 106, y, 106, "Add here", Action::WpAdd);
        }
        y += 22;
        for (i, w) in wps::all(c.host).into_iter().enumerate() {
            if c.visible(y, 14, view_top) {
                c.fill(left, y, left + 4, y + 14, w.color as u32 | 0xFF00_0000);
                self.named_field(
                    c,
                    NameKey::WpName(i),
                    w.name(),
                    [left + 6, y, left + 120, y + 14],
                    "Name",
                    now,
                    false,
                );
                let dim = w
                    .dim
                    .as_deref()
                    .unwrap_or("")
                    .trim_start_matches("minecraft:")
                    .to_owned();
                let info = format!("{} {} {}  {dim}", w.x, w.y, w.z);
                let info = c.fit(&info, right - 54 - (left + 126));
                c.text(&info, left + 126, y + 3, DIM);
                c.button(right - 50, y, 50, "Delete", Action::WpDelete(i));
            }
            y += 17;
        }
        y
    }

    // -------------------------------------------------------- inventory

    fn inventory_tab(&mut self, c: &mut Ctx<'_>, mut y: i32, view_top: i32, now: u64) -> i32 {
        if c.host.player().is_none() {
            return c.paragraph(
                y,
                view_top,
                "Join a world to use hotbar presets and the craft calculator.",
            );
        }
        y = c.paragraph(y, view_top, HOTBAR_INTRO) + 2;
        let (left, right) = (c.l.left, c.l.right);
        if c.visible(y, 14, view_top) {
            self.named_field(
                c,
                NameKey::HotbarName,
                "",
                [left, y, right - 110, y + 14],
                "Preset name",
                now,
                false,
            );
            c.button(right - 106, y, 106, "Save hotbar", Action::HotbarSave);
        }
        y += 18;
        for (name, ids) in hotbar::list(c.host) {
            if c.visible(y, 15, view_top) {
                c.fill(left, y, right, y + 15, ROW);
                c.text(&name, left + 4, y + 4, TEXT);
                let preview: Vec<String> = ids
                    .iter()
                    .map(|i| {
                        if i.is_empty() {
                            "-".to_owned()
                        } else {
                            i.trim_start_matches("minecraft:").to_owned()
                        }
                    })
                    .collect();
                let px = left + 12 + c.width(&name);
                let shown = c.fit(&preview.join(", "), right - 114 - px);
                c.text(&shown, px, y + 4, DIM);
                c.button(right - 110, y, 50, "Load", Action::HotbarLoad(name.clone()));
                c.button(right - 56, y, 56, "Delete", Action::HotbarDelete(name));
            }
            y += 17;
        }
        if !self.hotbar_status.is_empty() {
            let s = self.hotbar_status.clone();
            y = c.paragraph(y, view_top, &s);
        }
        y
    }

    /// The Settings tab's share block (beta).
    fn share_block(&mut self, c: &mut Ctx<'_>, mut y: i32, view_top: i32) -> i32 {
        y += 6;
        y = c.paragraph(y, view_top, SHARE_INTRO);
        let left = c.l.left;
        if c.visible(y, 14, view_top) {
            c.button(left, y, 120, "Copy share code", Action::ShareCopy(false));
            c.button(
                left + 126,
                y,
                130,
                "Copy with workflows",
                Action::ShareCopy(true),
            );
            c.button(
                left + 262,
                y,
                130,
                "Import from clipboard",
                Action::ShareImport,
            );
        }
        y += 18;
        if !self.notice.is_empty() {
            let n = self.notice.clone();
            y = c.paragraph(y, view_top, &n);
        }
        let preview = self.import_preview.clone();
        if !preview.is_empty() {
            for line in preview.iter().take(15) {
                y = c.paragraph(y, view_top, line);
            }
            if preview.len() > 15 {
                y = c.paragraph(y, view_top, &format!("... +{}", preview.len() - 15));
            }
            if c.visible(y, 14, view_top) {
                c.button(left, y, 120, "Apply import", Action::ShareApply);
                c.button(left + 126, y, 80, "Cancel", Action::ShareCancel);
            }
            y += 18;
        }
        y
    }

    // -------------------------------------------------------- actions

    /// A picked dropdown entry.
    fn pick(&mut self, host: &mut dyn Host, d: &Dropdown, i: usize) {
        match &d.target {
            Pick::Mode(uid) => edit_wf(host, *uid, |b| {
                b.mode = Some(if i == 1 { Mode::Hold } else { Mode::Press });
            }),
            Pick::Trigger(uid) => {
                let t = Trigger::ALL[i.min(Trigger::ALL.len() - 1)];
                edit_wf(host, *uid, |b| {
                    b.trigger = t;
                    if t.hint().is_empty() {
                        b.trigger_arg.clear();
                    }
                });
                self.clear_wf_fields();
            }
            Pick::StepArg(uid, n, values) => {
                let Some(v) = values.get(i).cloned() else {
                    return;
                };
                edit_wf(host, *uid, |b| {
                    if let Some(step) = b.steps.get(*n) {
                        let verb = workflows::verb(step);
                        b.steps[*n] = format!("{verb}:{v}");
                    }
                });
                self.clear_wf_fields();
            }
            Pick::StepType(uid, _, ids) | Pick::NewStep(uid, ids) => {
                let Some(id) = ids.get(i).cloned() else {
                    return;
                };
                let profile =
                    core::read(&*host, |s| s.profiles().first().cloned()).unwrap_or_default();
                let new = flow::new_step(&id, MODULES[0].id, &profile);
                let at = match &d.target {
                    Pick::StepType(_, n, _) => Some(*n),
                    _ => None,
                };
                edit_wf(host, *uid, |b| {
                    let idx = match at {
                        Some(n) if n < b.steps.len() => {
                            b.steps[n] = new;
                            n
                        }
                        _ => {
                            b.steps.push(new);
                            b.steps.len() - 1
                        }
                    };
                    if flow::needs_end(&id) {
                        let (jump, _) = workflows::blocks(&b.steps);
                        if jump[idx] == b.steps.len() as i32 {
                            b.steps.insert(idx + 1, "end".to_owned());
                        }
                    }
                });
                self.clear_wf_fields();
            }
        }
    }

    fn act_tabs(&mut self, host: &mut dyn Host, action: Action, button: i32) {
        match action {
            Action::OpenWf(i) => {
                keep(|k| k.open_wf = if k.open_wf == i { -1 } else { i });
                self.confirm_delete = None;
                self.clear_wf_fields();
            }
            Action::WfRun(uid) => core::run_workflow(host, uid),
            Action::WfStop(uid) => core::stop_workflow(host, uid),
            Action::WfKey(uid) => {
                if button == 1 {
                    edit_wf(host, uid, |b| b.key = NONE);
                } else {
                    self.listening_wf = Some(uid);
                }
            }
            Action::WfDelete(uid) => {
                if self.confirm_delete == Some(uid) {
                    core::with(host, |s, _| s.bindings.retain(|b| b.uid != uid));
                    self.confirm_delete = None;
                    keep(|k| k.open_wf = -1);
                    self.clear_wf_fields();
                } else {
                    self.confirm_delete = Some(uid);
                }
            }
            Action::WfNew => {
                let n = core::with(host, |s, _| {
                    let name = format!("Workflow {}", s.bindings.len() + 1);
                    s.bindings.push(workflows::Binding::new(&name));
                    s.bindings.len() as i32 - 1
                });
                keep(|k| k.open_wf = n);
                self.confirm_delete = None;
                self.clear_wf_fields();
            }
            Action::WfRecord(uid) => {
                if core::workflow_recording(uid) {
                    core::stop_recording(host);
                } else {
                    core::with(host, |s, _| s.save());
                    core::start_recording(uid);
                    self.close = true;
                }
            }
            Action::StepMove(uid, n, dir) => {
                edit_wf(host, uid, |b| {
                    let m = n as i32 + dir;
                    if m >= 0 && (m as usize) < b.steps.len() && n < b.steps.len() {
                        b.steps.swap(n, m as usize);
                    }
                });
                self.clear_wf_fields();
            }
            Action::StepRemove(uid, n) => {
                edit_wf(host, uid, |b| {
                    if n < b.steps.len() {
                        b.steps.remove(n);
                    }
                });
                self.clear_wf_fields();
            }
            Action::DropToggle(uid, n) => {
                edit_wf(host, uid, |b| {
                    if let Some(step) = b.steps.get(n).cloned() {
                        let arg = workflows::arg(&step);
                        let new = workflows::with_drop(arg, !workflows::has_drop(arg));
                        b.steps[n] = format!("{}:{new}", workflows::verb(&step));
                    }
                });
                self.clear_wf_fields();
            }
            Action::Drop(d) => {
                if !d.labels.is_empty() {
                    self.dropdown = Some(*d);
                }
            }
            Action::WpAdd => {
                let text = self.field_text(NameKey::WpAdd);
                wps::add_here(host, if text.is_empty() { None } else { Some(&text) });
                self.named.retain(|f| f.0 != NameKey::WpAdd);
            }
            Action::WpDelete(i) => {
                wps::remove_at(&*host, i);
                self.named.retain(|f| !matches!(f.0, NameKey::WpName(_)));
                self.focused = None;
            }
            Action::HotbarSave => {
                let text = self.field_text(NameKey::HotbarName);
                self.hotbar_status = hotbar::save(host, &text);
                self.named.retain(|f| f.0 != NameKey::HotbarName);
            }
            Action::HotbarLoad(name) => self.hotbar_status = hotbar::restore(host, &name),
            Action::HotbarDelete(name) => {
                hotbar::delete(&*host, &name);
            }
            Action::ShareCopy(with_workflows) => {
                self.notice = core::share_export(host, with_workflows);
                self.import_preview.clear();
            }
            Action::ShareImport => match core::share_import_preview(host) {
                Ok(changes) if changes.is_empty() => {
                    self.notice =
                        "The share code matches your settings - nothing to change".to_owned();
                    self.import_preview.clear();
                }
                Ok(changes) => {
                    self.notice = format!("{} changes:", changes.len());
                    self.import_preview = changes;
                }
                Err(e) => {
                    self.notice = e;
                    self.import_preview.clear();
                }
            },
            Action::ShareApply => {
                if core::share_import_apply(host) {
                    self.notice = "Settings imported".to_owned();
                    self.fields.clear();
                }
                self.import_preview.clear();
            }
            Action::ShareCancel => self.import_preview.clear(),
            _ => {}
        }
    }

    fn field_text(&self, key: NameKey) -> String {
        self.named
            .iter()
            .find(|f| f.0 == key)
            .map(|f| f.1.text.trim().to_owned())
            .unwrap_or_default()
    }
}

fn new_dropdown(x: i32, y: i32, w: i32, target: Pick) -> Dropdown {
    Dropdown {
        x,
        y,
        w,
        labels: Vec::new(),
        headers: Vec::new(),
        hints: Vec::new(),
        scroll: 0,
        target,
    }
}

/// Fills `d` with the step picker (spec 4.6: groups as headers, available
/// steps only); the step ids in entry order (headers get "").
fn picker(c: &Ctx<'_>, beta: bool, d: &mut Dropdown) -> Vec<String> {
    let mut ids = Vec::new();
    for (g, (title, _)) in flow::GROUPS.iter().enumerate() {
        let steps: Vec<&workflows::Step> = workflows::STEPS
            .iter()
            .filter(|s| flow::group(s.id) == g && s.available(beta, core::CURSEFORGE))
            .collect();
        if steps.is_empty() {
            continue;
        }
        d.headers.push(d.labels.len());
        d.labels.push(c.t(title));
        d.hints.push(String::new());
        ids.push(String::new());
        for s in steps {
            let (label, hint) = flow::label_hint(s.id);
            d.labels.push(c.t(label));
            d.hints.push(hint.to_owned());
            ids.push(s.id.to_owned());
        }
    }
    ids
}

// ------------------------------------------------------------ tests

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::omnitool_core::tests::Fake;
    use serde_json::Value as Json;

    fn fixtures() -> Json {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/clickgui.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn int(j: &Json, k: &str) -> i32 {
        j.get(k).unwrap().as_f64().unwrap() as i32
    }

    fn num(j: &Json, k: &str) -> f64 {
        j.get(k).unwrap().as_f64().unwrap()
    }

    fn rect(j: &Json, k: &str) -> [i32; 4] {
        let a = j.get(k).unwrap().as_array().unwrap();
        std::array::from_fn(|i| a[i].as_f64().unwrap() as i32)
    }

    fn six(s: &str) -> i32 {
        s.chars().count() as i32 * 6
    }

    #[test]
    fn panel_layout() {
        for case in fixtures().get("panel_layout").unwrap().as_array().unwrap() {
            let s = case.get("screen").unwrap().as_array().unwrap();
            let l = layout(s[0].as_f64().unwrap() as i32, s[1].as_f64().unwrap() as i32);
            assert_eq!([l.px, l.py, l.px + l.pw, l.py + l.ph], rect(case, "panel"));
            assert_eq!(
                [l.px + 1, l.py + 18, l.px + 92, l.py + l.ph - 1],
                rect(case, "sidebar")
            );
            assert_eq!(l.left, int(case, "left"));
            assert_eq!(l.right, int(case, "right"));
            assert_eq!(l.top, int(case, "top"));
            assert_eq!(l.bottom, int(case, "bottom"));
            assert_eq!(l.top + 18, int(case, "view_top_module_tab"));
            assert_eq!(l.py + l.ph - 12, int(case, "footer_y"));
        }
    }

    #[test]
    fn fit_and_wrap() {
        let f = fixtures();
        for case in f.get("fit_text").unwrap().as_array().unwrap() {
            let text = case.get("text").unwrap().as_str().unwrap();
            let got = fit_text(&six, text, int(case, "max"));
            assert_eq!(
                got,
                case.get("expected").unwrap().as_str().unwrap(),
                "{text}"
            );
        }
        for case in f.get("paragraph_wrap").unwrap().as_array().unwrap() {
            let text = case.get("text").unwrap().as_str().unwrap();
            let want: Vec<String> = case
                .get("lines")
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l.as_str().unwrap().to_owned())
                .collect();
            assert_eq!(wrap(&six, text, int(case, "max")), want);
        }
    }

    #[test]
    fn scrollbar_and_animation() {
        let f = fixtures();
        for case in f.get("scrollbar").unwrap().as_array().unwrap() {
            let e = case.get("expected").unwrap();
            let none = e.is_null();
            let (max, bar_h, bar_y) = scrollbar(
                int(case, "view_top"),
                int(case, "bottom"),
                int(case, "content"),
                int(case, "scroll"),
            );
            if none {
                assert_eq!(max, 0);
                continue;
            }
            assert_eq!(max, int(e, "max_scroll"));
            if max > 0 {
                assert_eq!(bar_h, int(e, "bar_h"));
                assert_eq!(bar_y, int(e, "bar_y"));
            }
        }
        let anim = f.get("scroll_animation").unwrap();
        let target = num(anim, "target");
        let mut a = num(anim, "start");
        for step in anim.get("steps").unwrap().as_array().unwrap() {
            a = scroll_step(a, target, Some(num(step, "dt")));
            assert!((a - num(step, "anim")).abs() < 1e-5, "{a}");
            assert_eq!(round(a), int(step, "scroll_px"));
        }
    }

    #[test]
    fn search_cases() {
        let f = fixtures();
        let s = f.get("search").unwrap();
        let modules = s.get("modules").unwrap().as_array().unwrap();
        for case in s.get("cases").unwrap().as_array().unwrap() {
            let raw = case.get("query").unwrap().as_str().unwrap();
            let q = raw.trim().to_lowercase();
            let german = case.get("lang").unwrap().as_str().unwrap() == "de";
            let mut got = Vec::new();
            for m in modules {
                let name = m.get("name").unwrap().as_str().unwrap();
                let name_de = m.get("name_de").unwrap().as_str().unwrap().to_owned();
                let tr = |t: &str| {
                    if german && t == name {
                        name_de.clone()
                    } else {
                        t.to_owned()
                    }
                };
                let settings: Vec<&str> = m
                    .get("settings")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| s.as_str().unwrap())
                    .collect();
                let replaces = m.get("replaces").and_then(|r| r.as_str()).unwrap_or("");
                let id = m.get("id").unwrap().as_str().unwrap();
                let desc = m.get("description").unwrap().as_str().unwrap();
                if let Some((name_match, hits)) =
                    search_match(&q, &tr, id, name, desc, replaces, &settings)
                {
                    let names: Vec<String> = if name_match {
                        Vec::new()
                    } else {
                        hits.iter().map(|&i| settings[i].to_owned()).collect()
                    };
                    got.push((id.to_owned(), name_match, names));
                }
            }
            let want: Vec<(String, bool, Vec<String>)> = case
                .get("result")
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    (
                        r.get("id").unwrap().as_str().unwrap().to_owned(),
                        r.get("name_match").unwrap().as_bool().unwrap(),
                        r.get("matching_settings")
                            .unwrap()
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|s| s.as_str().unwrap().to_owned())
                            .collect(),
                    )
                })
                .collect();
            assert_eq!(got, want, "query {raw:?}");
        }
    }

    #[test]
    fn sidebar_order() {
        let tabs = fixtures();
        let order = tabs.get("tabs").unwrap().get("sidebar_order").unwrap();
        let want: Vec<&str> = order
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_str().unwrap())
            .filter(|t| !t.starts_with('('))
            .map(|t| t.split(" (").next().unwrap())
            .filter(|t| !t.starts_with("Screenshots"))
            .collect();
        let got: Vec<&str> = sidebar(true).iter().map(|t| t.0.title()).collect();
        assert_eq!(got, want);
        assert!(!sidebar(false).iter().any(|t| t.0 == Tab::Inventory));
    }

    /// A screen at 854x480 on a fresh state.
    pub fn fresh() -> Fake {
        crate::omnitool_core::tests::restart();
        KEEP.with(|k| *k.borrow_mut() = Keep::default());
        OPEN.with(|o| *o.borrow_mut() = None);
        open();
        Fake::default()
    }

    fn texts(ops: &[Op]) -> Vec<String> {
        ops.iter()
            .filter_map(|o| match o {
                Op::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn index(id: &str) -> usize {
        MODULES.iter().position(|m| m.id == id).unwrap()
    }

    /// Mouse position of the first header row of `id` on the current frame.
    fn header_y(ops: &[Op], name: &str) -> i32 {
        ops.iter()
            .find_map(|o| match o {
                Op::Text { text, y, .. } if text == name => Some(*y - 5),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn render_tabs_and_toggle() {
        let mut h = fresh();
        let f = frame(&mut h, 854, 480, 0, 0);
        let t = texts(&f.ops);
        assert!(t.contains(&"Omnitool".to_owned()));
        assert!(t.contains(&"Safe Mode OFF (Auto)".to_owned()));
        assert!(t.contains(&"Fullbright".to_owned()), "{t:?}");
        // the panel clips its content
        assert!(f.ops.contains(&Op::Clip(Some([285, 48, 659, 456]))));
        let l = layout(854, 480);
        let y = header_y(&f.ops, "Fullbright");
        let i = index("fullbright");
        let before = core::read(&h, |s| s.modules[i].enabled);
        // click the switch
        push(Input::Click(0));
        frame(&mut h, 854, 480, l.right - 10, y + 5);
        assert_eq!(core::read(&h, |s| s.modules[i].enabled), !before);
        // right click the row: settings open
        push(Input::Click(1));
        frame(&mut h, 854, 480, l.left + 60, y + 5);
        assert!(keep(|k| k.expanded.contains(&"fullbright")));
        assert_eq!(core::read(&h, |s| s.modules[i].enabled), !before);
        // the star
        push(Input::Click(0));
        frame(&mut h, 854, 480, l.left + 8, y + 5);
        assert!(core::read(&h, |s| s.modules[i].favorite));
    }

    #[test]
    fn sidebar_switches_tab_and_typing_searches() {
        let mut h = fresh();
        let l = layout(854, 480);
        frame(&mut h, 854, 480, 0, 0);
        // Settings tab: the 14th sidebar row incl. the gaps (beta: Inventory)
        let y = l.py + 22 + 15 * 12 + 9;
        push(Input::Click(0));
        let f = frame(&mut h, 854, 480, l.px + 10, y + 2);
        assert_eq!(keep(|k| k.tab), Tab::Settings);
        assert!(!texts(&f.ops).contains(&"Fullbright".to_owned()));
        // typing goes back to the last module tab and searches everywhere
        for ch in "zoom".chars() {
            push(Input::Char(ch));
        }
        let f = frame(&mut h, 854, 480, 0, 0);
        assert_eq!(keep(|k| k.tab), Tab::Category(Category::Render));
        let t = texts(&f.ops);
        assert!(t.contains(&"Zoom".to_owned()), "{t:?}");
        assert!(!t.contains(&"Fullbright".to_owned()));
        // Esc in the search clears it, a second Esc unfocuses, a third closes
        push(Input::Key(KEY_ESCAPE, false));
        push(Input::Key(KEY_ESCAPE, false));
        let f = frame(&mut h, 854, 480, 0, 0);
        assert!(!f.close);
        assert!(texts(&f.ops).contains(&"Fullbright".to_owned()));
        push(Input::Key(KEY_ESCAPE, false));
        assert!(frame(&mut h, 854, 480, 0, 0).close);
        assert!(!is_open());
    }

    #[test]
    fn nothing_found_and_scroll_clamps() {
        let mut h = fresh();
        for ch in "qqqq".chars() {
            push(Input::Char(ch));
        }
        let f = frame(&mut h, 854, 480, 0, 0);
        assert!(texts(&f.ops).contains(&"Nothing found for \"qqqq\"".to_owned()));
        push(Input::Scroll(-100.0));
        frame(&mut h, 854, 480, 0, 0);
        // nothing to scroll: the target snaps back to 0 on the next frame
        frame(&mut h, 854, 480, 0, 0);
        assert_eq!(keep(|k| k.scroll(Tab::Category(Category::Render))), 0);
    }

    #[test]
    fn slider_fixtures() {
        let f = fixtures();
        for case in f.get("slider_knob").unwrap().as_array().unwrap() {
            let got = slider_knob(int(case, "x1"), int(case, "x2"), num(case, "fraction"));
            assert_eq!(got, int(case, "knob_x"), "{case}");
        }
        for case in f.get("slider_drag").unwrap().as_array().unwrap() {
            let fr = slider_fraction(int(case, "x1"), int(case, "x2"), int(case, "mouse_x"));
            assert!((fr - num(case, "fraction")).abs() < 1e-9);
            let v =
                setting::from_fraction(fr, num(case, "min"), num(case, "max"), num(case, "step"));
            assert!((v - num(case, "value")).abs() < 1e-9, "{case}");
        }
    }

    /// The first (module, setting) whose kind matches.
    fn find(pred: fn(&Kind) -> bool) -> Slot {
        for (i, m) in MODULES.iter().enumerate() {
            if let Some(j) = m.settings.iter().position(|d| pred(&d.kind)) {
                return Slot::Own(Owner::Module(i), j);
            }
        }
        panic!("no such setting")
    }

    #[test]
    fn widgets_change_values() {
        let mut h = fresh();
        let mut g = Gui::default();
        // number: drag to the middle, reset
        let n = find(|k| matches!(k, Kind::Number { .. }));
        let Kind::Number { min, max, step, .. } = n.def().kind else {
            unreachable!()
        };
        g.mx = 150;
        g.act(&mut h, Action::Slider(n, 100, 200, None), 0);
        let want = setting::from_fraction(0.5, min, max, step);
        assert_eq!(core::read(&h, |s| n.get(s)), Value::Num(want));
        g.mx = 300;
        g.input(&mut h, Input::Release(0), 300, 0);
        assert!(g.drag.is_none());
        g.act(&mut h, Action::Reset(n), 0);
        assert!(core::read(&h, |s| n.def().is_default(&n.get(s))));
        // enum: forward, then right click back
        let e = find(|k| matches!(k, Kind::Enum { .. }));
        let before = core::read(&h, |s| e.get(s));
        g.act(&mut h, Action::Setting(e), 0);
        assert_ne!(core::read(&h, |s| e.get(s)), before);
        g.act(&mut h, Action::Setting(e), 1);
        assert_eq!(core::read(&h, |s| e.get(s)), before);
        // bool
        let b = find(|k| matches!(k, Kind::Bool(_)));
        let on = core::read(&h, |s| b.get(s).on());
        g.act(&mut h, Action::Setting(b), 0);
        assert_eq!(core::read(&h, |s| b.get(s).on()), !on);
        // module key: listen, press F, then Esc unbinds
        let k = Slot::Key(index("fullbright"));
        g.act(&mut h, Action::Setting(k), 0);
        assert_eq!(g.listening, Some(k));
        assert!(g.key(&mut h, 70, false));
        assert_eq!(core::read(&h, |s| k.get(s)), Value::Key(70));
        g.act(&mut h, Action::Setting(k), 0);
        g.key(&mut h, KEY_ESCAPE, false);
        assert_eq!(core::read(&h, |s| k.get(s)), Value::Key(NONE));
        // colour channel
        let c = find(|k| matches!(k, Kind::Color(_)));
        g.act(&mut h, Action::Setting(c), 0);
        assert!(g.open_colors.contains(&c));
        g.mx = 100;
        g.act(&mut h, Action::Slider(c, 100, 200, Some(24)), 0);
        let Value::Color(argb) = core::read(&h, |s| c.get(s)) else {
            panic!()
        };
        assert_eq!(setting::channel(argb, 24), 0);
    }

    #[test]
    fn expanded_module_shows_rows_and_advanced() {
        let mut h = fresh();
        let i = index("zoom");
        toggle_expanded(i);
        keep(|k| k.tab = Tab::Category(MODULES[i].category));
        let f = frame(&mut h, 854, 480, 0, 0);
        let t = texts(&f.ops);
        assert!(t.contains(&"Advanced".to_owned()), "{t:?}");
        for d in MODULES[i].settings {
            if !matches!(d.kind, Kind::Action) {
                assert!(t.contains(&d.name.to_owned()), "{} in {t:?}", d.name);
            }
        }
        keep(|k| k.advanced.push(MODULES[i].id));
        let t = texts(&frame(&mut h, 854, 480, 0, 0).ops);
        assert!(t.contains(&core::SAFE_BLOCK.name.to_owned()), "{t:?}");
    }

    #[test]
    fn settings_and_profiles_tabs() {
        let mut h = fresh();
        keep(|k| k.tab = Tab::Settings);
        let t = texts(&frame(&mut h, 854, 480, 0, 0).ops);
        assert!(
            t.contains(&GLOBALS[core::G_GUI_KEY].name.to_owned()),
            "{t:?}"
        );
        assert!(t.contains(&"Right Shift".to_owned()));
        assert!(t.contains(&"Save config".to_owned()));
        keep(|k| k.tab = Tab::Profiles);
        let t = texts(&frame(&mut h, 854, 480, 0, 0).ops);
        assert!(t.contains(&"Save current".to_owned()), "{t:?}");
    }

    /// Renders, then clicks the first text `label` with `button`.
    fn press(h: &mut Fake, label: &str, button: i32) {
        let f = frame(h, 854, 480, 0, 0);
        let (x, y) = f
            .ops
            .iter()
            .find_map(|o| match o {
                Op::Text { text, x, y, .. } if text == label => Some((*x, *y)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no {label:?} in {:?}", texts(&f.ops)));
        push(Input::Click(button));
        frame(h, 854, 480, x + 1, y);
    }

    fn steps_of(h: &Fake) -> Vec<String> {
        core::read(h, |s| s.bindings[0].steps.clone())
    }

    #[test]
    fn workflow_editor_creates_edits_and_deletes() {
        let mut h = fresh();
        keep(|k| k.tab = Tab::Workflows);
        let t = texts(&frame(&mut h, 854, 480, 0, 0).ops);
        assert!(t.contains(&"+ New workflow".to_owned()), "{t:?}");
        press(&mut h, "+ New workflow", 0);
        assert_eq!(core::read(&h, |s| s.bindings[0].name.clone()), "Workflow 1");
        // the new workflow is open: empty list, Add step opens the picker
        let t = texts(&frame(&mut h, 854, 480, 0, 0).ops);
        assert!(t.contains(&"No steps yet - add the first one.".to_owned()));
        press(&mut h, "+ Add step", 0);
        assert!(OPEN.with(|o| o.borrow().as_ref().unwrap().dropdown.is_some()));
        // first entry after the "Modules & settings" header: Toggle module
        let (x, top) = OPEN.with(|o| {
            let g = o.borrow();
            let d = g.as_ref().unwrap().dropdown.clone().unwrap();
            (d.x, d.top(480))
        });
        let f = frame(&mut h, 854, 480, 0, 0);
        assert!(texts(&f.ops).contains(&"Modules & settings".to_owned()));
        push(Input::Click(0));
        frame(&mut h, 854, 480, x + 5, top + 12 + 3);
        assert_eq!(steps_of(&h), [format!("toggle:{}", MODULES[0].id)]);
        assert!(OPEN.with(|o| o.borrow().as_ref().unwrap().dropdown.is_none()));
        // a header row keeps the list open, Esc closes it only
        press(&mut h, "+ Add step", 0);
        push(Input::Click(0));
        frame(&mut h, 854, 480, x + 5, top + 3);
        assert!(OPEN.with(|o| o.borrow().as_ref().unwrap().dropdown.is_some()));
        push(Input::Key(KEY_ESCAPE, false));
        let f = frame(&mut h, 854, 480, 0, 0);
        assert!(!f.close);
        assert!(OPEN.with(|o| o.borrow().as_ref().unwrap().dropdown.is_none()));
        // an if step brings its end
        core::with(&mut h, |s, _| {
            s.bindings[0].steps = vec!["repeat:3".into(), "jump".into()];
        });
        let ids: Vec<String> = Vec::new();
        let _ = ids;
        core::with(&mut h, |s, _| {
            s.bindings[0].steps = vec!["wait:1".into()];
        });
        let uid = core::read(&h, |s| s.bindings[0].uid);
        let mut gui = OPEN.with(|o| o.borrow_mut().take().unwrap());
        let d = Dropdown {
            x: 0,
            y: 0,
            w: 100,
            labels: vec!["if".into()],
            headers: Vec::new(),
            hints: Vec::new(),
            scroll: 0,
            target: Pick::StepType(uid, 0, vec!["if".into()]),
        };
        gui.pick(&mut h, &d, 0);
        assert_eq!(steps_of(&h), ["if:health < 10", "end"]);
        OPEN.with(|o| *o.borrow_mut() = Some(gui));
        // move down, remove
        core::with(&mut h, |s, _| {
            s.bindings[0].steps = vec!["jump".into(), "close".into()];
        });
        let mut gui = OPEN.with(|o| o.borrow_mut().take().unwrap());
        gui.act(&mut h, Action::StepMove(uid, 0, 1), 0);
        assert_eq!(steps_of(&h), ["close", "jump"]);
        gui.act(&mut h, Action::StepRemove(uid, 0), 0);
        assert_eq!(steps_of(&h), ["jump"]);
        gui.act(&mut h, Action::DropToggle(uid, 0), 0);
        gui.act(&mut h, Action::WfDelete(uid), 0);
        assert_eq!(
            core::read(&h, |s| s.bindings.len()),
            1,
            "needs confirmation"
        );
        gui.act(&mut h, Action::WfDelete(uid), 0);
        assert_eq!(core::read(&h, |s| s.bindings.len()), 0);
        OPEN.with(|o| *o.borrow_mut() = Some(gui));
    }

    #[test]
    fn workflow_fields_write_through() {
        let mut h = fresh();
        keep(|k| k.tab = Tab::Workflows);
        core::with(&mut h, |s, _| {
            let mut b = workflows::Binding::new("A");
            b.steps = vec!["chat:hi".into(), "craft:stick 4".into()];
            s.bindings.push(b);
        });
        frame(&mut h, 854, 480, 0, 0);
        let uid = core::read(&h, |s| s.bindings[0].uid);
        let mut gui = OPEN.with(|o| o.borrow_mut().take().unwrap());
        // the open workflow's field starts with the step's argument
        gui.focused = Some(FieldId::Named(NameKey::WfArg(uid, 0)));
        gui.char_typed(&mut h, 'x');
        assert_eq!(steps_of(&h)[0], "chat:hix");
        // drop keeps the rest of a craft argument
        gui.act(&mut h, Action::DropToggle(uid, 1), 0);
        assert_eq!(steps_of(&h)[1], "craft:stick 4 drop");
        apply_named(&mut h, NameKey::WfArg(uid, 1), "torch all");
        assert_eq!(steps_of(&h)[1], "craft:torch all drop");
        apply_named(&mut h, NameKey::WfName(uid), "B");
        assert_eq!(core::read(&h, |s| s.bindings[0].name.clone()), "B");
        OPEN.with(|o| *o.borrow_mut() = Some(gui));
    }

    #[test]
    fn waypoints_and_inventory_tabs() {
        let mut h = fresh();
        keep(|k| k.tab = Tab::Waypoints);
        let t = texts(&frame(&mut h, 854, 480, 0, 0).ops);
        assert!(t.contains(&"Join a world to manage waypoints.".to_owned()));
        keep(|k| k.tab = Tab::Inventory);
        let t = texts(&frame(&mut h, 854, 480, 0, 0).ops);
        assert!(
            t.iter()
                .any(|x| x.starts_with("Join a world to use hotbar"))
        );
        // with a world: add, rename, delete
        let dir = std::env::temp_dir().join(format!("ot-gui-wp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        h.dir = Some(dir.clone());
        h.player = Some(core::Player {
            x: 1.5,
            y: 64.0,
            z: 2.5,
            yaw: 0.0,
            pitch: 0.0,
            dimension: "minecraft:overworld".into(),
        });
        keep(|k| k.tab = Tab::Waypoints);
        press(&mut h, "Add here", 0);
        assert_eq!(wps::all(&h).len(), 1);
        let t = texts(&frame(&mut h, 854, 480, 0, 0).ops);
        assert!(
            t.iter().any(|x| x.starts_with("1 64 2  overworld")),
            "{t:?}"
        );
        apply_named(&mut h, NameKey::WpName(0), "Home");
        assert_eq!(wps::all(&h)[0].name(), "Home");
        press(&mut h, "Delete", 0);
        assert!(wps::all(&h).is_empty());
        keep(|k| k.tab = Tab::Inventory);
        let t = texts(&frame(&mut h, 854, 480, 0, 0).ops);
        assert!(t.contains(&"Save hotbar".to_owned()), "{t:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dropdown_wheel_scrolls_the_list() {
        let mut h = fresh();
        keep(|k| k.tab = Tab::Workflows);
        // wheel with a list open scrolls the list, not the page
        frame(&mut h, 854, 480, 0, 0);
        let mut gui = OPEN.with(|o| o.borrow_mut().take().unwrap());
        gui.screen = (854, 480);
        let mut d = flow::Dropdown {
            x: 0,
            y: 0,
            w: 50,
            labels: (0..80).map(|i| i.to_string()).collect(),
            headers: Vec::new(),
            hints: Vec::new(),
            scroll: 0,
            target: Pick::Mode(0),
        };
        d.wheel(-1.0, 480);
        gui.dropdown = Some(d);
        OPEN.with(|o| *o.borrow_mut() = Some(gui));
        push(Input::Scroll(-1.0));
        frame(&mut h, 854, 480, 0, 0);
        let s = OPEN.with(|o| {
            o.borrow()
                .as_ref()
                .unwrap()
                .dropdown
                .as_ref()
                .unwrap()
                .scroll
        });
        assert_eq!(s, 2);
        assert_eq!(keep(|k| k.scroll(Tab::Workflows)), 0);
    }
}
