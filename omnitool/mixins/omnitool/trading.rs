// Trading logic (docs/spec/omnitool/automation.md, section 5; Java's
// core/Trading): the `trade:` / `villager:` argument syntax, the editor's
// coordinate helpers, villager filters, the offer search and the trade run
// at the merchant menu (spec 5.5, `Trading.tick` and `Interact`). The game is
// behind `Trader` (glue.rs). Checked against tests/fixtures/trading.json.
// Added to the game as `game::omnitool_trading`; also a module of this crate.

use super::omnitool_workflows::{Action, Input, item_matches, jtrim, tokens};
use std::collections::HashSet;

/// Java's `Trading.Trade`.
#[derive(Clone, Debug, PartialEq)]
pub struct Trade {
    pub item: String,
    /// The item is what the villager takes (`sell`).
    pub sell: bool,
    /// 0 = one trade.
    pub amount: i32,
    pub all: bool,
    pub from: String,
    /// Block centre of the villager (`at x y z`).
    pub at: Option<[f64; 3]>,
    pub drop: bool,
    /// `villager:` step: the item comes from the profession's setting.
    pub preset: bool,
}

fn is_number(t: &str) -> bool {
    !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit())
}

/// Java's `Double.parseDouble` for the three coordinates (no trailing `d`/`f`
/// suffix handling: such a token fails here and stays an item word).
fn parse_double(t: &str) -> Option<f64> {
    let t = jtrim(t);
    if t.is_empty() || t.ends_with(['d', 'D', 'f', 'F']) || t.contains(['_', 'x', 'X']) {
        return None;
    }
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// `Trading.parse`.
pub fn parse(arg: &str) -> Trade {
    let lower = jtrim(arg).to_lowercase();
    let t: Vec<&str> = tokens(&lower).collect();
    let mut trade = Trade {
        item: String::new(),
        sell: false,
        amount: 0,
        all: false,
        from: String::new(),
        at: None,
        drop: false,
        preset: false,
    };
    let mut words: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let tok = t[i];
        match tok {
            "sell" | "verkaufen" => trade.sell = true,
            "buy" | "kaufen" => trade.sell = false,
            "all" | "max" | "alle" => trade.all = true,
            "drop" => trade.drop = true,
            "from" | "von" => {
                if i + 1 < t.len() {
                    i += 1;
                    trade.from = t[i].to_owned();
                }
            }
            "at" | "bei" => {
                if i + 3 < t.len() {
                    let xyz = (
                        parse_double(t[i + 1]),
                        parse_double(t[i + 2]),
                        parse_double(t[i + 3]),
                    );
                    if let (Some(x), Some(y), Some(z)) = xyz {
                        trade.at = Some([x + 0.5, y, z + 0.5]);
                        i += 3;
                    }
                }
            }
            _ => {
                let digits = tok.strip_suffix('x').unwrap_or(tok);
                if is_number(digits) {
                    // Java throws on a too big number; clamp instead.
                    trade.amount = digits.parse().unwrap_or(i32::MAX);
                } else {
                    words.push(tok);
                }
            }
        }
        i += 1;
    }
    trade.item = words.join(" ");
    trade
}

/// `Trading.preset`: the `villager:` argument; only `at` and `drop` count.
pub fn preset(arg: &str) -> Trade {
    let a = jtrim(arg).to_lowercase();
    let coords = a.starts_with(['-', '~']) || a.starts_with(|c: char| c.is_ascii_digit());
    let p = if coords {
        parse(&format!("at {a}"))
    } else {
        parse(&a)
    };
    Trade {
        item: String::new(),
        sell: false,
        amount: 0,
        all: true,
        from: String::new(),
        at: p.at,
        drop: p.drop,
        preset: true,
    }
}

fn is_coord(t: &str) -> bool {
    // -?\d+(\.\d+)?
    let t = t.strip_prefix('-').unwrap_or(t);
    match t.split_once('.') {
        Some((a, b)) => is_number(a) && is_number(b),
        None => is_number(t),
    }
}

fn is_at(t: &str) -> bool {
    t.eq_ignore_ascii_case("at") || t.eq_ignore_ascii_case("bei")
}

/// `Trading.coords`: the `x y z` after `at`, or "".
#[allow(dead_code)] // the workflow editor uses it
pub fn coords(arg: &str) -> String {
    let t: Vec<&str> = tokens(arg).collect();
    if t.first().is_none_or(|f| !is_at(f)) {
        // Java looks for the first `at` anywhere.
        let Some(p) = t.iter().position(|x| is_at(x)) else {
            return String::new();
        };
        return take_coords(&t[p + 1..]);
    }
    take_coords(&t[1..])
}

fn take_coords(t: &[&str]) -> String {
    t.iter()
        .take(3)
        .take_while(|x| is_coord(x))
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `Trading.withoutCoords`: the argument without any `at x y z`.
pub fn without_coords(arg: &str) -> String {
    let t: Vec<&str> = tokens(arg).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if is_at(t[i]) {
            let mut n = 0;
            while n < 3 && i + 1 < t.len() && is_coord(t[i + 1]) {
                i += 1;
                n += 1;
            }
        } else {
            out.push(t[i]);
        }
        i += 1;
    }
    out.join(" ")
}

