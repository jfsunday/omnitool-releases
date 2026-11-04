// Omnitool in the game: `Host` on Horizon's `Game`, and one dispatcher per
// hook point that the helpers (hook_*.rs) call. The only file with game
// types besides the helpers; not part of the Cargo crate. Added to the game
// as `game::omnitool_glue`.

use super::omnitool_conditions::{Item, World};
use super::omnitool_core::{self as core, Host, Player};
use super::omnitool_hud::{Effect, HudCorner, HudOp, Inventory, Stack, Worn};
use super::omnitool_overlay_entries::{
    self as entries, BlockInfo, EntityInfo, OverlayInfo, Prop, SpawnInfo,
};
use super::omnitool_safe::Server;
use super::omnitool_trading::{
    self as trading, MenuOffer, MerchantView, NameText, Slot, Trader, TraderEntity,
};
use super::omnitool_workflows::Input;
use super::{
    Game, Screen,
    native_hooks::{
        NativeMinimap, NativeMod, NativePanelEvent, NativeRecipeKind, NativeRecipeLayout,
        NativeRecipeView, NativeStack, NativeZoom,
    },
    omnitool_clear_water, omnitool_clickgui, omnitool_custom_crosshair, omnitool_freecam,
    omnitool_freelook, omnitool_low_fire, omnitool_minimap_cfg, omnitool_no_fog,
    omnitool_no_hurt_cam, omnitool_no_pumpkin_overlay, omnitool_omni_overlay,
    omnitool_session_stats, omnitool_tooltips, omnitool_tps, omnitool_waypoints,
    omnitool_world_lines, omnitool_world_map, omnitool_world_map_screen, omnitool_zoom,
};
use horizon_client::clipboard::Clipboard;
use horizon_client::mobs::{Mob, Species, spawn_rules};
use horizon_client::{extras::Corner, extras_hud::Corners, hud::HudBatch, items::ItemStack};
use horizon_core::position::BlockPos;
use horizon_protocol::packets::play::{self, Serverbound};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use winit::{event::MouseButton, keyboard::KeyCode};

/// The game as the modules see it.
pub struct GameHost<'a, 'g> {
    pub(super) game: &'a mut Game<'g>,
}

impl Host for GameHost<'_, '_> {
    fn chat(&mut self, line: &str) {
        // Our own lines are no received chat (Java adds them straight to
        // the chat component, past ChatListener): keep what came before,
        // then skip the line.
        let before = received_chat(self.game);
        CHAT_PENDING.with_borrow_mut(|p| p.extend(before));
        self.game.native_chat(line);
        let _ = received_chat(self.game);
    }
    fn player(&self) -> Option<Player> {
        self.game.native_player().map(|p| Player {
            x: p.x,
            y: p.y,
            z: p.z,
            yaw: p.yaw,
            pitch: p.pitch,
            dimension: p.dimension.to_owned(),
        })
    }
    fn view(&self) -> Option<core::View> {
        // `gui_size` (game/options.rs:91); `fov_degrees` (game/fov.rs:110)
        // takes the camera: the eye, 1.62 above the feet.
        let p = self.game.native_player()?;
        let (gui_w, gui_h) = self.game.gui_size();
        let fov = self.game.fov_degrees(1.0, [p.x, p.y + 1.62, p.z]);
        Some(core::View { gui_w, gui_h, fov })
    }
    fn data_dir(&self) -> Option<PathBuf> {
        Some(self.game.native_data_dir(core::ID))
    }
    fn text_width(&self, text: &str) -> f32 {
        self.game.native_text_width(text)
    }
    fn mapping(&mut self, name: &str) -> Option<(bool, String)> {
        self.game.native_mapping(name)
    }
    fn clipboard(&mut self) -> Option<String> {
        // `SystemClipboard::get` (horizon-client-input/src/clipboard.rs:147):
        // empty when there is no text. Field `clipboard` (game.rs:2026).
        Some(self.game.clipboard.get()).filter(|t| !t.is_empty())
    }
    fn set_clipboard(&mut self, text: &str) -> bool {
        // `SystemClipboard::set` (clipboard.rs:160) keeps the text in the
        // game when the system clipboard can't be reached; empty text is
        // not copied.
        self.game.clipboard.set(text);
        !text.is_empty()
    }
    fn language_code(&self) -> Option<String> {
        language_code()
    }
    fn server(&self) -> Server {
        server(self.game)
    }
    fn worn(&self) -> Worn {
        worn(self.game)
    }
    fn effects(&self) -> Vec<Effect> {
        effects(self.game)
    }
    fn inventory(&self) -> Inventory {
        inventory(self.game)
    }
    fn screen_open(&self) -> bool {
        !self.game.native_playing()
    }
    fn window_state(&self) -> core::WindowState {
        // Focus is `Game::focused` (game.rs:1616, from WindowEvent::Focused),
        // minimized is `Window::is_minimized` as `framerate_limit`
        // (game/options.rs:371). Horizon tracks no mouse hover over an
        // unfocused window and no occlusion: `hovered` stays false.
        core::WindowState {
            focused: self.game.focused,
            hovered: false,
            invisible: self
                .game
                .window
                .as_ref()
                .is_some_and(|w| w.is_minimized().unwrap_or(false)),
        }
    }
    fn set_fullbright(&mut self, on: bool) {
        // `Renderer::fullbright` (game/options.rs:297) is set from the saved
        // `settings.extras.fullbright` by `apply_ui_settings`; the module
        // ORs its own wish on top and never writes the setting.
        let saved = self.game.settings.extras.fullbright;
        if let Some(r) = self.game.renderer.as_mut() {
            r.fullbright = saved || on;
        }
    }
    fn set_distance_fog_removed(&mut self, removed: bool) {
        // `settings.horizon.fog` false makes `fog::clear_distance_fog` run
        // (game/render_tick.rs:141). It is the player's saved option, so the
        // first change remembers it and the end of the override restores it.
        thread_local! {
            static SAVED: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
        }
        match (removed, SAVED.get()) {
            (true, None) => {
                SAVED.set(Some(self.game.settings.horizon.fog));
                self.game.settings.horizon.fog = false;
            }
            (false, Some(original)) => {
                self.game.settings.horizon.fog = original;
                SAVED.set(None);
            }
            _ => {}
        }
    }
    fn disconnect(&mut self) {
        // The pause menu's Disconnect / Save and Quit to Title button
        // (game/screens.rs:245 -> `Game::close_world`, game.rs:9931): a
        // server goes through `leave_server` (game/remote.rs:565), a world
        // through the closing save. Nothing on the title screen or without
        // a level (Java: `mc.level == null`).
        if self.game.title.is_some() || self.game.player.is_none() {
            return;
        }
        if let Err(e) = self.game.close_world() {
            core::log_error(&format!("disconnect failed: {e}"));
        }
    }
    fn send_chat(&mut self, text: &str) {
        // `Game::send_chat` (game/remote.rs:6213, sendChat/sendCommand):
        // nothing without a connection. A line with a leading `/` goes out
        // as a command (specified behaviour of the `chat` step).
        self.game.send_chat(text);
    }
    fn send_command(&mut self, command: &str) {
        self.game.send_chat(&format!("/{command}"));
    }
    fn action_bar(&mut self, text: &str) {
        // As the server's action bar text (game/remote.rs:3310): field
        // `titles` (game.rs:1774), `Titles::set_overlay`
        // (horizon-client-render/src/hud_overlays.rs:308); `Text::legacy`
        // (horizon-client-core/src/text.rs:379) reads the `§` codes.
        self.game
            .titles
            .set_overlay(horizon_client::text::Text::legacy(text), false);
    }
    fn play_warning_sound(&mut self) {
        // Java: ANVIL_LAND at the player, volume 0.6, pitch 1.6. Field
        // `audio` (game.rs:1685), `Audio::play`
        // (horizon-client-render/src/audio.rs:797), as game/books.rs:1009.
        let at = self.game.native_player().map(|p| [p.x, p.y, p.z]);
        self.game
            .audio
            .play("minecraft:block.anvil.land", at, 0.6, 1.6);
    }
    fn world(&self) -> Option<World> {
        world(self.game)
    }
    fn map_loaded(&self) -> Option<super::omnitool_world_map::Loaded> {
        map_loaded(self.game)
    }
    fn map_scan(&mut self, cx: i32, cz: i32) -> Option<super::omnitool_world_map::Tile> {
        map_scan(self.game, cx, cz)
    }
    fn overlay_info(&self) -> OverlayInfo {
        overlay_info(self.game)
    }
    fn session_sample(&self) -> Option<core::SessionSample> {
        session_sample(self.game)
    }
    /// `time_since_rest` of the statistics (`stats`, game.rs:2112;
    /// `Stats::get`, game/stats.rs:68): counted by the game in single
    /// player (game/stats.rs:216), set from `AwardStats` on servers
    /// (game/remote.rs:3280). None without a player.
    fn time_since_rest(&self) -> Option<i64> {
        self.game.player.as_ref()?;
        Some(i64::from(
            self.game.stats.get("custom", "minecraft:time_since_rest"),
        ))
    }
    /// `ServerboundClientCommandPacket(REQUEST_STATS)`: action 1
    /// (horizon-protocol packets/play.rs:2132), sent through `Game::send`
    /// (game/remote.rs:665; nothing without a connection, where the
    /// statistics are the game's own).
    fn request_stats(&mut self) {
        self.game
            .send(Serverbound::ClientCommand(play::ClientCommand {
                action: 1,
            }));
    }
    fn input(&mut self) -> Option<&mut dyn Input> {
        if self.game.player.is_some() {
            Some(self)
        } else {
            None
        }
    }
    fn main_hand_id(&self) -> String {
        // `inventory[selected]` (game.rs:1566-1567), `ItemStack::item` is
        // the namespaced id.
        self.game
            .inventory
            .get(self.game.selected)
            .and_then(Option::as_ref)
            .map_or_else(String::new, |s| s.item.to_string())
    }
    fn aiming_at_block(&self) -> bool {
        // The block under the crosshair within reach: `aim` (game.rs:2246)
        // through `clip_ray` (game.rs:2326) over `reach` (game.rs:2249).
        self.game
            .aim()
            .and_then(|(origin, dir)| self.game.clip_ray(origin, dir, self.game.reach()))
            .is_some()
    }
    fn mining_hotbar(&self) -> Option<Vec<core::HotbarTool>> {
        // The targeted block as `aiming_at_block`; its name from the block
        // facts (game.rs:2262, as `Game::destroy_progress`, hand.rs:158) and
        // the base speed `Items::mining_speed` (horizon-items items.rs:890),
        // without effects or enchantments as Java's `getDestroySpeed`.
        let (origin, dir) = self.game.aim()?;
        let [x, y, z] = self.game.clip_ray(origin, dir, self.game.reach())?.pos;
        let name = self
            .game
            .res
            .facts()
            .get(self.game.world.block(x, y, z))
            .name
            .clone();
        let items = self.game.items();
        Some(
            (0..9)
                .map(|i| {
                    let s = self.game.inventory.get(i).and_then(Option::as_ref);
                    core::HotbarTool {
                        speed: f64::from(items.mining_speed(s, &name)),
                        durability_left: s
                            .filter(|s| horizon_client::stations::is_damageable(items, s))
                            .and_then(|s| {
                                items
                                    .get(&s.item)
                                    .map(|i| i.max_damage as i32 - s.damage as i32)
                            }),
                    }
                })
                .collect(),
        )
    }
    fn refill_view(&self) -> Option<core::RefillView> {
        // Menu slots 9-35 = `inventory[9..36]`, 36-44 = `inventory[0..9]`,
        // 45 = `offhand` (the plain inventory menu's mapping, game.rs:7260).
        // Ready = playing without a screen, nothing on the cursor
        // (`carried`, game.rs), not creative.
        self.game.player.as_ref()?;
        let items = self.game.items();
        let conv = |s: Option<&ItemStack>| {
            s.map(|s| {
                let info = items.get(&s.item);
                super::omnitool_auto_refill::Stack {
                    id: s.item.to_string(),
                    count: s.count as i32,
                    max_stack: info.map_or(64, |i| i.max_stack as i32),
                    damageable: horizon_client::stations::is_damageable(items, s),
                    block: info.is_some_and(|i| i.block.is_some()),
                    food: info.is_some_and(|i| i.food.is_some()),
                    comp: format!("{}|{:?}", s.damage, s.extra),
                }
            })
            .unwrap_or_default()
        };
        let inv = &self.game.inventory;
        let slots = (9..36)
            .chain(0..9)
            .map(|i| conv(inv.get(i).and_then(Option::as_ref)))
            .chain(std::iter::once(conv(self.game.offhand.as_ref())))
            .collect();
        Some(core::RefillView {
            slots,
            ready: self.game.native_playing()
                && self.game.carried.is_none()
                && !self.game.creative
                && !self.game.spectator(),
        })
    }
    fn click_menu_slot(&mut self, slot: usize) -> bool {
        // `tweak_click` (game/tweaks.rs:71) takes game slots: main 9-35,
        // hotbar 0-8, offhand 504 (menu mapping, game.rs:7260).
        let game_slot = match slot {
            9..=35 => slot,
            36..=44 => slot - 36,
            45 => 504,
            _ => return false,
        };
        self.game
            .tweak_click(game_slot, 0, horizon_client::menu::Input::Pickup)
    }
    fn carried_empty(&self) -> bool {
        self.game.carried.is_none()
    }
    fn attack_charged(&self) -> bool {
        // `attack_strength_scale` (game.rs:8949) is vanilla's
        // `getAttackStrengthScale(0.5)`; Java asks with 0.
        self.game.attack_strength_scale() >= 1.0
    }
    fn hotbar_slot(&self) -> i32 {
        // `selected` (game.rs:1567), 0-8.
        self.game.selected as i32
    }
    fn shift_down(&self) -> bool {
        // `modifiers` (game.rs:1621), winit's state of the last key event.
        self.game.modifiers.shift_key()
    }
    // `now_ms` is the trait's wall clock, which is all the HUD needs.
    fn craft_data(&mut self) -> Option<super::omnitool_craft_calc::CraftData> {
        use super::omnitool_craft_calc::{BookRecipe, CraftData};
        use super::omnitool_craft_option::CraftOption;
        // `native_recipes` (None outside a world) holds every recipe; the
        // book's unlocked ones are those whose `book_id` `native_known_recipes`
        // lists. Only the crafting kinds count (Java: shaped and shapeless).
        let all = self.game.native_recipes()?;
        let known: std::collections::HashSet<i32> =
            self.game.native_known_recipes().iter().copied().collect();
        let stack = |s: &super::native_hooks::NativeRecipeStack| {
            all.items
                .get(s.item as usize)
                .map(|i| (i.id.clone(), i.name.clone()))
        };
        let mut data = CraftData::default();
        for r in &all.recipes {
            let shaped = r.kind == NativeRecipeKind::CraftingShaped;
            if !(shaped || r.kind == NativeRecipeKind::CraftingShapeless)
                || !known.contains(&r.book_id)
            {
                continue;
            }
            let Some(result) = r
                .results
                .first()
                .and_then(|s| stack(s).map(|n| (n, s.count)))
            else {
                continue;
            };
            let slots: Vec<Vec<(String, String)>> = r
                .slots
                .iter()
                .map(|alts| alts.iter().filter_map(&stack).collect::<Vec<_>>())
                .filter(|alts| !alts.is_empty())
                .collect();
            data.options.push((
                result.0.0.clone(),
                CraftOption {
                    handle: r.book_id.to_string(),
                    slots: slots
                        .iter()
                        .map(|a| a.iter().map(|(id, _)| id.clone()).collect())
                        .collect(),
                    needs_table: if shaped {
                        r.width > 2 || r.height > 2
                    } else {
                        slots.len() > 4
                    },
                },
            ));
            data.book.push(BookRecipe {
                result: result.0,
                count: result.1 as i64,
                slots,
            });
        }
        // Menu slots 9..45 as `refill_view` maps them.
        let names = &self.game.pack.names;
        let inv = &self.game.inventory;
        data.inventory = (9..36)
            .chain(0..9)
            .map(|i| inv.get(i).and_then(Option::as_ref))
            .chain(std::iter::once(self.game.offhand.as_ref()))
            .flatten()
            .map(|s| {
                (
                    s.item.to_string(),
                    names.display_item(&s.item),
                    s.count as i64,
                )
            })
            .collect();
        Some(data)
    }
    fn craft_panel(&mut self, title: &str, recipes: &[super::omnitool_craft_calc::PanelRecipe]) {
        // `native_panel_recipes` (API 2): at most 256 recipes.
        let stack = |id: &str, count: i64| NativeStack {
            item: id.to_owned(),
            count: count.clamp(1, 64) as u32,
            components: String::new(),
        };
        let views = recipes
            .iter()
            .take(256)
            .map(|r| NativeRecipeView {
                layout: NativeRecipeLayout::Grid,
                width: r.width as u32,
                height: r.height as u32,
                slots: r
                    .slots
                    .iter()
                    .map(|alts| alts.iter().map(|id| stack(id, 1)).collect())
                    .collect(),
                results: vec![stack(&r.result.0, r.result.1)],
                note: r.note.clone(),
                action: r.action,
            })
            .collect();
        self.game
            .native_panel_recipes(core::ID, title, Vec::new(), 0, views);
    }
    fn craft_place(&mut self, handle: &str) -> Result<(), String> {
        let id: i32 = handle.parse().map_err(|_| format!("Bad recipe {handle}"))?;
        self.game.native_place_recipe(id, false)
    }
}

/// The canonical key a movement mapping is held as: `keys` (game.rs:1602)
/// holds keys after rebinding, as their mapping's default key
/// (`mapped_key`, game.rs:5980); movement reads them each tick
/// (game.rs:5546) and `native_mapping` shows them (native_hooks.rs:253).
fn move_key_code(key: &str) -> Option<KeyCode> {
    Some(match key {
        "forward" => KeyCode::KeyW,
        "back" => KeyCode::KeyS,
        "left" => KeyCode::KeyA,
        "right" => KeyCode::KeyD,
        "jump" => KeyCode::Space,
        "sneak" => KeyCode::ShiftLeft,
        "sprint" => KeyCode::ControlLeft,
        _ => return None,
    })
}

/// Workflow input on the window's game (only while there is a player).
impl Input for GameHost<'_, '_> {
    /// Java's `KeyMapping.setDown`. Movement keys go into `keys` as
    /// Horizon's own control agent does (game/agent.rs:1165, 920); a real
    /// key event of the player changes them again, as in Java. Attack and
    /// use are the held buttons `left`/`right` (game.rs:5175 mining,
    /// game.rs:5948 repeated use; servers read them in
    /// game/remote.rs:6006) without a click, like `setDown`.
    fn set_key(&mut self, key: &str, down: bool) {
        match key {
            "attack" => self.game.left = down,
            "use" => self.game.right = down,
            _ => {
                if let Some(k) = move_key_code(key) {
                    if down {
                        self.game.keys.insert(k);
                    } else {
                        self.game.keys.remove(&k);
                    }
                }
            }
        }
    }
    /// Java's `KeyMapping.click`: a press and release of the button through
    /// `play_mouse` (game.rs:12973; servers: `remote_mouse`,
    /// game/remote.rs:5946), as the control agent clicks
    /// (game/agent.rs:1306). With Toggle Attack/Use on, the press also
    /// flips the toggle (Horizon's `toggles.button`).
    fn click(&mut self, attack: bool) {
        let button = if attack {
            MouseButton::Left
        } else {
            MouseButton::Right
        };
        let _ = self.game.play_mouse(button, true);
        let _ = self.game.play_mouse(button, false);
    }
    /// `yaw`/`pitch` of the game (game.rs:1564-1565), which the player body
    /// takes each tick (game.rs:5400).
    fn rotation(&self) -> (f32, f32) {
        (self.game.yaw, self.game.pitch)
    }
    /// `Entity.setYRot`/`setXRot` (pitch clamped to ±90 as vanilla does),
    /// then the model's head snaps to it as after a server's rotation
    /// (`snap_player_model_pitch`, game/perspective.rs:497).
    fn set_rotation(&mut self, yaw: f32, pitch: f32) {
        self.game.yaw = yaw;
        self.game.pitch = pitch.clamp(-90.0, 90.0);
        self.game.snap_player_model_pitch();
    }
    /// `eye_position` (game.rs:2235): feet plus the pose's eye height.
    fn eye(&self) -> [f64; 3] {
        self.game.eye_position().unwrap_or_default()
    }
    /// `player.position` (motion.rs:272).
    fn position(&self) -> [f64; 3] {
        self.game
            .player
            .as_ref()
            .map_or([0.0; 3], |p| [p.position.x, p.position.y, p.position.z])
    }
    /// `selected` as the control agent's `select` sets it
    /// (game/agent.rs:1449); servers get the slot from the next tick's
    /// carried-item check (game/remote.rs:5993).
    fn select_slot(&mut self, slot: usize) {
        if slot < 9 {
            self.game.selected = slot;
        }
    }
    /// Java: a container screen → `closeContainer`, any other screen →
    /// `setScreen(null)`. As Escape closes them (game.rs:12815-12833):
    /// inventory and containers (`Screen::Inventory`, game.rs:567) through
    /// `close_container` (game.rs:7070), creative drops what the cursor
    /// holds; then back to playing with the mouse captured (game.rs:6104).
    fn close_screen(&mut self) {
        match self.game.screen {
            Screen::Playing => return,
            Screen::Inventory => self.game.close_container(),
            Screen::Creative => self.game.carried = None,
            _ => {}
        }
        self.game.screen = Screen::Playing;
        self.game.capture(true);
    }
    /// `key.inventory` as the player presses it (`key_pressed`, game.rs:12875):
    /// a ridden mount's inventory, the creative screen or the inventory;
    /// on servers `remote_key` first. Only while playing (the key would
    /// close an open inventory).
    fn open_inventory(&mut self) {
        if self.game.screen == Screen::Playing {
            let _ = self.game.key_pressed(KeyCode::KeyE);
        }
    }
    fn trader(&mut self) -> Option<&mut dyn Trader> {
        Some(self)
    }
}

/// The translation key of a trader's type name: `entity.minecraft.villager.<profession>`
/// (`Villager.getTypeName`; profession `variant2` into `mobs::render::PROFESSIONS`,
/// mobs/render.rs:106, as `entity_info`) or `entity.minecraft.wandering_trader`.
fn trader_key(m: &Mob) -> String {
    use horizon_client::mobs::render::PROFESSIONS;
    if m.kind.id.as_str() == "minecraft:villager" {
        let p = PROFESSIONS[m.extra.variant2 as usize % PROFESSIONS.len()];
        format!("entity.minecraft.villager.{p}")
    } else {
        "entity.minecraft.wandering_trader".to_owned()
    }
}

fn is_trader_mob(m: &Mob) -> bool {
    m.alive()
        && matches!(
            m.kind.id.as_str(),
            "minecraft:villager" | "minecraft:wandering_trader"
        )
}

/// An item stack for offer matching: id, `stations::hover_name`
/// (stations.rs:180) and the components as text (`ItemStack::extra`, its
/// Debug form names `enchantments`/`stored_enchantments` with their ids,
/// horizon-items item_data.rs:14-16).
fn trade_stack(game: &Game<'_>, s: &ItemStack) -> trading::Stack {
    trading::Stack {
        id: s.item.to_string(),
        name: horizon_client::stations::hover_name(&|id| game.pack.names.display_item(id), s),
        components: format!("{:?}", s.extra),
    }
}

fn trade_slot(game: &Game<'_>, s: Option<&ItemStack>) -> Slot {
    s.map(|s| Slot {
        stack: trade_stack(game, s),
        count: s.count as i32,
        max_stack: game.items().get(&s.item).map_or(64, |i| i.max_stack as i32),
    })
    .unwrap_or_default()
}

/// Merchant menu slot -> game slot: payments `stations::INPUT`/`+1`, result
/// `stations::RESULT`, then main 9-35 and hotbar 0-8 (the merchant mapping
/// of `menu_session`, game.rs:7329-7338).
fn merchant_game_slot(slot: usize) -> Option<usize> {
    use super::stations::{INPUT, RESULT};
    match slot {
        0 => Some(INPUT),
        1 => Some(INPUT + 1),
        2 => Some(RESULT),
        _ => (9..36).chain(0..9).nth(slot - 3),
    }
}