/// `Trading.withCoords`.
pub fn with_coords(rest: &str, coords: &str) -> String {
    let (rest, coords) = (jtrim(rest), jtrim(coords));
    if coords.is_empty() {
        rest.to_owned()
    } else if rest.is_empty() {
        format!("at {coords}")
    } else {
        format!("{rest} at {coords}")
    }
}

/// A villager's display name as a text tree (Java `Component`): a translatable
/// key or literal text, plus siblings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NameText {
    pub translatable: Option<String>,
    pub siblings: Vec<NameText>,
}

/// `Trading.professionOf`: the last part of the translation key, searched in
/// the siblings too; "" for custom names.
pub fn profession_of(name: &NameText) -> String {
    if let Some(key) = &name.translatable {
        return key
            .rsplit_once('.')
            .map_or(key.as_str(), |(_, k)| k)
            .to_owned();
    }
    name.siblings
        .iter()
        .map(profession_of)
        .find(|p| !p.is_empty())
        .unwrap_or_default()
}

/// `Trading.isTrader`: a villager or wandering trader that `from` selects.
pub fn is_trader(entity_type: &str, profession: &str, shown_name: &str, from: &str) -> bool {
    if entity_type != "minecraft:villager" && entity_type != "minecraft:wandering_trader" {
        return false;
    }
    from.is_empty()
        || profession.eq_ignore_ascii_case(from)
        || entity_type.ends_with(from)
        || item_matches(from, profession, shown_name)
}

/// `Trading.resolvePreset` for the preset text of a profession (None = no
/// module or no text); `request_*` come from the `villager:` step,
/// `module_drop` is the module's `drop` setting.
pub fn resolve_preset(
    text: &str,
    request_at: Option<[f64; 3]>,
    request_drop: bool,
    module_drop: bool,
) -> Option<Trade> {
    let text = jtrim(text);
    if text.is_empty() {
        return None;
    }
    let t = parse(text);
    if t.item.is_empty() {
        return None;
    }
    Some(Trade {
        all: t.all || t.amount == 0,
        from: String::new(),
        at: request_at,
        drop: t.drop || request_drop || module_drop,
        preset: false,
        ..t
    })
}

/// One offer of a merchant, as `find_offer` needs it.
#[derive(Clone, Debug, Default)]
pub struct Offer {
    pub cost_a: Stack,
    pub cost_b: Stack,
    pub result: Stack,
}

/// An item stack as a name for matching: id, shown name and the text form of
/// its components. An empty `id` is an empty stack.
#[derive(Clone, Debug, Default)]
pub struct Stack {
    pub id: String,
    pub name: String,
    pub components: String,
}

impl Stack {
    fn matches(&self, query: &str) -> bool {
        !self.id.is_empty() && item_matches(query, &self.id, &self.name)
    }

    /// `Trading.hasEnchantment`: the component text mentions an enchantment
    /// and the query as a substring.
    fn has_enchantment(&self, query: &str) -> bool {
        let q = jtrim(query).to_lowercase().replace(' ', "_");
        if q.chars().count() < 4 || self.id.is_empty() {
            return false;
        }
        let c = self.components.to_lowercase();
        let want = if q.contains(':') {
            q
        } else {
            format!("minecraft:{q}")
        };
        c.contains("enchantment") && c.contains(&want)
    }
}

/// `Trading.findOffer`: the index of the first offer with the item.
pub fn find_offer(offers: &[Offer], item: &str, sell: bool) -> Option<usize> {
    offers.iter().position(|o| {
        if sell {
            o.cost_a.matches(item) || o.cost_b.matches(item)
        } else {
            o.result.matches(item) || o.result.has_enchantment(item)
        }
    })
}

// ------------------------------------------------------------ the trade run

/// A villager or wandering trader near the player, as glue.rs sees it.
#[derive(Clone, Debug, Default)]
pub struct TraderEntity {
    pub id: u32,
    pub type_id: String,
    pub name: NameText,
    /// The shown name (custom name or the type's name).
    pub shown: String,
    /// Feet position.
    pub pos: [f64; 3],
    /// Centre of the bounding box (where `Interact` looks).
    pub center: [f64; 3],
    /// Within interaction reach of the eyes with no block in between.
    pub visible: bool,
}

impl TraderEntity {
    fn block(&self) -> [i32; 3] {
        self.pos.map(|v| v.floor() as i32)
    }
    fn block_text(&self) -> String {
        let [x, y, z] = self.block();
        format!("{x} {y} {z}")
    }
}

/// A menu slot's stack; `count` 0 = empty.
#[derive(Clone, Debug, Default)]
pub struct Slot {
    pub stack: Stack,
    pub count: i32,
    pub max_stack: i32,
}

impl Slot {
    fn empty(&self) -> bool {
        self.count <= 0 || self.stack.id.is_empty()
    }
    /// `ItemStack.isSameItemSameComponents`.
    fn same(&self, other: &Slot) -> bool {
        self.stack.id == other.stack.id && self.stack.components == other.stack.components
    }
}

/// One offer of the open merchant menu.
#[derive(Clone, Debug, Default)]
pub struct MenuOffer {
    pub offer: Offer,
    /// The result's count per trade.
    pub result_count: i32,
    pub out_of_stock: bool,
}