/// Trading (`trade`/`villager` steps, Villager Trades key) on the game.
impl Trader for GameHost<'_, '_> {
    fn hint(&mut self, text: &str) {
        Host::action_bar(self, text);
    }
    /// Living villagers and wandering traders (`mobs`, also a server's
    /// entities, game/remote.rs:1588) whose box meets the player's box
    /// grown by `grow`. Visible: `ray_box` (game.rs:2195) from the eyes to
    /// the box centre within `interaction_range`, and `clip_ray`
    /// (game.rs:2326) finds no block before it.
    fn traders(&self, grow: f64) -> Vec<TraderEntity> {
        let g = &*self.game;
        let (Some(p), Some(eye)) = (g.player.as_ref(), g.eye_position()) else {
            return Vec::new();
        };
        let pb = p.bounds();
        let (lo, hi) = (pb.min_position(), pb.max_position());
        g.mobs
            .iter()
            .filter(|m| is_trader_mob(m))
            .filter(|m| {
                let b = m.bounds();
                let (a, z) = (b.min_position(), b.max_position());
                a.x <= hi.x + grow
                    && z.x >= lo.x - grow
                    && a.y <= hi.y + grow
                    && z.y >= lo.y - grow
                    && a.z <= hi.z + grow
                    && z.z >= lo.z - grow
            })
            .map(|m| trader_entity(g, m, eye))
            .collect()
    }
    fn trader(&self, id: u32) -> Option<TraderEntity> {
        let g = &*self.game;
        let eye = g.eye_position()?;
        g.mobs
            .iter()
            .find(|m| m.id == id && is_trader_mob(m))
            .map(|m| trader_entity(g, m, eye))
    }
    /// As `mob_hit` (game.rs:8898): 3, creative 5.
    fn interaction_range(&self) -> f64 {
        if self.game.creative { 5.0 } else { 3.0 }
    }
    /// `mob_hit` (game.rs:8898, the crosshair ray against the mob boxes)
    /// and `before_block` (game/frames.rs:195, no block nearer), as a right
    /// click picks its target.
    fn crosshair_entity(&self) -> Option<u32> {
        let (i, t) = self.game.mob_hit()?;
        self.game.before_block(t).then(|| self.game.mobs[i].id)
    }
    /// `station` of kind `Merchant` (game/stations.rs:130,157) with its
    /// `MerchantState` (game/trading.rs:24): the session's offers
    /// (`MerchantOffer`, merchant.rs:133; `cost_a` with demand and special
    /// price), the result `station.result` (menu slot 2), `carried`, and the
    /// inventory in menu order. The title: a custom name (`name`) is plain
    /// text, which a server's title also is (game/remote.rs:6503); it counts
    /// as a profession when it equals one's shown name.
    fn merchant(&self) -> Option<MerchantView> {
        use horizon_client::mobs::render::PROFESSIONS;
        let g = &*self.game;
        let station = g.station.as_ref()?;
        let m = station.merchant.as_ref()?;
        let items = g.items();
        let offers = m
            .session
            .offers
            .iter()
            .map(|o| MenuOffer {
                offer: trading::Offer {
                    cost_a: trade_stack(g, &o.cost_a(items)),
                    cost_b: o
                        .cost_b()
                        .map_or_else(Default::default, |s| trade_stack(g, &s)),
                    result: trade_stack(g, &o.result),
                },
                result_count: o.result.count as i32,
                out_of_stock: o.out_of_stock(),
            })
            .collect();
        let key = |k: &str| NameText {
            translatable: Some(k.to_owned()),
            siblings: Vec::new(),
        };
        let title = match &m.name {
            None if m.session.villager => key(&format!(
                "entity.minecraft.villager.{}",
                m.session.profession.trim_start_matches("minecraft:")
            )),
            None => key("entity.minecraft.wandering_trader"),
            Some(name) => PROFESSIONS
                .iter()
                .map(|p| format!("entity.minecraft.villager.{p}"))
                .chain(std::iter::once(
                    "entity.minecraft.wandering_trader".to_owned(),
                ))
                .find(|k| g.pack.names.display_get(k).as_deref() == Some(name.as_str()))
                .map_or_else(NameText::default, |k| key(&k)),
        };
        Some(MerchantView {
            offers,
            title,
            result: trade_slot(g, station.result.as_ref()),
            carried: trade_slot(g, g.carried.as_ref()),
            inventory: (9..36)
                .chain(0..9)
                .map(|i| trade_slot(g, g.inventory.get(i).and_then(Option::as_ref)))
                .collect(),
        })
    }
    /// `Inventory.countItem`: main, armor and offhand (game.rs:1566,1757,1759).
    fn count(&self, id: &str) -> i32 {
        let g = &*self.game;
        g.inventory
            .iter()
            .chain(g.armor.iter())
            .chain(std::iter::once(&g.offhand))
            .flatten()
            .filter(|s| &*s.item == id)
            .map(|s| s.count as i32)
            .sum()
    }
    /// What the screen's offer button does (`merchant_click` →
    /// `try_move_items`, game/trading.rs:521, private there): on a server the
    /// selection hint and `SelectTrade` (the server moves the payment), else
    /// `Menu::merchant_select` (menu.rs:2938) through `menu_session`
    /// (game.rs:7227).
    fn select_trade(&mut self, index: usize) {
        let g = &mut *self.game;
        if g.remote.is_some() {
            if let Some(m) = g.station.as_mut().and_then(|s| s.merchant.as_mut()) {
                m.session.selection_hint = index;
            }
            g.send(Serverbound::SelectTrade(play::SelectTrade {
                item: index as i32,
            }));
            return;
        }
        g.menu_session(|menu, env, _| {
            menu.merchant_select(env, index);
            Some(true)
        });
    }
    /// `tweak_click` (game/tweaks.rs:71: `remote_menu_ops` on a server, else
    /// `menu_click`) on the game slot; outside (-999) as Horizon's own
    /// outside click (game.rs:8036; `remote_menu_ops` sends None as -999,
    /// game/remote.rs:6317).
    fn click(&mut self, slot: Option<usize>, quick: bool) {
        use horizon_client::menu::Input as I;
        let input = if quick { I::QuickMove } else { I::Pickup };
        let g = &mut *self.game;
        match slot {
            Some(s) => {
                if let Some(gs) = merchant_game_slot(s) {
                    g.tweak_click(gs, 0, input);
                }
            }
            None if g.remote.is_some() => {
                g.remote_menu_ops(&[(None, 0, input)]);
            }
            None => {
                g.menu_click(None, 0, input);
            }
        }
    }
    fn preset(&self, profession: &str) -> Option<(String, bool)> {
        super::omnitool_villager_trades::preset_for(self, profession)
    }
}

fn trader_entity(g: &Game<'_>, m: &Mob, eye: [f64; 3]) -> TraderEntity {
    let b = m.bounds();
    let c = b.center();
    let center = [c.x, c.y, c.z];
    let key = trader_key(m);
    let (name, shown) = match &m.custom_name {
        Some(n) => (NameText::default(), n.clone()),
        None => (
            NameText {
                translatable: Some(key.clone()),
                siblings: Vec::new(),
            },
            g.pack.names.display_get(&key).unwrap_or(key),
        ),
    };
    let d = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let range = if g.creative { 5.0 } else { 3.0 };
    let visible = len > 1e-9 && {
        let dir = d.map(|v| v / len);
        super::ray_box(eye, dir, range, b).is_some_and(|t| g.clip_ray(eye, dir, t).is_none())
    };
    TraderEntity {
        id: m.id,
        type_id: m.kind.id.to_string(),
        name,
        shown,
        pos: [m.position.x, m.position.y, m.position.z],
        center,
        visible,
    }
}

/// The game for modules that only read (`&self` hooks); it cannot chat.
struct ReadHost<'a, 'g> {
    game: &'a Game<'g>,
}

impl Host for ReadHost<'_, '_> {
    fn chat(&mut self, _line: &str) {}
    fn player(&self) -> Option<Player> {
        self.game.native_player().map(|p| Player {
            x: p.x,
            y: p.y,
            z: p.z,
            yaw: p.yaw,
            pitch: p.pitch,
            dimension: p.dimension.to_owned(),
        })
    }
    fn data_dir(&self) -> Option<PathBuf> {
        Some(self.game.native_data_dir(core::ID))
    }
    fn language_code(&self) -> Option<String> {
        language_code()
    }
    fn server(&self) -> Server {
        server(self.game)
    }
    fn shift_down(&self) -> bool {
        self.game.modifiers.shift_key()
    }
    fn alt_down(&self) -> bool {
        self.game.modifiers.alt_key()
    }
    fn ctrl_down(&self) -> bool {
        self.game.modifiers.control_key()
    }
}

// ------------------------------------------------------------ game data
// Read straight from `Game`'s private fields: this file is a module of
// game.rs, so it sees them. Lines refer to Horizon fbd7288.

/// The game's language code (`de_de`): `horizon_client::lang::code`
/// (horizon-client-core/src/lang.rs:235), the language loaded from
/// `settings.language` (horizon-client-render/src/settings.rs:145).
fn language_code() -> Option<String> {
    Some(horizon_client::lang::code())
}

/// Where the player is. `has_singleplayer_server` (game/integrated.rs:207):
/// no connection, or the integrated server's. Otherwise the server list's
/// address of the server joined (`server_packs.address`,
/// game/server_packs.rs:85,252, set in `join_server` game/remote.rs:387),
/// else `--server` (`options.server`, game.rs:358). A server joined by
/// direct connect and not in the list stays Unknown (counts as foreign).
fn server(game: &Game<'_>) -> Server {
    if game.has_singleplayer_server() {
        return Server::Local;
    }
    match game
        .server_packs
        .address
        .clone()
        .or_else(|| game.options.server.clone())
    {
        Some(address) => Server::Remote(address),
        None => Server::Unknown,
    }
}

/// `ItemStack.getHoverName` (`stations::hover_name`, stations.rs:180) and
/// the durability: `max_damage` of the item (horizon-items/src/items.rs:68)
/// when `stations::is_damageable` (stations.rs:156, not unbreakable),
/// `ItemStack::damage` (items.rs:230).
fn stack(game: &Game<'_>, s: &ItemStack) -> Stack {
    let items = game.items();
    let max_damage = if horizon_client::stations::is_damageable(items, s) {
        items.get(&s.item).map_or(0, |i| i.max_damage as i32)
    } else {
        0
    };
    Stack {
        name: horizon_client::stations::hover_name(&|id| game.pack.names.display_item(id), s),
        max_damage,
        damage: s.damage as i32,
    }
}

/// Armor `armor: [head, chest, legs, feet]` (game.rs:1757), the main hand
/// `inventory[selected]` (game.rs:1566-1567, as game.rs:10024) and
/// `offhand` (game.rs:1759).
fn worn(game: &Game<'_>) -> Worn {
    let get = |s: Option<&ItemStack>| s.map(|s| stack(game, s));
    let [head, chest, legs, feet] = &game.armor;
    Worn {
        head: get(head.as_ref()),
        chest: get(chest.as_ref()),
        legs: get(legs.as_ref()),
        feet: get(feet.as_ref()),
        main: get(game.inventory.get(game.selected).and_then(Option::as_ref)),
        off: get(game.offhand.as_ref()),
    }
}

/// `potions.effects.list` (game.rs:1591, game/potions.rs:318; servers fill
/// it too, game/remote.rs:710): `EffectInstance` id/duration (-1 =
/// infinite)/amplifier/ambient (horizon-client-render/src/effects.rs:8).
/// Name as the inventory's effect panels translate it
/// (`effect.minecraft.<id>`, inventory_effects.rs:50); colour and category
/// from `brewing::effect` (horizon-client-core/src/potions.rs:87).
fn effects(game: &Game<'_>) -> Vec<Effect> {
    game.potions
        .effects
        .list
        .iter()
        .map(|e| {
            let def = horizon_client::brewing::effect(e.id);
            Effect {
                name: game
                    .pack
                    .names
                    .display_get(&format!("effect.minecraft.{}", e.id))
                    .unwrap_or_else(|| e.id.to_owned()),
                amplifier: e.amplifier,
                duration: e.duration,
                color: def.map_or(0xFFFFFF, |d| d.color),
                harmful: def
                    .is_some_and(|d| d.category == horizon_client::brewing::Category::Harmful),
                ambient: e.ambient,
            }
        })
        .collect()
}

/// All slots: `inventory` (36, game.rs:1566), `armor`, `offhand`, counted
/// per item type (the key is the item's name without components:
/// `Names::display_item`, creative.rs:93). A screen is open when not
/// `native_playing` (containers are `Screen::Inventory`, game.rs:567).
/// Creative or spectator: `creative` and `spectator()` (game.rs:1599-1601,
/// game/game_mode.rs:23; servers set them, game/remote.rs:2911).
fn inventory(game: &Game<'_>) -> Inventory {
    let slots = game
        .inventory
        .iter()
        .chain(&game.armor)
        .chain(std::iter::once(&game.offhand))
        .flatten()
        .map(|s| (game.pack.names.display_item(&s.item), s.count))
        .collect();
    Inventory {
        slots,
        screen_open: !game.native_playing(),
        creative: game.creative || game.spectator(),
    }
}

/// `Conditions`' and the triggers' view of the game, None without a player.
/// Health, food and death from the player's `health` (motion.rs:361,
/// health.rs:227,241; servers set it, game/remote.rs:2800), as Horizon's
/// HUD reads it (game.rs:6550). Items as `stack`'s names with the
/// namespaced id (`ItemStack::item`). The day time `world.day_time`
/// (horizon-client-render/src/world.rs:88, as `Game::day_time`,
/// game.rs:962). Sneaking = the sneak key held, as Horizon's own checks
/// (game.rs:4848). World name: the save's level name (`level.name`,
/// save.rs:2861, as the control socket, game/events.rs:166), only in
/// single player.
fn world(game: &Game<'_>) -> Option<World> {
    let p = game.player.as_ref()?;
    let item = |s: &ItemStack| Item {
        id: s.item.to_string(),
        name: horizon_client::stations::hover_name(&|id| game.pack.names.display_item(id), s),
        count: s.count,
    };
    let main = game.inventory.iter().take(36);
    let server = server(game);
    Some(World {
        health: f64::from(p.health.health()),
        hunger: p.health.food.food,
        y: p.position.y,
        holding: game
            .inventory
            .get(game.selected)
            .and_then(Option::as_ref)
            .map(item),
        inventory: main.clone().flatten().map(item).collect(),
        inventory_full: main.clone().count() == 36 && main.clone().all(Option::is_some),
        dimension: game.dim.id().to_owned(),
        world_name: if server == Server::Local {
            game.level.name.clone()
        } else {
            String::new()
        },
        server,
        sneaking: game.keys.contains(&KeyCode::ShiftLeft),
        dead: p.health.dead_or_dying(),
        day_time: game.world.day_time,
        screen_open: !game.native_playing(),
    })
}

// ------------------------------------------------------------ world map data

thread_local! {
    /// (colour, flags) per block state for the world map's scan; built on
    /// first use, as Horizon's minimap `Palette` (minimap.rs:36).
    static MAP_PALETTE: RefCell<Option<Vec<(u8, u8)>>> = const { RefCell::new(None) };
}

/// The loaded chunks (`world.chunks`, keys are chunk coordinates), the
/// world's `revision` and the chunk the player stands in (`native_player`).
fn map_loaded(game: &Game<'_>) -> Option<super::omnitool_world_map::Loaded> {
    let p = game.native_player()?;
    Some(super::omnitool_world_map::Loaded {
        revision: game.world.revision,
        center: ((p.x.floor() as i32) >> 4, (p.z.floor() as i32) >> 4),
        chunks: game.world.chunks.keys().copied().collect(),
    })
}

/// One chunk's columns, scanned like Horizon's minimap `scan_tile`
/// (horizon-client-render/src/minimap.rs:155): block facts from
/// `res.facts()` (`map_color`, `fluid`, `sturdy_full` bit 1 = UP), the top
/// from the first non-uniform section from above.
fn map_scan(game: &Game<'_>, cx: i32, cz: i32) -> Option<super::omnitool_world_map::Tile> {
    use super::omnitool_world_map as wm;
    use horizon_client::blocks::Fluid;
    use horizon_client::chunk::{MIN_Y, SECTIONS};
    let chunk = game.world.chunks.get(&(cx, cz))?;
    let facts = game.res.facts();
    let air = game.world.air.0;
    let top = (0..SECTIONS)
        .rev()
        .find(|&s| {
            chunk
                .sections
                .get(s)
                .is_some_and(|sec| !sec.is_uniform(air))
        })
        .map_or(MIN_Y, |s| MIN_Y + s as i32 * 16 + 15);
    MAP_PALETTE.with_borrow_mut(|palette| {
        let palette = palette.get_or_insert_with(|| {
            facts
                .states
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let mut flags = match f.fluid {
                        Fluid::None => 0,
                        Fluid::Water => 1,
                        Fluid::Lava => 2,
                    };
                    if facts.is_air(horizon_world::registry::BlockStateId(i as u32)) {
                        flags |= 4;
                    }
                    if f.sturdy_full >> 1 & 1 != 0 {
                        flags |= wm::STURDY_TOP;
                    }
                    (f.map_color, flags)
                })
                .collect()
        });
        let mut tile = wm::Tile::empty();
        for z in 0..16 {
            for x in 0..16 {
                let block = |y: i32| {
                    palette
                        .get(chunk.get(x, y, z) as usize)
                        .copied()
                        .unwrap_or((0, 4))
                };
                let (c, h, d) = wm::scan_column(&block, top);
                let i = (z * 16 + x) as usize;
                tile.color[i] = c;
                tile.height[i] = h;
                tile.depth[i] = d;
            }
        }
        Some(tile)
    })
}

// ------------------------------------------------------------ overlay data
// What the Omni Overlay entries read (docs/spec/omnitool/omni-overlay.md
// §3.3). Most is read per frame from `&Game`; what needs `&mut Game` or is
// too heavy per frame (ping, the spawn rule, the redstone input) is sampled
// in `tick` while the module is active and kept in `OVERLAY`.

/// Samples `tick` took for the overlay.
#[derive(Default)]
struct Sampled {
    ticks: u64,
    ping: Option<i32>,
    spawn: Option<SpawnInfo>,
    /// The targeted block and `getBestNeighborSignal` there.
    signal: Option<([i32; 3], i32)>,
}

thread_local! {
    static OVERLAY: RefCell<Sampled> = RefCell::new(Sampled::default());
    /// Entry `memory`, read at most once a second: (when, MB).
    static MEMORY: Cell<Option<(Instant, Option<(u64, u64)>)>> = const { Cell::new(None) };
    /// The properties of the last targeted block state.
    static PROPS: RefCell<Option<(horizon_world::registry::BlockStateId, Vec<Prop>)>> =
        const { RefCell::new(None) };
}

/// The overlay's view of the game, per frame. Unchanged from 111: the frame
/// counter (`native_frame_rate`, native_hooks.rs:405), the level's day time
/// (`world.day_time`, as `world()`), gamemode (`creative`, `spectator()`, as
/// `inventory()`) and the F3 screen (`debug`, game.rs:1637). TPS, the lag
/// warning and session statistics come from their modules; waypoints stay
/// empty (no module yet).
fn overlay_info(game: &Game<'_>) -> OverlayInfo {
    let feet = player_block(game);
    let block = target_block(game);
    OVERLAY.with_borrow(|o| OverlayInfo {
        fps: Some(game.native_frame_rate().0),
        memory_mb: memory_mb(),
        ping: o.ping,
        biome: feet.and_then(|p| biome(game, p)),
        light: feet.map(|p| light(game, p)),
        day_time: Some(game.world.day_time),
        slime_seed: slime_seed(game),
        spawn: o.spawn.clone(),
        creative_or_spectator: game.creative || game.spectator(),
        block: block.map(|(pos, state)| BlockInfo {
            pos,
            id: game.res.facts().get(state).name.to_string(),
            props: block_props(game, state),
            neighbor_signal: o.signal.filter(|s| s.0 == pos).map_or(0, |s| s.1),
        }),
        entity: target_mob(game).map(entity_info),
        vehicle: vehicle(game).map(entity_info),
        // `sleeping` (game.rs:1815): the bed's head while asleep, as
        // `getSleepingPos`; set game.rs:11598, servers game/remote.rs:2412,
        // cleared on waking (game.rs:11611).
        sleeping_pos: game.sleeping,
        vanilla_debug: game.debug,
        tps: omnitool_tps::info(&ReadHost { game }),
        lag_warning: omnitool_tps::lag_warning(&ReadHost { game }),
        session: omnitool_session_stats::info(&ReadHost { game }),
        ..OverlayInfo::default()
    })
}

/// Identity of the connection for the TPS tracker (`@local` for the
/// integrated server); None while no player is in a world.
fn tps_source(game: &Game<'_>) -> Option<String> {
    game.player.as_ref()?;
    Some(match server(game) {
        Server::Remote(address) => address,
        _ => "@local".to_owned(),
    })
}

thread_local! {
    /// Times the player joined a world or server (Session Stats resets on it).
    static JOINS: Cell<(u64, bool)> = const { Cell::new((0, false)) };
}

/// Counts joins: a player appearing after none.
fn count_joins(game: &Game<'_>) {
    let here = game.player.is_some();
    let (n, was) = JOINS.get();
    JOINS.set((if here && !was { n + 1 } else { n }, here));
}

/// Session Stats' per-tick view: `player.flying` (game.rs:5408),
/// `fall_flying` (as game/music.rs:38), `riding` (game.rs:1922), the game
/// mode (`spectator()`, game/game_mode.rs:23), `health.dead_or_dying()`
/// (health.rs:241) and the statistics `mined` (game.rs:4384, summed over
/// blocks) and `custom:mob_kills` (game/creatures.rs:923; single player
/// only, servers only fill `stats` from `AwardStats`).
fn session_sample(game: &Game<'_>) -> Option<core::SessionSample> {
    let p = game.player.as_ref()?;
    let mined = game
        .stats
        .values
        .get("minecraft:mined")
        .map_or(0, |m| m.values().map(|&n| i64::from(n)).sum());
    Some(core::SessionSample {
        join: JOINS.get().0,
        flying: p.flying,
        fall_flying: p.fall_flying,
        passenger: game.riding.is_some(),
        spectator: game.spectator(),
        dead: p.health.dead_or_dying(),
        mined_total: mined,
        kills_total: i64::from(game.stats.get("custom", "minecraft:mob_kills")),
    })
}

/// `set_time` injection: `ClientboundSetTimePacket` reached
/// `Game::handle_packet`; hooked via `remote_clock_tick` (docs/ARCHITECTURE.md).
pub(super) fn set_time(game: &Game<'_>, game_time: i64) {
    if game.native_on(core::ID)
        && let Some(source) = tps_source(game)
    {
        omnitool_tps::on_time_packet(&source, game_time);
    }
}

/// `player.blockPosition()`: the block of the feet (`player.position`,
/// motion.rs:272).
fn player_block(game: &Game<'_>) -> Option<[i32; 3]> {
    let p = &game.player.as_ref()?.position;
    Some([p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32])
}

/// `level.getBiome(pos)`: the zoomed biome as natural spawning reads it
/// (`streaming.biome_zoom.quart_at_block`, game/natural_spawning.rs:171;
/// servers set the zoom from the hashed seed, game/remote.rs:1281), then the
/// chunk's biome (`Chunk::biome`, horizon-client-render/src/chunk.rs:483) by
/// name (`BiomeInfo::name`, biome.rs:90, as game/music.rs:128). Namespaced.
fn biome(game: &Game<'_>, [x, y, z]: [i32; 3]) -> Option<String> {
    let [qx, qy, qz] = game
        .streaming
        .biome_zoom
        .quart_at_block(BlockPos::new(x, y, z));
    let (x, y, z) = (qx << 2, qy << 2, qz << 2);
    let chunk = game.world.chunks.get(&(x >> 4, z >> 4))?;
    Some(game.res.biomes().get(chunk.biome(x, y, z)).name.clone())
}

/// `getBrightness(BLOCK)` and `getBrightness(SKY)`: the packed light of the
/// block (`World::light`, horizon-client-render/src/world.rs:392, block in
/// the low nibble, sky in the high one, as the block library reads it,
/// game.rs:1031-1036).
fn light(game: &Game<'_>, [x, y, z]: [i32; 3]) -> (i32, i32) {
    let l = game.world.light(x, y, z);
    (i32::from(l & 15), i32::from(l >> 4))
}

/// `SlimeChunks.seed`: the single player world's seed (`level.seed`,
/// save.rs:2862) when the game hosts the world (`has_singleplayer_server`,
/// game/integrated.rs:207). Servers: unknown (the Slime Chunks / Chunk
/// Borders seed settings come with those modules).
fn slime_seed(game: &Game<'_>) -> Option<i64> {
    game.has_singleplayer_server().then_some(game.level.seed)
}

/// `memory` (deviation, omni-overlay.md §9.6): /proc read once a second.
fn memory_mb() -> Option<(u64, u64)> {
    let now = Instant::now();
    if let Some((at, mb)) = MEMORY.get()
        && now.duration_since(at) < Duration::from_secs(1)
    {
        return mb;
    }
    let read = |f| std::fs::read_to_string(f).ok();
    let mb = read("/proc/self/status")
        .zip(read("/proc/meminfo"))
        .and_then(|(status, meminfo)| entries::memory_mb(&status, &meminfo));
    MEMORY.set(Some((now, mb)));
    mb
}

/// `cam.pick(20, partialTick, false)`, block hits only: the eye and view
/// (`aim`, game.rs:2246) through `clip_ray` (game.rs:2316, outline shapes,
/// no fluids) 20 blocks far instead of the reach.
fn target_block(game: &Game<'_>) -> Option<([i32; 3], horizon_world::registry::BlockStateId)> {
    let (origin, dir) = game.aim()?;
    let [x, y, z] = game.clip_ray(origin, dir, 20.0)?.pos;
    Some(([x, y, z], game.world.block(x, y, z)))
}