/// The open merchant menu (Java's `MerchantMenu` with its screen).
#[derive(Clone, Debug, Default)]
pub struct MerchantView {
    pub offers: Vec<MenuOffer>,
    /// The screen title (the merchant's name).
    pub title: NameText,
    /// Menu slot 2.
    pub result: Slot,
    pub carried: Slot,
    /// Menu slots 3 to the end (the player inventory), in menu order.
    pub inventory: Vec<Slot>,
}

/// What trading needs from the game; glue.rs implements it on `Game`.
pub trait Trader {
    /// An action bar hint.
    fn hint(&mut self, text: &str);
    /// Living traders within the player's box grown by `grow` blocks.
    fn traders(&self, grow: f64) -> Vec<TraderEntity>;
    /// The trader with this id while it is alive.
    fn trader(&self, id: u32) -> Option<TraderEntity>;
    /// Java's `entityInteractionRange`.
    fn interaction_range(&self) -> f64;
    /// The entity the crosshair ray hits first within interaction reach
    /// (no block in between).
    fn crosshair_entity(&self) -> Option<u32>;
    /// The open merchant menu.
    fn merchant(&self) -> Option<MerchantView>;
    /// How many items with this id the player inventory holds (all slots).
    fn count(&self, id: &str) -> i32;
    /// The vanilla trade button: selection hint, `tryMoveItems`, then
    /// `ServerboundSelectTradePacket`.
    fn select_trade(&mut self, index: usize);
    /// A click on a merchant menu slot (None = outside, slot -999); `quick`
    /// = Shift (`QUICK_MOVE`), else a left click (`PICKUP`).
    fn click(&mut self, slot: Option<usize>, quick: bool);
    /// The Villager Trades text of a profession and the module's `drop`
    /// setting; None when the module isn't loaded.
    fn preset(&self, profession: &str) -> Option<(String, bool)>;
}

/// Java's `lookAt` (yaw, pitch) from `eye` to `to`.
pub fn look_at(eye: [f64; 3], to: [f64; 3]) -> (f32, f32) {
    let (dx, dy, dz) = (to[0] - eye[0], to[1] - eye[1], to[2] - eye[2]);
    let h = (dx * dx + dz * dz).sqrt();
    let yaw = dz.atan2(dx).to_degrees() - 90.0;
    let pitch = -dy.atan2(h).to_degrees();
    (yaw as f32, pitch as f32)
}

fn dist2(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum()
}

/// `Trading.targetCoords`: the trader under the crosshair, else the nearest
/// within 8 blocks; its block position as `x y z`.
pub fn target_coords(t: &dyn Trader, player: [f64; 3]) -> Option<String> {
    if let Some(e) = t
        .crosshair_entity()
        .and_then(|id| t.trader(id))
        .filter(|e| is_trader(&e.type_id, &profession_of(&e.name), &e.shown, ""))
    {
        return Some(e.block_text());
    }
    t.traders(8.0)
        .into_iter()
        .filter(|e| is_trader(&e.type_id, &profession_of(&e.name), &e.shown, ""))
        .map(|e| (dist2(e.pos, player), e))
        .filter(|(d, _)| *d < 64.0)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, e)| e.block_text())
}

/// `Interact.findEntity`: the nearest visible candidate (to `near`, else to
/// the eyes); with `near` only those within 2 blocks of it.
pub fn find_entity(
    t: &mut dyn Trader,
    eye: [f64; 3],
    label: &str,
    filter: &dyn Fn(&TraderEntity) -> bool,
    near: Option<[f64; 3]>,
) -> Option<TraderEntity> {
    let grow = t.interaction_range() + 2.0;
    let mut found: Vec<(f64, TraderEntity)> = t
        .traders(grow)
        .into_iter()
        .filter(|e| filter(e))
        .map(|e| (dist2(e.pos, near.unwrap_or(eye)), e))
        .filter(|(d, _)| near.is_none() || *d <= 4.0)
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, e)) = found.iter().find(|(_, e)| e.visible) {
        return Some(e.clone());
    }
    t.hint(&if found.is_empty() {
        format!("Omnitool: no {label} within reach")
    } else {
        format!("Omnitool: {label} is not visible from here (blocked)")
    });
    None
}

/// Java's `Interact` for an entity: turn to it for two ticks, then right
/// click once the real crosshair ray hits it first.
#[derive(Debug)]
pub struct Interact {
    pub id: u32,
    ticks: i32,
}

impl Interact {
    pub fn new(id: u32) -> Interact {
        Interact { id, ticks: 0 }
    }

    /// 1 = clicked, 0 = still turning, -1 = gone or not reachable.
    pub fn tick(&mut self, input: &mut dyn Input) -> i32 {
        let Some(e) = input.trader().and_then(|t| t.trader(self.id)) else {
            return -1;
        };
        let (yaw, pitch) = look_at(input.eye(), e.center);
        input.set_rotation(yaw, pitch);
        self.ticks += 1;
        if self.ticks > 2
            && input
                .trader()
                .and_then(|t| t.crosshair_entity())
                .is_some_and(|id| id == self.id)
        {
            input.click(false);
            return 1;
        }
        if self.ticks > 8 {
            hint(
                input,
                "Omnitool: villager is not visible from here (blocked)",
            );
            return -1;
        }
        0
    }
}