/// `state.getProperties()` with values: the registry's state
/// (`pack.assets.registry`, as game/machines.rs:124) has them sorted by name
/// like Java's `StateDefinition`. Int properties get their largest value
/// from `property_values` (horizon-world/src/registry.rs:156). Kept for the
/// last state (`property_values` walks all states).
fn block_props(game: &Game<'_>, state: horizon_world::registry::BlockStateId) -> Vec<Prop> {
    PROPS.with_borrow_mut(|cache| {
        if let Some((s, props)) = cache.as_ref()
            && *s == state
        {
            return props.clone();
        }
        let registry = game.pack.assets.registry;
        let props: Vec<Prop> = registry.state(state).map_or_else(Vec::new, |st| {
            st.properties
                .iter()
                .map(|(name, value)| Prop {
                    name: name.clone(),
                    value: value.clone(),
                    int: entries::int_prop(value, registry.property_values(&st.name, name)),
                })
                .collect()
        });
        *cache = Some((state, props.clone()));
        props
    })
}

/// `mc.crosshairPickEntity`, mobs only: the mob the crosshair hits
/// (`mob_hit`, game.rs:8898, the attack reach) before any block
/// (`before_block`, game/frames.rs:195), as `pick_entity_hit`
/// (game/pick_entity.rs:11). Boats, minecarts, item frames and LAN guests
/// are not mobs and stay unknown.
fn target_mob<'a>(game: &'a Game<'_>) -> Option<&'a Mob> {
    let (i, distance) = game.mob_hit()?;
    game.before_block(distance).then(|| &game.mobs[i])
}

/// `player.getVehicle()` when it is a mob: `riding` (game.rs:1922, set by
/// mounting, game/mounts.rs:74,229, game/llamas.rs:138).
fn vehicle<'a>(game: &'a Game<'_>) -> Option<&'a Mob> {
    let id = game.riding?;
    game.mobs.iter().find(|m| m.id == id)
}

/// A mob as the entries see it: type id (`MobType::id`, namespaced),
/// health and max health (`Mob::health`, `finalize::max_health` =
/// `MAX_HEALTH` with modifiers, mobs/finalize.rs:684), `MOVEMENT_SPEED`
/// with effects and sprinting (`Mob::movement_speed`, mobs.rs:2575),
/// `JUMP_STRENGTH` (`attrs.jump_strength`, mobs.rs:743) and, for villagers
/// and zombie villagers, profession (`variant2` into
/// `mobs::render::PROFESSIONS`) and level (`variant3`, mobs/villager.rs:14,
/// as the renderer reads them, mobs/render.rs:797,1406).
fn entity_info(m: &Mob) -> EntityInfo {
    use horizon_client::mobs::{finalize, render::PROFESSIONS};
    let villager = matches!(m.species(), Species::Villager | Species::ZombieVillager).then(|| {
        let profession = PROFESSIONS[m.extra.variant2 as usize % PROFESSIONS.len()];
        (
            format!("minecraft:{profession}"),
            i32::from(m.extra.variant3),
        )
    });
    EntityInfo {
        type_id: m.kind.id.to_string(),
        health: Some((m.health, finalize::max_health(m))),
        movement_speed: m.movement_speed(),
        jump_strength: m.attrs.jump_strength,
        villager,
    }
}

/// The `spawn_here` test (the Zombie, `Mc.spawnTestType`) at a block:
/// `isValidSpawn` of the block below and `isValidEmptySpawnBlock` here,
/// through Horizon's spawn rules (`spawn_rules::is_valid_spawn`,
/// `is_valid_empty_spawn`, mobs/spawn_rules.rs:203,215) on the game's mob
/// world (`mob_world_base`, game/creatures.rs:465). Light Overlay shares
/// this rule (world-render.md §2) once it is ported.
fn spawn_spot(game: &Game<'_>, [x, y, z]: [i32; 3]) -> bool {
    let w = game.mob_world_base(&[]);
    spawn_rules::is_valid_spawn(Species::Zombie, &w, x, y - 1, z)
        && spawn_rules::is_valid_empty_spawn(Species::Zombie, &w, x, y, z)
}

/// The latency of our own `PlayerInfo` (`connection.getPlayerInfo(uuid)`):
/// the tab list's entry (`remote_tab_players`, game/remote.rs:3407, latency
/// from `PlayerInfoUpdate`, game/remote.rs:3131) of our profile
/// (`Remote::profile`, game/remote.rs:7327). None when the game hosts the
/// world (Java: `isLocalServer`), without a connection or an entry.
fn ping(game: &mut Game<'_>) -> Option<i32> {
    if game.has_singleplayer_server() {
        return None;
    }
    let own = game.remote.as_ref()?.profile();
    let (players, _, _) = game.remote_tab_players()?;
    players.iter().find(|p| p.uuid == own).map(|p| p.latency)
}

/// Samples for the overlay, each tick while Omni Overlay is active: the
/// spawn rule and light at the feet, the targeted block's redstone input
/// (`getBestNeighborSignal` through the block library, `with_level`,
/// game.rs:3049, as command blocks read it, game/command_blocks.rs:292)
/// and, once a second, the ping (`remote_tab_players` also refreshes skins).
fn sample_overlay(game: &mut Game<'_>) {
    if !core::active(
        &GameHost { game: &mut *game },
        &omnitool_omni_overlay::MODULE,
    ) {
        OVERLAY.take();
        return;
    }
    let ticks = OVERLAY.with_borrow(|o| o.ticks);
    let ping = if ticks % 20 == 0 {
        Some(ping(game))
    } else {
        None
    };
    let spawn = player_block(game).map(|p| {
        let (block_light, sky_light) = light(game, p);
        SpawnInfo {
            spot: spawn_spot(game, p),
            block_light,
            sky_light,
        }
    });
    let signal = target_block(game).map(|(pos, _)| {
        let at = BlockPos::new(pos[0], pos[1], pos[2]);
        (
            pos,
            game.with_level(|b, level, t| b.best_neighbor_signal(level, t, at)),
        )
    });
    OVERLAY.with_borrow_mut(|o| {
        o.ticks += 1;
        if let Some(p) = ping {
            o.ping = p;
        }
        o.spawn = spawn;
        o.signal = signal;
    });
}

// ------------------------------------------------------------ chat
// Chat triggers read the lines the game received. Java's ChatListener mixin
// sees player and system messages, not the action bar. Horizon counts them:
// on a connection (also the single player server's) `remote.chat` keeps
// the last 100 plain lines and `chat_received` counts them
// (game/remote.rs:767-796, only lines the Chat option shows; the action
// bar goes to `titles`, game/remote.rs:3078); without one the chat
// component itself (`ui.chat.received`, `recent_lines`,
// horizon-client-render/src/chat.rs:205,604), which also holds client lines.

thread_local! {
    /// The count read last, and whether it was the connection's.
    static CHAT_SEEN: Cell<Option<(bool, u64)>> = const { Cell::new(None) };
    /// Lines read before one of our own chat lines, for the next tick.
    static CHAT_PENDING: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// The lines received since the last call, oldest first. The first call
/// (and a switch between connection and none) only starts counting.
fn received_chat(game: &Game<'_>) -> Vec<String> {
    let (remote, count) = match game.remote.as_ref() {
        Some(r) => (true, r.chat_received),
        None => (false, game.ui.chat.received),
    };
    let seen = CHAT_SEEN.replace(Some((remote, count)));
    let new = match seen {
        Some((was_remote, n)) if was_remote == remote && n <= count => (count - n) as usize,
        _ => return Vec::new(),
    };
    if new == 0 {
        return Vec::new();
    }
    match game.remote.as_ref() {
        Some(r) => {
            let skip = r.chat.len().saturating_sub(new);
            r.chat[skip..].iter().map(|(t, _)| t.clone()).collect()
        }
        None => {
            let mut lines = game.ui.chat.recent_lines(new);
            lines.reverse();
            lines
        }
    }
}

thread_local! {
    /// Set while `chat_line` runs the game's own `chat_line`.
    static IN_CHAT_LINE: Cell<bool> = const { Cell::new(false) };
}

/// Injection `hook_chat_line` (HEAD, cancellable) on `Game::chat_line`
/// (game/remote.rs:767), where every connection line enters the chat.
/// Chat Tweaks decides: hide the line, or add timestamp / `(xN)` text and
/// run the game's `chat_line` again with the new text. Client lines
/// (Omnitool's own) pass untouched.
pub(super) fn chat_line(
    game: &mut Game<'_>,
    content: &horizon_client::text::Text,
    source: horizon_client::chat::ChatSource,
) -> ::core::ops::ControlFlow<bool> {
    use super::omnitool_chat_tweaks::Verdict;
    use ::core::ops::ControlFlow;
    if IN_CHAT_LINE.get()
        || source == horizon_client::chat::ChatSource::Client
        || !game.native_on(core::ID)
    {
        return ControlFlow::Continue(());
    }
    let plain = content.plain();
    match super::omnitool_chat_tweaks::intercept(&mut GameHost { game: &mut *game }, &plain) {
        Verdict::Keep => ControlFlow::Continue(()),
        Verdict::Hide => ControlFlow::Break(false),
        Verdict::Decorate(pre, post) => {
            let text = horizon_client::text::Text::legacy(&pre)
                .append(content.clone())
                .append(horizon_client::text::Text::legacy(&post));
            IN_CHAT_LINE.set(true);
            let shown = game.chat_line(text, source);
            IN_CHAT_LINE.set(false);
            ControlFlow::Break(shown)
        }
    }
}

/// `native_tick`: the window's game only (config autosave, workflows).
/// Received chat lines go to the chat triggers first.
pub(super) fn tick(game: &mut Game<'_>) {
    // F1 (hidden HUD) draws no menu but would still take its input: close it
    if game.ui.hide_gui && omnitool_clickgui::is_open() {
        omnitool_clickgui::close(&mut GameHost { game: &mut *game });
        game.capture(true);
    }
    if game.native_window() && game.native_on(core::ID) {
        let mut lines = CHAT_PENDING.take();
        lines.extend(received_chat(game));
        for line in lines {
            core::chat_received(&line);
            // Chat Tweaks / Chat Log see the same lines; with a connection
            // the chat injection (`chat_line`) already did.
            if game.remote.is_none() {
                super::omnitool_chat_tweaks::on_line(&mut GameHost { game: &mut *game }, &line);
            }
        }
        count_joins(game);
        omnitool_tps::keep_only(tps_source(game).as_deref());
        core::tick(&mut GameHost { game: &mut *game });
        push_render_settings(game);
        freecam_tick(game);
        sample_overlay(game);
        light_overlay_tick(game);
        ranges_tick(game);
    }
    world_map_check(game);
}

// ------------------------------------------------------------ world lines
// docs/ARCHITECTURE.md "World lines": the frame's gizmo lines come from
// `fishing_line` (game/fishing.rs:782, called once per frame at
// game.rs:12371, before the F3 debug gizmos are added). hook_world_lines.rs
// runs the original through this guard and appends Omnitool's lines.

thread_local! {
    static IN_WORLD_LINES: Cell<bool> = const { Cell::new(false) };
}

/// The cancellable HEAD of `fishing_line`: on re-entry the original body
/// runs (Continue); otherwise the original result plus the world lines of
/// every active module (Break).
pub(super) fn world_lines(
    game: &Game<'_>,
    alpha: f64,
    size: [u32; 2],
) -> std::ops::ControlFlow<Vec<horizon_client::gizmos::Line>> {
    use std::ops::ControlFlow;
    if IN_WORLD_LINES.get() || !game.native_on(core::ID) {
        return ControlFlow::Continue(());
    }
    IN_WORLD_LINES.set(true);
    let mut lines = game.fishing_line(alpha, size);
    IN_WORLD_LINES.set(false);
    let frame = world_frame(game, alpha);
    let ours = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        omnitool_world_lines::draw(&ReadHost { game }, &frame)
    }))
    .unwrap_or_default();
    lines.extend(ours.into_iter().map(|l| horizon_client::gizmos::Line {
        start: l.start,
        end: l.end,
        color: l.color,
        width: l.width,
    }));
    ControlFlow::Break(lines)
}

/// What the world-line modules read from the game this frame: the player
/// (`player.position`, motion.rs:272), the dimension (`Dim::id`, as
/// `native_player`, game/native_hooks.rs:400), the singleplayer seed and
/// the targeted block's outline boxes (`pick`, game.rs:2314, and
/// `outline_boxes`, game.rs:2286, world space with the random offset);
/// none when a mob is targeted first (Java: `hitResult` is an entity).
fn world_frame(game: &Game<'_>, alpha: f64) -> omnitool_world_lines::Frame {
    let target = if target_mob(game).is_some() {
        Vec::new()
    } else {
        game.pick()
            .map(|h| game.outline_boxes(h.pos))
            .unwrap_or_default()
    };
    omnitool_world_lines::Frame {
        player: game
            .player
            .as_ref()
            .map(|p| [p.position.x, p.position.y, p.position.z]),
        eye: eye(game, alpha),
        dim: game.dim.id().to_string(),
        seed: slime_seed(game),
        target,
    }
}

/// `player.getEyePosition(pt)`: the feet between the previous and the
/// current position (`previous_position`, as `rod_hand_position`,
/// game/fishing.rs:810) plus the eye height (`dimensions.eye_height`).
fn eye(game: &Game<'_>, alpha: f64) -> Option<[f64; 3]> {
    let p = game.player.as_ref()?;
    let feet = game.previous_position.lerp(p.position, alpha);
    Some([feet.x, feet.y + f64::from(p.dimensions.eye_height), feet.z])
}

/// The block entities and blocks Spawner Range and Beacon Range scan
/// (world-ranges.md §1): `world.block_entities` (world positions, as
/// game/beacon.rs:115) in loaded chunks (`world.chunks`) within the square
/// of chunks around the player, told apart by the block there
/// (`block_name`, game/end.rs:354).
struct Ranges<'a, 'g> {
    game: &'a Game<'g>,
}

impl super::omnitool_world_ranges::RangeWorld for Ranges<'_, '_> {
    fn block_entities(&self, chunks: i32) -> Vec<([i32; 3], super::omnitool_world_ranges::Found)> {
        use super::omnitool_world_ranges::Found;
        let Some([px, _, pz]) = player_block(self.game) else {
            return Vec::new();
        };
        let (cx, cz) = (px >> 4, pz >> 4);
        let mut out = Vec::new();
        for (&(x, y, z), _) in self.game.world.block_entities.iter() {
            let (bx, bz) = (x >> 4, z >> 4);
            if (bx - cx).abs() > chunks
                || (bz - cz).abs() > chunks
                || !self.game.world.chunks.contains_key(&(bx, bz))
            {
                continue;
            }
            let kind = match &*self.game.block_name([x, y, z]) {
                "minecraft:spawner" => Found::Spawner,
                "minecraft:beacon" => Found::Beacon,
                "minecraft:conduit" => Found::Conduit,
                _ => continue,
            };
            out.push(([x, y, z], kind));
        }
        out
    }
    fn block(&self, pos: [i32; 3]) -> String {
        self.game.block_name(pos).to_string()
    }
}

/// Spawn Spheres' switch watch and the range scans.
fn ranges_tick(game: &Game<'_>) {
    let host = ReadHost { game };
    super::omnitool_world_ranges::spheres_tick(&host, eye(game, 1.0));
    super::omnitool_world_ranges::ranges_tick(&host, &Ranges { game });
}

/// Light Overlay's tick (world-render.md §1.3) with the shared zombie spawn
/// test of `spawn_spot` and `light`, on one mob world for the whole scan.
fn light_overlay_tick(game: &Game<'_>) {
    let Some(center) = player_block(game) else {
        return;
    };
    let dim = game.dim.id().to_string();
    let host = ReadHost { game };
    if !core::active(&host, &super::omnitool_light_overlay::MODULE) {
        // clears the spots; the mob world is not built for nothing
        super::omnitool_light_overlay::tick(&host, center, &dim, &mut |_| None);
        return;
    }
    let w = game.mob_world_base(&[]);
    let mut probe = |[x, y, z]: [i32; 3]| {
        (spawn_rules::is_valid_spawn(Species::Zombie, &w, x, y - 1, z)
            && spawn_rules::is_valid_empty_spawn(Species::Zombie, &w, x, y, z))
        .then(|| light(game, [x, y, z]))
    };
    super::omnitool_light_overlay::tick(&host, center, &dim, &mut probe);
}

/// `native_command`: /ot and the modules' commands.
pub(super) fn command(game: &mut Game<'_>, command: &str, ran: &mut bool) {
    if *ran || !game.native_on(core::ID) {
        return;
    }
    let mut host = GameHost { game };
    *ran = core::command(&mut host, command);
    if !*ran {
        // goes on to the server: Chat Log's `sendCommand`
        super::omnitool_chat_log::sent(&mut host, command, true);
    }
}

/// `native_mods`: Omnitool's row in Options > Mods.
pub fn listed(mods: &mut Vec<NativeMod>) {
    mods.push(NativeMod {
        id: core::ID.into(),
        title: format!("{} {}", core::NAME, core::VERSION),
        description: core::DESCRIPTION.into(),
        keys: Vec::new(),
    });
}

/// `native_settings`: Omnitool's page, one button per module.
pub(super) fn settings(game: &mut Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    if mod_id == core::ID {
        buttons.extend(core::settings_labels(&GameHost { game }));
    }
}

/// `native_settings_click`: a module's button switches it.
pub(super) fn settings_click(game: &mut Game<'_>, mod_id: &str, button: usize) {
    if mod_id == core::ID {
        core::settings_click(&mut GameHost { game }, button);
    }
}

/// `native_key`: shortcut keys of the modules (presses only while playing).
pub(super) fn key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if game.native_on(core::ID) {
        let playing = game.native_playing();
        core::key_event(&mut GameHost { game }, key, pressed, playing);
    }
}

// ------------------------------------------------------------ render tweaks

thread_local! {
    static FOG_BUSY: Cell<bool> = const { Cell::new(false) };
}

/// Held while `frame_fog` runs its original body from the fog hook, so the hook
/// does not run again inside it.
pub struct FogBusy;

impl FogBusy {
    pub fn enter() -> Option<FogBusy> {
        (!FOG_BUSY.replace(true)).then_some(FogBusy)
    }
}

impl Drop for FogBusy {
    fn drop(&mut self) {
        FOG_BUSY.set(false);
    }
}

/// Whether No Fog or Clear Water may change this frame's fog.
pub(super) fn fog_wanted(game: &Game<'_>) -> bool {
    let host = ReadHost { game };
    game.native_on(core::ID)
        && (core::active(&host, &omnitool_no_fog::MODULE)
            || core::active(&host, &omnitool_clear_water::MODULE))
}

/// No Fog's and Clear Water's change to the environmental fog (`frame_fog`).
pub(super) fn edit_fog(
    game: &Game<'_>,
    fluid: omnitool_no_fog::Fluid,
    blindness: bool,
    darkness: bool,
    start: &mut f32,
    end: &mut f32,
) {
    let host = ReadHost { game };
    if let Some((s, e)) =
        omnitool_clear_water::fog_edit(&host, fluid, blindness, darkness, *start, *end)
    {
        (*start, *end) = (s, e);
    }
}

/// Hands the render modules' settings to the render crate (once a tick):
/// the camera-overlay hook (`ScreenEffects::build`) and the hurt-cam hooks
/// read them there.
fn push_render_settings(game: &Game<'_>) {
    use horizon_client::screen_effects::omnitool_bridge::{
        Camera, Overlays, set_camera, set_hide_crosshair, set_overlays,
    };
    let host = ReadHost { game };
    let fire_on = core::active(&host, &omnitool_low_fire::MODULE);
    let burning = omnitool_low_fire::Burning {
        has_player: game.player.is_some(),
        fire_resistance: game.potions.effects.get("fire_resistance").is_some(),
        creative: game.creative,
    };
    set_overlays(Overlays {
        hide_water: omnitool_clear_water::hide_overlay(&host),
        hide_fire: omnitool_low_fire::hide(&host, burning),
        fire_offset_y: if fire_on {
            omnitool_low_fire::offset_y(&host)
        } else {
            0.0
        },
        fire_scale: if fire_on {
            omnitool_low_fire::scale_factor(&host)
        } else {
            1.0
        },
        fire_alpha: omnitool_low_fire::alpha(&host, 1.0),
        hide_pumpkin: omnitool_no_pumpkin_overlay::hide_texture(&host, Some("misc/pumpkinblur")),
        hide_powder_snow: omnitool_no_pumpkin_overlay::hide_texture(
            &host,
            Some("misc/powder_snow_outline"),
        ),
        hide_portal: omnitool_no_pumpkin_overlay::hide_portal(&host),
    });
    set_hide_crosshair(crosshair_replaced(game));
    set_camera(Camera {
        hurt_tilt: omnitool_no_hurt_cam::tilt(&host, 1.0),
        no_bobbing: omnitool_no_hurt_cam::skip_bobbing(&host, 1),
    });
}

/// Custom Crosshair replaces Vanilla's crosshair (Vanilla's F3 one stays).
fn crosshair_replaced(game: &Game<'_>) -> bool {
    omnitool_custom_crosshair::wanted(&ReadHost { game }) && !game.debug_crosshair()
}

/// Custom Crosshair's rectangles and attack bar (spec 7.4), in GUI pixels
/// after Vanilla's HUD. Nothing in third person or for a spectator that
/// aims at nothing, as Vanilla's own crosshair.
fn draw_crosshair(game: &Game<'_>, batch: &mut HudBatch) {
    use omnitool_custom_crosshair as cc;
    let host = ReadHost { game };
    if !crosshair_replaced(game)
        || game.ui.hide_gui
        || !game.camera_type.first_person()
        || (game.spectator() && !game.spectator_crosshair())
    {
        return;
    }
    let num = |id: &str| core::value(&host, &cc::MODULE, id).num() as i32;
    let flag = |id: &str| core::value(&host, &cc::MODULE, id).on();
    let rgb = |id: &str| core::value(&host, &cc::MODULE, id).color();
    let (cx, cy) = cc::center(batch.width as i32, batch.height as i32);
    let (size, gap) = (num("size"), num("gap"));
    let style = cc::Style::from_index(core::value(&host, &cc::MODULE, "style").index());
    let rects = cc::shape(style, size, gap, num("thickness"), flag("center_dot"));
    let color = cc::color_for(
        flag("dynamic"),
        game.crosshair_living().is_some(),
        rgb("color"),
        rgb("entity_color"),
    ) as u32;
    let mut fill = |r: cc::Rect, argb: u32| {
        game.native_fill(
            batch,
            [
                r[0] as f32,
                r[1] as f32,
                (r[2] - r[0]) as f32,
                (r[3] - r[1]) as f32,
            ],
            argb,
        );
    };
    let moved = |r: &cc::Rect| [r[0] + cx, r[1] + cy, r[2] + cx, r[3] + cy];
    if flag("outline") {
        for r in &rects {
            let r = moved(r);
            fill(
                [r[0] - 1, r[1] - 1, r[2] + 1, r[3] + 1],
                rgb("outline_color") as u32,
            );
        }
    }
    for r in &rects {
        fill(moved(r), color);
    }
    if flag("attack_indicator") && game.settings.attack_indicator == 1 {
        let f = game.hand_strength.scale(0.0, game.main_hand_attack_speed());
        if let Some((back, front)) = cc::attack_bar(cx, cy, gap, size, f) {
            fill(back, 0x8000_0000);
            fill(front, rgb("color") as u32 | 0xFF00_0000);
        }
    }
}

/// `native_panel_event`: the action button of the craft calculator's panel.
pub(super) fn panel_event(game: &mut Game<'_>, mod_id: &str, event: &NativePanelEvent) {
    if mod_id == core::ID && game.native_on(core::ID) {
        if let NativePanelEvent::Action(i) = event {
            super::omnitool_craft_ui::panel_action(&mut GameHost { game }, *i as usize);
        }
    }
}

/// `native_wheel`: Zoom adjusts with the wheel.
pub(super) fn wheel(game: &mut Game<'_>, steps: f32, claimed: &mut bool) {
    if !*claimed && game.native_on(core::ID) {
        *claimed = omnitool_zoom::wheel(&ReadHost { game }, steps);
    }
}

/// HEAD of `draw_stack_tooltip_with` (game/stations.rs:2353): the hook
/// takes over the drawing only while Omnitool is on.
pub(super) fn tooltip_active(game: &Game<'_>) -> bool {
    game.native_on(core::ID)
}

/// The lines of Better Tooltips, Shulker Box Tooltip and AppleSkin on top of
/// the game's (`stack_tooltip_lines`, stations.rs:1818). A crossbow's
/// projectile lines come from inside that call and are not touched.
pub(super) fn tooltip_lines(
    game: &Game<'_>,
    stack: &ItemStack,
    mut lines: Vec<(horizon_client::text::Text, u32)>,
) -> Vec<(horizon_client::text::Text, u32)> {
    let view = tooltip_view(game, stack);
    let extra = omnitool_tooltips::append(&mut ReadHost { game }, &view);
    if !extra.remove.is_empty() {
        lines.retain(|(t, _)| !extra.remove.contains(&t.plain()));
    }
    for line in extra.lines {
        let (r, g, b) = (
            line.color >> 16 & 255,
            line.color >> 8 & 255,
            line.color & 255,
        );
        let color = horizon_client::hud::rgba(r as u8, g as u8, b as u8, 255);
        lines.push((horizon_client::text::Text::legacy(&line.text), color));
    }
    lines
}