fn with(input: &mut dyn Input, f: impl FnOnce(&mut dyn Trader)) {
    if let Some(t) = input.trader() {
        f(t);
    }
}

fn hint(input: &mut dyn Input, text: &str) {
    with(input, |t| t.hint(text));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Select,
    WaitResult,
    AfterTake,
}

/// The `trade` / `villager` step (Java's `Trading`).
pub struct Trading {
    request: Trade,
    trade: Trade,
    target: Option<Interact>,
    /// The target's entity, for its name and id.
    entity: Option<TraderEntity>,
    clicked: bool,
    opened: bool,
    wait: i32,
    timeout: i32,
    offer: Option<usize>,
    start: i32,
    tried: HashSet<u32>,
    phase: Phase,
    quick_moved: bool,
}

impl Trading {
    pub fn new(request: Trade) -> Trading {
        Trading {
            trade: request.clone(),
            request,
            target: None,
            entity: None,
            clicked: false,
            opened: false,
            wait: 0,
            timeout: 0,
            offer: None,
            start: -1,
            tried: HashSet::new(),
            phase: Phase::Select,
            quick_moved: false,
        }
    }

    fn close(input: &mut dyn Input) -> bool {
        input.close_screen();
        true
    }

    fn no_menu(&mut self, input: &mut dyn Input) -> bool {
        if self.opened {
            return true;
        }
        if self.target.is_none() {
            let eye = input.eye();
            let Some(t) = input.trader() else { return true };
            let req = &self.request;
            let label = if req.from.is_empty() {
                "villager".to_owned()
            } else {
                req.from.clone()
            };
            let (tried, preset) = (&self.tried, req.preset);
            let presets: Vec<(u32, bool)> = if preset {
                t.traders(t.interaction_range() + 2.0)
                    .iter()
                    .map(|e| {
                        let p = profession_of(&e.name);
                        let has = t
                            .preset(&p)
                            .is_some_and(|(text, _)| !jtrim(&text).is_empty());
                        (e.id, has)
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let filter = |e: &TraderEntity| {
                is_trader(&e.type_id, &profession_of(&e.name), &e.shown, &req.from)
                    && !tried.contains(&e.id)
                    && (!preset || presets.iter().any(|&(id, has)| id == e.id && has))
            };
            let Some(e) = find_entity(t, eye, &label, &filter, req.at) else {
                if !self.tried.is_empty() {
                    let what = if preset {
                        "its preferred trade".to_owned()
                    } else {
                        format!("a trade for {}", req.item)
                    };
                    t.hint(&format!("Omnitool: no villager in reach has {what}"));
                }
                return true;
            };
            t.hint(&format!(
                "Omnitool: trading with {} at {}",
                e.shown,
                e.block_text()
            ));
            self.target = Some(Interact::new(e.id));
            self.entity = Some(e);
            self.clicked = false;
            self.timeout = 40;
        }
        if !self.clicked {
            let r = self.target.as_mut().map_or(-1, |t| t.tick(input));
            if r < 0 {
                return true;
            }
            self.clicked = r > 0;
            return false;
        }
        let out = self.timeout <= 0;
        self.timeout -= 1;
        out
    }

    /// Chooses the offer; Some(done) ends the tick.
    fn choose(&mut self, input: &mut dyn Input, m: &MerchantView) -> Option<bool> {
        let t = input.trader()?;
        if self.request.preset {
            let prof = match &self.entity {
                Some(e) => profession_of(&e.name),
                None => profession_of(&m.title),
            };
            let resolved = t.preset(&prof).and_then(|(text, module_drop)| {
                resolve_preset(&text, self.request.at, self.request.drop, module_drop)
            });
            match resolved {
                Some(tr) => self.trade = tr,
                None => {
                    t.hint(&if prof.is_empty() {
                        "Omnitool: unknown villager profession (renamed?)".to_owned()
                    } else {
                        format!(
                            "Omnitool: no preferred trade set for {prof} (Villager Trades module)"
                        )
                    });
                    return Some(Self::close(input));
                }
            }
        }
        let offers: Vec<Offer> = m.offers.iter().map(|o| o.offer.clone()).collect();
        match find_offer(&offers, &self.trade.item, self.trade.sell) {
            Some(i) => {
                self.offer = Some(i);
                self.start = t.count(&m.offers[i].offer.result.id);
                None
            }
            None if self.request.at.is_some() || self.entity.is_none() => {
                t.hint(&format!(
                    "Omnitool: this villager has no trade for {}",
                    self.trade.item
                ));
                Some(Self::close(input))
            }
            None => {
                if let Some(e) = self.entity.take() {
                    self.tried.insert(e.id);
                }
                input.close_screen();
                self.opened = false;
                self.target = None;
                Some(self.tried.len() > 8)
            }
        }
    }

    /// `take(menu)`.
    fn take(&mut self, input: &mut dyn Input) {
        if self.trade.all {
            with(input, |t| t.click(Some(2), true));
            self.quick_moved = true;
            return;
        }
        with(input, |t| t.click(Some(2), false));
        Self::place(input);
    }

    /// The cursor's stack into the inventory: the first slot with the same
    /// stack and room for all of it, else the first empty one. False when
    /// nothing fits. A server's click shows on the cursor only once it
    /// answers, so `AfterTake` tries again with what the cursor holds.
    fn place(input: &mut dyn Input) -> bool {
        let Some(t) = input.trader() else {
            return false;
        };
        let Some(m) = t.merchant() else { return false };
        let c = &m.carried;
        if c.empty() {
            return false;
        }
        let slot = m
            .inventory
            .iter()
            .position(|s| !s.empty() && s.same(c) && s.count + c.count <= s.max_stack)
            .or_else(|| m.inventory.iter().position(Slot::empty));
        if let Some(i) = slot {
            t.click(Some(3 + i), false);
        }
        slot.is_some()
    }

    fn full(&mut self, input: &mut dyn Input) -> bool {
        hint(input, "Omnitool: inventory full - stopped trading");
        Self::close(input)
    }

    fn menu(&mut self, input: &mut dyn Input, m: MerchantView) -> bool {
        self.opened = true;
        if self.wait > 0 {
            self.wait -= 1;
            return false;
        }
        if m.offers.is_empty() {
            return false;
        }
        if self.offer.is_none()
            && let Some(done) = self.choose(input, &m)
        {
            return done;
        }
        let Some(index) = self.offer else { return true };
        let Some(o) = m.offers.get(index) else {
            return Self::close(input);
        };
        let Some(count) = input.trader().map(|t| t.count(&o.offer.result.id)) else {
            return true;
        };
        let received = count - self.start;
        let trades = if o.result_count == 0 {
            0
        } else {
            received / o.result_count
        };
        let goal = self.trade.amount.max(1);
        let ready = !m.result.empty()
            && item_matches(&o.offer.result.id, &m.result.stack.id, &m.result.stack.name);
        let reached = !self.trade.all && trades >= goal;
        match self.phase {
            Phase::Select => {
                if reached {
                    return Self::close(input);
                }
                if o.out_of_stock {
                    if trades == 0 {
                        hint(input, "Omnitool: this trade is sold out");
                    }
                    return Self::close(input);
                }
                with(input, |t| t.select_trade(index));
                self.phase = Phase::WaitResult;
                self.timeout = 20;
                self.wait = 1;
            }
            Phase::WaitResult => {
                if reached {
                    return Self::close(input);
                }
                if ready {
                    self.take(input);
                    self.phase = Phase::AfterTake;
                    self.wait = 2;
                } else {
                    let out = self.timeout <= 0;
                    self.timeout -= 1;
                    if out {
                        if trades == 0 {
                            hint(input, "Omnitool: not enough items for this trade");
                        }
                        return Self::close(input);
                    }
                }
            }
            Phase::AfterTake => {
                let drop = self.trade.drop;
                if !m.carried.empty() && !self.trade.all && Self::place(input) {
                    self.wait = 2;
                } else if !m.carried.empty() {
                    if !drop {
                        return self.full(input);
                    }
                    with(input, |t| t.click(None, false));
                    self.wait = 2;
                } else if self.trade.all && ready && self.quick_moved {
                    if !drop {
                        return self.full(input);
                    }
                    with(input, |t| t.click(Some(2), false));
                    with(input, |t| t.click(None, false));
                    self.wait = 2;
                } else {
                    self.phase = if ready {
                        Phase::WaitResult
                    } else {
                        Phase::Select
                    };
                    self.timeout = 20;
                }
                self.quick_moved = false;
            }
        }
        false
    }
}

impl Action for Trading {
    fn tick(&mut self, input: &mut dyn Input) -> bool {
        let Some(m) = input.trader().map(|t| t.merchant()) else {
            return true;
        };
        match m {
            None => self.no_menu(input),
            Some(m) => self.menu(input, m),
        }
    }
    fn stop(&mut self, _input: &mut dyn Input) {}
    fn in_screen(&self) -> bool {
        true
    }
}

/// The `trade:` / `villager:` step's action; None for a `trade` without an
/// item (no action, as Java).
pub fn action(id: &str, arg: &str) -> Option<Box<dyn Action>> {
    let request = match id {
        "trade" => parse(arg),
        "villager" => preset(arg),
        _ => return None,
    };
    if !request.preset && request.item.is_empty() {
        return None;
    }
    Some(Box::new(Trading::new(request)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixtures() -> Value {
        serde_json::from_str(
            &std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../tests/fixtures/trading.json"
            ))
            .unwrap(),
        )
        .unwrap()
    }

    fn at(v: &Value) -> Option<[f64; 3]> {
        v.as_array()
            .map(|a| [0, 1, 2].map(|i| a[i].as_f64().unwrap()))
    }

    fn check(t: &Trade, e: &Value, ctx: &str) {
        assert_eq!(t.item, e["item"].as_str().unwrap(), "{ctx}");
        assert_eq!(t.sell, e["sell"].as_bool().unwrap(), "{ctx}");
        assert_eq!(i64::from(t.amount), e["amount"].as_i64().unwrap(), "{ctx}");
        assert_eq!(t.all, e["all"].as_bool().unwrap(), "{ctx}");
        assert_eq!(t.from, e["from"].as_str().unwrap(), "{ctx}");
        assert_eq!(t.drop, e["drop"].as_bool().unwrap(), "{ctx}");
        assert_eq!(t.preset, e["preset"].as_bool().unwrap(), "{ctx}");
        match (t.at, at(&e["at"])) {
            (None, None) => {}
            (Some(a), Some(b)) => {
                for i in 0..3 {
                    assert!((a[i] - b[i]).abs() < 1e-9, "{ctx}");
                }
            }
            (a, b) => panic!("{ctx}: at {a:?} vs {b:?}"),
        }
    }

    #[test]
    fn parse_fixtures() {
        for c in fixtures()["parse"].as_array().unwrap() {
            let arg = c["arg"].as_str().unwrap();
            check(&parse(arg), &c["expect"], arg);
        }
    }

    #[test]
    fn preset_fixtures() {
        for c in fixtures()["preset"].as_array().unwrap() {
            let arg = c["arg"].as_str().unwrap();
            check(&preset(arg), &c["expect"], arg);
        }
    }

    #[test]
    fn coord_helpers() {
        let f = fixtures();
        for c in f["coords"].as_array().unwrap() {
            let a = c["arg"].as_str().unwrap();
            assert_eq!(coords(a), c["expect"].as_str().unwrap(), "{a}");
        }
        for c in f["without_coords"].as_array().unwrap() {
            let a = c["arg"].as_str().unwrap();
            assert_eq!(without_coords(a), c["expect"].as_str().unwrap(), "{a}");
        }
        for c in f["with_coords"].as_array().unwrap() {
            let (r, k) = (c["rest"].as_str().unwrap(), c["coords"].as_str().unwrap());
            assert_eq!(with_coords(r, k), c["expect"].as_str().unwrap(), "{r}|{k}");
        }
    }

    #[test]
    fn item_matches_fixtures() {
        for c in fixtures()["item_matches"].as_array().unwrap() {
            let (q, id, n) = (
                c["query"].as_str().unwrap(),
                c["id"].as_str().unwrap(),
                c["name"].as_str().unwrap(),
            );
            assert_eq!(
                item_matches(q, id, n),
                c["expect"].as_bool().unwrap(),
                "{q} {id}"
            );
        }
    }

    #[test]
    fn is_trader_fixtures() {
        for c in fixtures()["is_trader"].as_array().unwrap() {
            let g = |k: &str| c[k].as_str().unwrap();
            assert_eq!(
                is_trader(g("entity_type"), g("profession"), g("name"), g("from")),
                c["expect"].as_bool().unwrap(),
                "{c}"
            );
        }
    }

    fn name(v: &Value) -> NameText {
        NameText {
            translatable: v["translatable"].as_str().map(str::to_owned),
            siblings: v["siblings"]
                .as_array()
                .map_or_else(Vec::new, |a| a.iter().map(name).collect()),
        }
    }

    #[test]
    fn profession_of_fixtures() {
        for c in fixtures()["profession_of"].as_array().unwrap() {
            assert_eq!(
                profession_of(&name(&c["name"])),
                c["expect"].as_str().unwrap(),
                "{c}"
            );
        }
    }

    #[test]
    fn resolve_preset_fixtures() {
        for c in fixtures()["resolve_preset"].as_array().unwrap() {
            let t = resolve_preset(
                c["preset_text"].as_str().unwrap(),
                at(&c["request_at"]),
                c["request_drop"].as_bool().unwrap(),
                c["module_drop"].as_bool().unwrap(),
            );
            match (&t, &c["expect"]) {
                (None, Value::Null) => {}
                (Some(t), e) if !e.is_null() => check(t, e, &c.to_string()),
                _ => panic!("{c}"),
            }
        }
    }

    fn stack(id: &str, components: &str) -> Stack {
        let path = id.split_once(':').map_or(id, |(_, p)| p);
        let name = path
            .split('_')
            .map(|w| {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect::<String>())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ");
        Stack {
            id: id.to_owned(),
            name: if id.is_empty() { String::new() } else { name },
            components: components.to_owned(),
        }
    }

    #[test]
    fn find_offer_fixtures() {
        let f = &fixtures()["find_offer"];
        let offers: Vec<Offer> = f["offers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| Offer {
                cost_a: stack(o["cost_a"].as_str().unwrap(), ""),
                cost_b: stack(o["cost_b"].as_str().unwrap(), ""),
                result: stack(
                    o["result"].as_str().unwrap(),
                    o["result_components"].as_str().unwrap(),
                ),
            })
            .collect();
        for c in f["cases"].as_array().unwrap() {
            let got = find_offer(
                &offers,
                c["item"].as_str().unwrap(),
                c["sell"].as_bool().unwrap(),
            );
            assert_eq!(
                got.map_or(-1, |i| i as i64),
                c["expect"].as_i64().unwrap(),
                "{c}"
            );
        }
    }
    // ---------------------------------------------------------- trade run

    /// A villager with one paying offer per entry: (cost id, cost count,
    /// result id, result count, max uses).
    struct Sim {
        entities: Vec<TraderEntity>,
        crosshair: bool,
        open: Option<u32>,
        offers: Vec<(&'static str, i32, &'static str, i32, i32)>,
        uses: Vec<i32>,
        inv: std::collections::HashMap<String, i32>,
        selected: Option<usize>,
        carried: std::cell::Cell<i32>,
        /// A server: a click shows on the cursor one menu read later.
        server: bool,
        pending: std::cell::Cell<i32>,
        hints: Vec<String>,
        presets: Vec<(&'static str, &'static str)>,
        clicks: Vec<(Option<usize>, bool)>,
    }

    fn villager(id: u32, prof: &str, pos: [f64; 3]) -> TraderEntity {
        TraderEntity {
            id,
            type_id: "minecraft:villager".into(),
            name: NameText {
                translatable: Some(format!("entity.minecraft.villager.{prof}")),
                siblings: Vec::new(),
            },
            shown: "Villager".into(),
            pos,
            center: [pos[0], pos[1] + 1.0, pos[2]],
            visible: true,
        }
    }

    impl Sim {
        fn new() -> Sim {
            let mut inv = std::collections::HashMap::new();
            inv.insert("minecraft:wheat".to_owned(), 64);
            Sim {
                entities: vec![villager(7, "farmer", [2.5, 64.0, 0.5])],
                crosshair: true,
                open: None,
                offers: vec![("minecraft:wheat", 20, "minecraft:emerald", 1, 3)],
                uses: vec![0],
                inv,
                selected: None,
                carried: Default::default(),
                server: false,
                pending: Default::default(),
                hints: Vec::new(),
                presets: vec![("farmer", "emerald")],
                clicks: Vec::new(),
            }
        }
        fn affordable(&self, i: usize) -> bool {
            let o = self.offers[i];
            self.uses[i] < o.4 && self.inv.get(o.0).copied().unwrap_or(0) >= o.1
        }
        fn pay(&mut self, i: usize) {
            let o = self.offers[i];
            *self.inv.get_mut(o.0).unwrap() -= o.1;
            self.uses[i] += 1;
        }
        fn run(&mut self, mut a: Trading) -> i32 {
            for n in 0..400 {
                if a.tick(self) {
                    return n;
                }
            }
            panic!("no end");
        }
    }

    fn plain(id: &str) -> Stack {
        Stack {
            id: id.into(),
            name: String::new(),
            components: String::new(),
        }
    }

    impl Input for Sim {
        fn click(&mut self, attack: bool) {
            if !attack && self.crosshair {
                self.open = Some(7);
            }
        }
        fn close_screen(&mut self) {
            self.open = None;
            self.selected = None;
        }
        fn trader(&mut self) -> Option<&mut dyn Trader> {
            Some(self)
        }
    }

    impl Trader for Sim {
        fn hint(&mut self, text: &str) {
            self.hints.push(text.to_owned());
        }
        fn traders(&self, _grow: f64) -> Vec<TraderEntity> {
            self.entities.clone()
        }
        fn trader(&self, id: u32) -> Option<TraderEntity> {
            self.entities.iter().find(|e| e.id == id).cloned()
        }
        fn interaction_range(&self) -> f64 {
            3.0
        }
        fn crosshair_entity(&self) -> Option<u32> {
            self.crosshair.then_some(7)
        }
        fn merchant(&self) -> Option<MerchantView> {
            self.open?;
            // the first read after the click still shows the old cursor
            let p = self.pending.get();
            if p < 0 {
                self.carried.set(self.carried.get() - p);
                self.pending.set(0);
            } else if p > 0 {
                self.pending.set(-p);
            }
            let result = match self.selected {
                Some(i) if self.affordable(i) => Slot {
                    stack: plain(self.offers[i].2),
                    count: self.offers[i].3,
                    max_stack: 64,
                },
                _ => Slot::default(),
            };
            Some(MerchantView {
                offers: self
                    .offers
                    .iter()
                    .zip(&self.uses)
                    .map(|(o, &u)| MenuOffer {
                        offer: Offer {
                            cost_a: plain(o.0),
                            cost_b: Stack::default(),
                            result: plain(o.2),
                        },
                        result_count: o.3,
                        out_of_stock: u >= o.4,
                    })
                    .collect(),
                title: NameText::default(),
                result,
                carried: if self.carried.get() > 0 {
                    Slot {
                        stack: plain("minecraft:emerald"),
                        count: self.carried.get(),
                        max_stack: 64,
                    }
                } else {
                    Slot::default()
                },
                inventory: vec![Slot::default(); 36],
            })
        }
        fn count(&self, id: &str) -> i32 {
            self.inv.get(id).copied().unwrap_or(0)
        }
        fn select_trade(&mut self, index: usize) {
            self.selected = Some(index);
        }
        fn click(&mut self, slot: Option<usize>, quick: bool) {
            self.clicks.push((slot, quick));
            match (slot, self.selected) {
                (Some(2), Some(i)) if self.affordable(i) => {
                    self.pay(i);
                    let o = self.offers[i];
                    if quick {
                        // vanilla's quick move repeats while it can pay
                        *self.inv.entry(o.2.into()).or_default() += o.3;
                        while self.affordable(i) {
                            self.pay(i);
                            *self.inv.entry(o.2.into()).or_default() += o.3;
                        }
                    } else if self.server {
                        self.pending.set(o.3);
                    } else {
                        self.carried.set(self.carried.get() + o.3);
                    }
                }
                (Some(s), _) if s >= 3 && self.carried.get() > 0 => {
                    *self.inv.entry("minecraft:emerald".into()).or_default() += self.carried.take();
                }
                (None, _) => self.carried.set(0),
                _ => {}
            }
        }
        fn preset(&self, profession: &str) -> Option<(String, bool)> {
            let text = self
                .presets
                .iter()
                .find(|p| p.0 == profession)
                .map_or("", |p| p.1);
            Some((text.to_owned(), false))
        }
    }

    #[test]
    fn trades_everything_and_stops_when_sold_out() {
        let mut sim = Sim::new();
        sim.run(Trading::new(parse("emerald all")));
        assert_eq!(sim.inv["minecraft:emerald"], 3);
        assert_eq!(sim.inv["minecraft:wheat"], 4);
        assert_eq!(sim.open, None);
        assert_eq!(sim.hints[0], "Omnitool: trading with Villager at 2 64 0");
        assert_eq!(sim.hints.len(), 1, "{:?}", sim.hints);
    }

    #[test]
    fn fixed_amount_takes_through_the_cursor() {
        let mut sim = Sim::new();
        sim.run(Trading::new(parse("emerald 2")));
        assert_eq!(sim.inv["minecraft:emerald"], 2);
        assert!(sim.clicks.contains(&(Some(3), false)));
        assert!(!sim.clicks.iter().any(|c| c.1));
        // a server shows the taken result on the cursor only later
        let mut sim = Sim::new();
        sim.server = true;
        sim.run(Trading::new(parse("emerald 2")));
        assert_eq!(sim.inv["minecraft:emerald"], 2);
        assert!(sim.hints.len() == 1, "{:?}", sim.hints);
    }

    #[test]
    fn sold_out_and_missing_items_hint_once() {
        let mut sim = Sim::new();
        sim.uses[0] = 3;
        sim.run(Trading::new(parse("emerald")));
        assert_eq!(
            sim.hints.last().unwrap(),
            "Omnitool: this trade is sold out"
        );
        let mut sim = Sim::new();
        sim.inv.insert("minecraft:wheat".into(), 5);
        sim.run(Trading::new(parse("emerald")));
        assert_eq!(
            sim.hints.last().unwrap(),
            "Omnitool: not enough items for this trade"
        );
    }

    #[test]
    fn fixed_villager_without_the_offer_ends() {
        let mut sim = Sim::new();
        sim.run(Trading::new(parse("diamond at 2 64 0")));
        assert_eq!(
            sim.hints.last().unwrap(),
            "Omnitool: this villager has no trade for diamond"
        );
        assert_eq!(sim.open, None);
    }

    #[test]
    fn nearest_villager_without_the_offer_is_skipped() {
        let mut sim = Sim::new();
        sim.run(Trading::new(parse("diamond")));
        assert_eq!(
            sim.hints.last().unwrap(),
            "Omnitool: no villager in reach has a trade for diamond"
        );
    }

    #[test]
    fn coordinates_only_take_the_villager_there() {
        let mut sim = Sim::new();
        sim.run(Trading::new(parse("emerald at 10 64 10")));
        assert_eq!(sim.hints, ["Omnitool: no villager within reach"]);
        let mut sim = Sim::new();
        sim.entities[0].visible = false;
        sim.run(Trading::new(parse("emerald")));
        assert_eq!(
            sim.hints,
            ["Omnitool: villager is not visible from here (blocked)"]
        );
    }

    #[test]
    fn interact_gives_up_when_the_ray_misses() {
        let mut sim = Sim::new();
        sim.crosshair = false;
        let n = sim.run(Trading::new(parse("emerald")));
        assert_eq!(n, 8);
        assert_eq!(
            sim.hints.last().unwrap(),
            "Omnitool: villager is not visible from here (blocked)"
        );
    }

    #[test]
    fn preset_uses_the_profession_text() {
        let mut sim = Sim::new();
        sim.run(Trading::new(preset("")));
        assert_eq!(sim.inv["minecraft:emerald"], 3);
        let mut sim = Sim::new();
        sim.presets.clear();
        sim.run(Trading::new(preset("")));
        // no farmer text: the villager isn't a candidate
        assert_eq!(sim.hints, ["Omnitool: no villager within reach"]);
    }

    #[test]
    fn look_at_matches_vanilla() {
        let (y, p) = look_at([0.0; 3], [0.0, 0.0, 1.0]);
        assert!((y - 0.0).abs() < 1e-4 && p.abs() < 1e-4);
        let (y, _) = look_at([0.0; 3], [-1.0, 0.0, 0.0]);
        assert!((y - 90.0).abs() < 1e-4);
        let (_, p) = look_at([0.0; 3], [1.0, -1.0, 0.0]);
        assert!((p - 45.0).abs() < 1e-4);
    }

    #[test]
    fn target_coords_prefers_the_crosshair() {
        let mut sim = Sim::new();
        sim.entities.push(villager(8, "mason", [0.5, 64.0, 0.5]));
        assert_eq!(target_coords(&sim, [0.0, 64.0, 0.0]).unwrap(), "2 64 0");
        sim.crosshair = false;
        assert_eq!(target_coords(&sim, [0.0, 64.0, 0.0]).unwrap(), "0 64 0");
        sim.entities.clear();
        assert_eq!(target_coords(&sim, [0.0; 3]), None);
    }
}