/// The stack as the tooltip modules read it: `items().get` for the
/// durability (`max_damage`, items.rs:68, as `stack` above) and `food`
/// (items.rs:77, nutrition and saturation), `consumable` (items.rs:80),
/// `StackData::container` (item_data.rs:27, non-empty slots) and
/// `bundle_contents` (item_data.rs:55). The names are `getHoverName`.
fn tooltip_view(game: &Game<'_>, s: &ItemStack) -> omnitool_tooltips::StackView {
    let info = game.items().get(&s.item);
    let name = |x: &ItemStack| {
        horizon_client::stations::hover_name(&|id| game.pack.names.display_item(id), x)
    };
    let slot = |x: &ItemStack| omnitool_tooltips::Slot {
        name: name(x),
        count: x.count as i32,
    };
    let data = s.data();
    let mut container = data.map(|d| {
        let mut slots: Vec<&(u8, ItemStack)> = d.container.iter().collect();
        slots.sort_by_key(|(n, _)| *n);
        slots.into_iter().map(|(_, x)| slot(x)).collect::<Vec<_>>()
    });
    if container.as_ref().is_some_and(Vec::is_empty) {
        container = None;
    }
    let bundle = data
        .filter(|d| !d.bundle_contents.is_empty() || s.item.ends_with("bundle"))
        .map(|d| d.bundle_contents.iter().map(slot).collect());
    let translate = |k: &str| game.pack.names.display_get(k);
    omnitool_tooltips::StackView {
        id: s.item.to_string(),
        damage: s.damage as i32,
        max_damage: stack(game, s).max_damage,
        food: info.and_then(|i| i.food).map(|(n, sat)| (n as i32, sat)),
        consumable: info.is_some_and(|i| i.consumable.is_some()),
        container,
        bundle,
        vanilla_container: horizon_client::item_tooltip::container_lines(s, &name, &translate),
    }
}

/// `native_zoom`: Zoom's factor (already eased here, so Horizon's own
/// easing is off) and the scale of mouse turning.
pub(super) fn zoom(game: &Game<'_>, zoom: &mut Option<NativeZoom>) {
    if zoom.is_none() && game.native_on(core::ID) {
        *zoom = omnitool_zoom::request(&ReadHost { game }).map(|(factor, turn)| NativeZoom {
            factor,
            smooth: false,
            turn,
        });
    }
}

/// `native_minimap`: Minimap Config's corner, size, zoom, rotation and shape.
pub(super) fn minimap(game: &Game<'_>, minimap: &mut Option<NativeMinimap>) {
    if minimap.is_none() && game.native_on(core::ID) {
        *minimap = omnitool_minimap_cfg::request(&ReadHost { game }).map(|c| NativeMinimap {
            corner: match c.corner {
                HudCorner::TopLeft => Corner::TopLeft,
                HudCorner::TopRight => Corner::TopRight,
                HudCorner::BottomLeft => Corner::BottomLeft,
                HudCorner::BottomRight => Corner::BottomRight,
            },
            size: c.size,
            blocks_per_pixel: c.blocks_per_pixel,
            rotate: c.rotate,
            round: c.round,
        });
    }
}

/// `native_hud`: the HUD modules' boxes, placed by corner.
pub(super) fn hud(game: &mut Game<'_>, batch: &mut HudBatch, corners: &mut Corners) {
    if !game.native_window() || !game.native_on(core::ID) {
        return;
    }
    draw_crosshair(game, batch);
    let boxes = core::hud_frame(&mut GameHost { game: &mut *game });
    for b in boxes {
        let corner = match b.corner {
            HudCorner::TopLeft => Corner::TopLeft,
            HudCorner::TopRight => Corner::TopRight,
            HudCorner::BottomLeft => Corner::BottomLeft,
            HudCorner::BottomRight => Corner::BottomRight,
        };
        if !game.native_corner_shown(corner) {
            continue;
        }
        let (x, y) = corners.place(corner, b.w, b.h);
        for op in &b.ops {
            match op {
                HudOp::Fill {
                    rect: [rx, ry, w, h],
                    argb,
                } => game.native_fill(batch, [x + rx, y + ry, *w, *h], *argb),
                HudOp::Text {
                    text,
                    at: [tx, ty],
                    rgb,
                    shadow,
                } => game.native_text(batch, text, [x + tx, y + ty], *rgb, *shadow),
            }
        }
    }
    // Waypoint labels sit at free screen positions, not in a corner.
    let ops = omnitool_waypoints::world_ops(&GameHost { game: &mut *game });
    for op in &ops {
        match op {
            HudOp::Fill {
                rect: [rx, ry, w, h],
                argb,
            } => game.native_fill(batch, [*rx, *ry, *w, *h], *argb),
            HudOp::Text {
                text,
                at: [tx, ty],
                rgb,
                shadow,
            } => game.native_text(batch, text, [*tx, *ty], *rgb, *shadow),
        }
    }
    clickgui_frame(game, batch);
    world_map_frame(game, batch);
}

// ------------------------------------------------------------ ClickGui

thread_local! {
    /// Ctrl as the last ModifiersChanged said (the menu's Ctrl+V).
    static CTRL: Cell<bool> = const { Cell::new(false) };
}

/// HEAD of `follow_keymap` (game.rs:1480 at fbd7288), the first thing
/// `window_event` (game.rs:13329) does with an event. While the ClickGui is
/// open, presses, the wheel and typed text go to the menu and the event
/// becomes `Occluded(false)`, which `window_event` ignores (its match ends in
/// `_ => {}`, game.rs:14292). Releases pass on so held keys and buttons let
/// go; cursor moves pass on and update `Game.cursor`.
fn clickgui_window_input(event: &mut winit::event::WindowEvent) {
    use omnitool_clickgui::Input as In;
    use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
    use winit::keyboard::PhysicalKey;
    if let WindowEvent::ModifiersChanged(m) = event {
        CTRL.set(m.state().control_key());
    }
    if !omnitool_clickgui::is_open() {
        return;
    }
    let taken = match event {
        WindowEvent::KeyboardInput { event: k, .. } if k.state == ElementState::Pressed => {
            if let PhysicalKey::Code(code) = k.physical_key
                && let Some(glfw) = horizon_client::keybinds::key_name(code)
                    .and_then(super::omnitool_menu::key_code)
            {
                omnitool_clickgui::push(In::Key(glfw, CTRL.get()));
            }
            if let Some(text) = &k.text
                && !CTRL.get()
            {
                for ch in text.chars() {
                    omnitool_clickgui::push(In::Char(ch));
                }
            }
            true
        }
        WindowEvent::MouseInput { state, button, .. } => {
            let b = match button {
                MouseButton::Left => 0,
                MouseButton::Right => 1,
                MouseButton::Middle => 2,
                _ => -1,
            };
            let pressed = *state == ElementState::Pressed;
            if b >= 0 {
                omnitool_clickgui::push(if pressed {
                    In::Click(b)
                } else {
                    In::Release(b)
                });
            }
            pressed
        }
        WindowEvent::MouseWheel { delta, .. } => {
            let v = match delta {
                MouseScrollDelta::LineDelta(_, y) => f64::from(*y),
                MouseScrollDelta::PixelDelta(p) => p.y / 30.0,
            };
            omnitool_clickgui::push(In::Scroll(v));
            true
        }
        _ => false,
    };
    if taken {
        *event = WindowEvent::Occluded(false);
    }
}

/// The ClickGui in `native_hud`: opens on request (gui_key) while playing,
/// releases the cursor (`capture`, game.rs:6104, private), runs one frame
/// with `Game.cursor` (game.rs:1608, GUI pixels) and draws it on top of the
/// HUD; the content scissor is `HudBatch::clip_since`.
fn clickgui_frame(game: &mut Game<'_>, batch: &mut HudBatch) {
    if omnitool_clickgui::take_request()
        && game.native_playing()
        && !game.ui.hide_gui
        && !omnitool_clickgui::is_open()
    {
        omnitool_clickgui::open();
        game.capture(false);
    }
    if !omnitool_clickgui::is_open() {
        return;
    }
    if !game.native_playing() {
        // pause on focus loss, death, a screen the server opened
        omnitool_clickgui::close(&mut GameHost { game: &mut *game });
        return;
    }
    let (mx, my) = (game.cursor.0.floor() as i32, game.cursor.1.floor() as i32);
    let frame = omnitool_clickgui::frame(
        &mut GameHost { game: &mut *game },
        batch.width as i32,
        batch.height as i32,
        mx,
        my,
    );
    if frame.close {
        game.capture(true);
        return;
    }
    let mut clip: Option<(usize, [i32; 4])> = None;
    for op in &frame.ops {
        match op {
            omnitool_clickgui::Op::Fill([x1, y1, x2, y2], argb) => game.native_fill(
                batch,
                [*x1 as f32, *y1 as f32, (x2 - x1) as f32, (y2 - y1) as f32],
                *argb,
            ),
            omnitool_clickgui::Op::Text {
                text,
                x,
                y,
                argb,
                shadow,
            } => game.native_text(batch, text, [*x as f32, *y as f32], *argb, *shadow),
            omnitool_clickgui::Op::Clip(Some(r)) => clip = Some((batch.vertices.len(), *r)),
            omnitool_clickgui::Op::Clip(None) => {
                if let Some((start, [x1, y1, x2, y2])) = clip.take() {
                    batch.clip_since(start, x1 as f32, y1 as f32, x2 as f32, y2 as f32);
                }
            }
        }
    }
}

// ------------------------------------------------------------ camera
// Freelook and Freecam (docs/ARCHITECTURE.md, "Camera"). Targets in Horizon
// fbd7288: `turn_player` (game/perspective.rs:473), `camera_view`
// (game/perspective.rs:215), `camera_orientation` (game.rs:8369).

fn view_of(t: super::perspective::CameraType) -> omnitool_freelook::View {
    use super::perspective::CameraType as C;
    match t {
        C::FirstPerson => omnitool_freelook::View::First,
        C::ThirdPersonBack => omnitool_freelook::View::Back,
        C::ThirdPersonFront => omnitool_freelook::View::Front,
    }
}

fn camera_type_of(v: omnitool_freelook::View) -> super::perspective::CameraType {
    use super::perspective::CameraType as C;
    match v {
        omnitool_freelook::View::First => C::FirstPerson,
        omnitool_freelook::View::Back => C::ThirdPersonBack,
        omnitool_freelook::View::Front => C::ThirdPersonFront,
    }
}

/// Freelook's start/stop edges (camera type saved and set back).
fn sync_freelook(game: &mut Game<'_>) {
    let camera = view_of(game.camera_type);
    if let Some(set) = omnitool_freelook::sync(&ReadHost { game }, camera) {
        game.camera_type = camera_type_of(set);
    }
}

/// The cancellable HEAD of `turn_player`: Break = the camera took the turn.
pub(super) fn turn_player(game: &mut Game<'_>, xo: f64, yo: f64) -> std::ops::ControlFlow<()> {
    use std::ops::ControlFlow;
    if !game.native_on(core::ID) {
        return ControlFlow::Continue(());
    }
    sync_freelook(game);
    if omnitool_freecam::turn(&ReadHost { game }, xo, yo) || omnitool_freelook::turn(xo, yo) {
        ControlFlow::Break(())
    } else {
        ControlFlow::Continue(())
    }
}

thread_local! {
    static IN_CAMERA_VIEW: Cell<bool> = const { Cell::new(false) };
}

/// The cancellable HEAD of `camera_view`: on re-entry the original body runs
/// (Continue). While Freelook runs, the original runs with the camera
/// rotation in place of the player's (so the third-person distance is pulled
/// in along the camera's direction, as Java's `move(-maxZoom)`), then the
/// player's rotation is put back.
pub(super) fn camera_view(
    game: &mut Game<'_>,
    partial: f32,
) -> std::ops::ControlFlow<Option<(glam::DVec3, f32, f32)>> {
    use std::ops::ControlFlow;
    if IN_CAMERA_VIEW.get() || !game.native_on(core::ID) {
        return ControlFlow::Continue(());
    }
    sync_freelook(game);
    if let Some((pos, yaw, pitch)) = omnitool_freecam::view(f64::from(partial)) {
        // The original still runs: it keeps `view` (the camera's state) in step.
        IN_CAMERA_VIEW.set(true);
        let view = game.camera_view(partial);
        IN_CAMERA_VIEW.set(false);
        return ControlFlow::Break(view.map(|_| (glam::DVec3::from_array(pos), yaw, pitch)));
    }
    let Some((yaw, pitch)) = omnitool_freelook::view() else {
        return ControlFlow::Continue(());
    };
    let saved = (game.yaw, game.pitch);
    (game.yaw, game.pitch) = (yaw, pitch);
    IN_CAMERA_VIEW.set(true);
    let view = game.camera_view(partial);
    IN_CAMERA_VIEW.set(false);
    (game.yaw, game.pitch) = saved;
    ControlFlow::Break(view)
}

/// The cancellable HEAD of `camera_orientation`: the camera's rotation.
pub(super) fn camera_orientation(game: &Game<'_>) -> std::ops::ControlFlow<glam::Quat> {
    use std::ops::ControlFlow;
    if !game.native_on(core::ID) {
        return ControlFlow::Continue(());
    }
    let freecam = omnitool_freecam::view(0.0).map(|(_, yaw, pitch)| (yaw, pitch));
    match freecam.or_else(omnitool_freelook::view) {
        Some((yaw, pitch)) => {
            ControlFlow::Break(super::camera_rotation(game.camera_type, yaw, pitch))
        }
        None => ControlFlow::Continue(()),
    }
}

/// Freecam's world: the player (eyes `eye_position`, game.rs:2235; health
/// `PlayerHealth::health`, health.rs:227; a death counts as a new player),
/// block collisions as the spawn check takes them (`no_collision_empty`,
/// game/natural_spawning.rs:311) and the movement keys the player holds
/// (`physical_keys` through the key binds, `translate_all`, game.rs:14069).
impl omnitool_freecam::Env for GameHost<'_, '_> {
    fn body(&self) -> Option<omnitool_freecam::Body> {
        let p = self.game.player.as_ref()?;
        Some(omnitool_freecam::Body {
            eye: self.game.eye_position()?,
            yaw: self.game.yaw,
            pitch: self.game.pitch,
            health: p.health.health(),
            id: u64::from(p.health.dead_or_dying()),
        })
    }
    fn world_key(&self) -> String {
        let dim = self.game.native_player().map_or("", |p| p.dimension);
        let world = match server(self.game) {
            Server::Local => "local".to_owned(),
            Server::Remote(address) => address,
            _ => "unknown".to_owned(),
        };
        format!("{world}|{dim}")
    }
    fn collides(&mut self, min: [f64; 3], max: [f64; 3]) -> bool {
        use horizon_core::geometry::{Aabb, Vec3};
        let mut scene = horizon_client::collisions::SceneCollisions {
            world: &self.game.world,
            binding: self.game.collision,
            subject: None,
        };
        let bounds = Aabb::new(
            Vec3::new(min[0], min[1], min[2]),
            Vec3::new(max[0], max[1], max[2]),
        );
        !horizon_client::collisions::no_collision_empty(&mut scene, bounds)
    }
    fn keys(&self) -> omnitool_freecam::Keys {
        let held = mapped_held(self.game);
        let has = |k| held.contains(&k);
        omnitool_freecam::Keys {
            forward: has(KeyCode::KeyW),
            back: has(KeyCode::KeyS),
            left: has(KeyCode::KeyA),
            right: has(KeyCode::KeyD),
            jump: has(KeyCode::Space),
            sneak: has(KeyCode::ShiftLeft),
            sprint: has(KeyCode::ControlLeft),
        }
    }
    fn glfw_down(&self, key: i32) -> bool {
        let Some(name) = core::vanilla_key_name(key) else {
            return false;
        };
        self.game
            .physical_keys
            .iter()
            .any(|k| horizon_client::keybinds::key_name(*k) == Some(name.as_str()))
    }
}

/// The mapped keys (default key of each mapping) the player physically holds.
fn mapped_held(game: &Game<'_>) -> Vec<KeyCode> {
    game.physical_keys
        .iter()
        .flat_map(|k| game.settings.keys.translate_all(*k))
        .filter_map(Result::ok)
        .collect()
}

/// The movement keys the game reads each tick (remote.rs:4147, game.rs:5505ff).
const MOVE_KEYS: [KeyCode; 7] = [
    KeyCode::KeyW,
    KeyCode::KeyS,
    KeyCode::KeyA,
    KeyCode::KeyD,
    KeyCode::Space,
    KeyCode::ShiftLeft,
    KeyCode::ControlLeft,
];

thread_local! {
    /// The camera type before Freecam started, and whether it holds the keys.
    static FREECAM_SAVED: Cell<Option<super::perspective::CameraType>> = const { Cell::new(None) };
    static FREECAM_BLOCKING: Cell<bool> = const { Cell::new(false) };
}

/// Java's `PlayerInput.restore`: the held movement keys back into `keys`
/// (sneak and sprint as their toggle mappings say, game.rs:5960).
fn restore_keys(game: &mut Game<'_>) {
    let held = mapped_held(game);
    for k in MOVE_KEYS {
        let down = match k {
            KeyCode::ShiftLeft => game.toggles.is_down(0),
            KeyCode::ControlLeft => game.toggles.is_down(1),
            _ => held.contains(&k),
        };
        if down {
            game.keys.insert(k);
        } else {
            game.keys.remove(&k);
        }
    }
}

/// Freecam each tick, after the modules (Toggle Sprint/Sneak set keys) and
/// before the game reads the movement keys (`native_tick` runs first in both
/// ticks, game.rs:5505 and remote.rs:4017). While the camera is steered the
/// movement keys are taken out of `keys`, so the player stands (Java
/// `PlayerInput.block`). The camera type is held: third person behind when
/// `show_player` (Horizon draws the own body only then), else first person.
fn freecam_tick(game: &mut Game<'_>) {
    use super::perspective::CameraType;
    let out = omnitool_freecam::tick(&mut GameHost { game: &mut *game });
    if out.running {
        if FREECAM_SAVED.get().is_none() {
            FREECAM_SAVED.set(Some(game.camera_type));
        }
        game.camera_type = if omnitool_freecam::show_player(&ReadHost { game }) {
            CameraType::ThirdPersonBack
        } else {
            CameraType::FirstPerson
        };
    }
    if out.block_input {
        for k in MOVE_KEYS {
            game.keys.remove(&k);
        }
    } else if FREECAM_BLOCKING.get() {
        restore_keys(game);
    }
    FREECAM_BLOCKING.set(out.block_input);
    if out.stopped
        && let Some(saved) = FREECAM_SAVED.take()
    {
        game.camera_type = saved;
    }
}

// ------------------------------------------------------------ world map screen

thread_local! {
    /// Ctrl as the last ModifiersChanged said.
    static MAP_CTRL: Cell<bool> = const { Cell::new(false) };
}

/// HEAD of `follow_keymap` (game.rs:1480 at fbd7288), the first thing
/// `window_event` (game.rs:13329) does with an event. Each Omnitool screen
/// may take the event; a taken event becomes `Occluded(false)`, which
/// `window_event` ignores (its match ends in `_ => {}`, game.rs:14292).
pub(super) fn window_input(event: &mut winit::event::WindowEvent) {
    clickgui_window_input(event);
    if map_window_input(event) {
        *event = winit::event::WindowEvent::Occluded(false);
    }
}

/// The world map while open: key presses (as vanilla names), button
/// presses and the wheel go to the map. Releases pass on so held keys and
/// buttons let go; cursor moves pass on and update `Game.cursor`.
fn map_window_input(event: &winit::event::WindowEvent) -> bool {
    use omnitool_world_map_screen::Input as In;
    use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
    use winit::keyboard::PhysicalKey;
    if let WindowEvent::ModifiersChanged(m) = event {
        MAP_CTRL.set(m.state().control_key());
    }
    if !omnitool_world_map_screen::is_open() {
        return false;
    }
    match event {
        WindowEvent::KeyboardInput { event: k, .. } if k.state == ElementState::Pressed => {
            if let PhysicalKey::Code(code) = k.physical_key
                && let Some(name) = horizon_client::keybinds::key_name(code)
                && !MAP_CTRL.get()
            {
                omnitool_world_map_screen::push(In::Key(name.to_owned()));
            }
            true
        }
        WindowEvent::MouseInput { state, button, .. } => {
            let b = match button {
                MouseButton::Left => 0,
                MouseButton::Right => 1,
                MouseButton::Middle => 2,
                _ => -1,
            };
            let pressed = *state == ElementState::Pressed;
            if b >= 0 {
                omnitool_world_map_screen::push(if pressed {
                    In::Click(b)
                } else {
                    In::Release(b)
                });
            }
            pressed
        }
        WindowEvent::MouseWheel { delta, .. } => {
            let v = match delta {
                MouseScrollDelta::LineDelta(_, y) => f64::from(*y),
                MouseScrollDelta::PixelDelta(p) => p.y / 30.0,
            };
            omnitool_world_map_screen::push(In::Scroll(v));
            true
        }
        _ => false,
    }
}

/// The atlas slot of a world map texture tile. The pack screen's icon
/// slots (`horizon/pack-icon0..63`, 128x128, hud.rs:104/343) are free
/// while playing: the pack screen writes the icons it shows into them
/// every frame it draws (pack_screen.rs:801 `bind_icons`), and the map
/// composes all its tiles again each time it opens.
fn map_slot_sprite(slot: usize) -> String {
    format!("horizon/pack-icon{slot}")
}

/// Closes the map when Omnitool is off, the game left the playing state
/// (pause, death, a screen the server opened) or no frame drew it for a
/// second (F1 hides the HUD, where `native_hud` runs: extras.rs:226).
fn world_map_check(game: &mut Game<'_>) {
    let playing = game.native_window() && game.native_on(core::ID) && game.native_playing();
    let now = GameHost { game: &mut *game }.now_ms();
    if omnitool_world_map_screen::check(now, playing) == Some(true) {
        game.capture(true);
    }
}

/// The world map in `native_hud`: opens on request while playing,
/// releasing the cursor (`capture`, game.rs:6104, private); runs one frame
/// with `Game.cursor` (game.rs:1608, GUI pixels), queues the composed
/// tiles as atlas patches (`map_patches`, game.rs:1902, uploaded with this
/// frame's HUD at game.rs:12560) and draws over the whole HUD.
fn world_map_frame(game: &mut Game<'_>, batch: &mut HudBatch) {
    if omnitool_world_map_screen::try_open(&mut GameHost { game: &mut *game }) {
        if game.native_playing() {
            game.capture(false);
        } else {
            omnitool_world_map_screen::with(|s| s.close());
        }
    }
    if !omnitool_world_map_screen::is_open() {
        return;
    }
    let mouse = (f64::from(game.cursor.0), f64::from(game.cursor.1));
    let open_key = core::vanilla_key_name(
        core::value(
            &ReadHost { game: &*game },
            &omnitool_world_map::MODULE,
            "open_key",
        )
        .key(),
    );
    let now = GameHost { game: &mut *game }.now_ms();
    let colors = horizon_client::blocks::MAP_COLORS;
    let input = omnitool_world_map_screen::FrameIn {
        w: f64::from(batch.width),
        h: f64::from(batch.height),
        mouse,
        now_ms: now,
        colors: &colors,
        open_key,
    };
    let frame = omnitool_world_map::with_store(|store| {
        let mut src = omnitool_world_map_screen::StoreSource { store, now_ms: now };
        omnitool_world_map_screen::frame(&mut GameHost { game: &mut *game }, &mut src, &input)
    });
    let Some(frame) = frame else {
        // World Map turned off: no store any more
        omnitool_world_map_screen::with(|s| s.close());
        game.capture(true);
        return;
    };
    if frame.close {
        game.capture(true);
        return;
    }
    let (aw, ah) = (game.gui.atlas.width as f32, game.gui.atlas.height as f32);
    for (slot, rgba) in frame.uploads {
        if let Some(r) = game.gui.atlas.sprites.get(&map_slot_sprite(slot)).copied() {
            game.map_patches.push(horizon_client::font::Patch {
                x: (r.u0 * aw).round() as u32,
                y: (r.v0 * ah).round() as u32,
                width: omnitool_world_map_screen::TEX as u32,
                height: omnitool_world_map_screen::TEX as u32,
                rgba,
            });
        }
    }
    for op in &frame.ops {
        match op {
            omnitool_world_map_screen::Op::Fill(rect, argb) => {
                game.native_fill(batch, *rect, *argb)
            }
            omnitool_world_map_screen::Op::Quad(corners, argb) => {
                let [a, r, g, b] = argb.to_be_bytes();
                batch.fill_quad(
                    &game.gui.atlas,
                    *corners,
                    horizon_client::hud::rgba(r, g, b, a),
                );
            }
            omnitool_world_map_screen::Op::Text {
                text,
                x,
                y,
                rgb,
                shadow,
            } => game.native_text(batch, text, [*x, *y], *rgb, *shadow),
            omnitool_world_map_screen::Op::Tex {
                slot,
                rect: [x, y, w, h],
            } => {
                if let Some(r) = game.gui.atlas.sprites.get(&map_slot_sprite(*slot)).copied() {
                    batch.rect_uv(*x, *y, *w, *h, r, u32::MAX);
                }
            }
        }
    }
}
