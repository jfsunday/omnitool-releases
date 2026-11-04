// Villager Trades (docs/spec/omnitool/automation.md, section 4): one
// preferred trade per villager profession, in the syntax of the `trade` step.
// Cheaty, beta. The module is a push button: its key trades the villager at
// the crosshair (or in the open trade screen) with the `villager` step; a
// second press stops; Shift + key in the trade screen saves the selected
// trade. Added to the game as `game::omnitool_villager_trades`; also a module
// of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{self as setting, Kind, SettingDef};
use super::omnitool_trading::{self as trading, MerchantView};

const fn profession(id: &'static str, label: &'static str) -> SettingDef {
    SettingDef::new(id, label, Kind::Text(""))
}

/// The professions with a setting, in Java's order.
pub const PROFESSIONS: [&str; 14] = [
    "armorer",
    "butcher",
    "cartographer",
    "cleric",
    "farmer",
    "fisherman",
    "fletcher",
    "leatherworker",
    "librarian",
    "mason",
    "shepherd",
    "toolsmith",
    "weaponsmith",
    "wandering_trader",
];

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "villager_trades",
    name: "Villager Trades",
    category: Category::Misc,
    description: "Set one trade per villager profession (like the trade step: emerald / sell stick / mending). Press the key in the trade screen or while looking at a villager to trade it completely; press again to stop. Shift + key in the trade screen saves the selected trade.",
    default_on: false,
    cheaty: true,
    beta: true,
    fair_variant: false,
    replaces: "",
    settings: &[
        profession("armorer", "Armorer"),
        profession("butcher", "Butcher"),
        profession("cartographer", "Cartographer"),
        profession("cleric", "Cleric"),
        profession("farmer", "Farmer"),
        profession("fisherman", "Fisherman"),
        profession("fletcher", "Fletcher"),
        profession("leatherworker", "Leatherworker"),
        profession("librarian", "Librarian"),
        profession("mason", "Mason"),
        profession("shepherd", "Shepherd"),
        profession("toolsmith", "Toolsmith"),
        profession("weaponsmith", "Weaponsmith"),
        profession("wandering_trader", "Wandering trader"),
        SettingDef::new(
            "drop",
            "Drop results when the inventory is full",
            Kind::Bool(false),
        ),
    ],
    command: None,
    key: setting::NONE,
    hold: false,
    hud: None,
    tick: Some(tick),
    on_key: Some(key_event),
    hud_right: None,
    on_action: None,
};

/// Java's `onEnable`: the module switches itself off again at once and
/// acts. The key turns it on (core's key handling); a settings click or a
/// workflow can too, which `tick` catches.
fn fire(host: &mut dyn Host) {
    let on = core::with(host, |s, host| {
        let blocked = s.blocked(host, &MODULE);
        let st = s.state_mut(&MODULE);
        let on = st.enabled;
        st.enabled = false;
        on && !blocked
    });
    if on {
        press(host);
    }
}

fn tick(host: &mut dyn Host) {
    fire(host);
}

fn key_event(host: &mut dyn Host, _key: &str, pressed: bool) {
    if pressed {
        fire(host);
    }
}

/// The key pressed while a screen is open (core skips module keys there):
/// acts only in the trade screen.
pub fn screen_key(host: &mut dyn Host, key: &str) {
    let bound = core::read(host, |s| {
        let code = s.state(&MODULE).key;
        s.loaded(&MODULE)
            && !s.blocked(host, &MODULE)
            && code != setting::NONE
            && core::vanilla_key_name(code).as_deref() == Some(key)
    });
    let in_trade = host
        .input()
        .and_then(|i| i.trader())
        .is_some_and(|t| t.merchant().is_some());
    if bound && in_trade {
        press(host);
    }
}

/// Java's `onEnable` after switching off: Shift in the trade screen saves
/// the selected trade, else the `villager` run starts (or stops).
fn press(host: &mut dyn Host) {
    let shift = host.shift_down();
    let Some(input) = host.input() else { return };
    let player = input.position();
    let Some(t) = input.trader() else { return };
    let menu = t.merchant();
    if shift && let Some(m) = &menu {
        return save_selected(host, m);
    }
    let at = if menu.is_some() {
        None
    } else {
        trading::target_coords(t, player)
    };
    core::villager_key(host, &at.unwrap_or_default());
}

/// The text `saveSelected` stores for the trade screen, or the hint.
pub fn selected(m: &MerchantView) -> Result<(String, String), &'static str> {
    let prof = trading::profession_of(&m.title);
    if !PROFESSIONS.contains(&prof.as_str()) {
        return Err("Omnitool: unknown villager profession (renamed?)");
    }
    if m.result.count <= 0 || m.result.stack.id.is_empty() {
        return Err("Omnitool: select a trade first (its result must show in the output slot)");
    }
    let id = &m.result.stack.id;
    let path = id.split_once(':').map_or(id.as_str(), |(_, p)| p);
    Ok((prof, path.to_owned()))
}

/// Java's `saveSelected(menu)`.
fn save_selected(host: &mut dyn Host, m: &MerchantView) {
    match selected(m) {
        Err(hint) => host.action_bar(hint),
        Ok((prof, id)) => {
            core::with(host, |s, _| {
                let _ = s.set_setting(&MODULE, &prof, &id);
                s.save();
            });
            host.action_bar(&format!(
                "Omnitool: preferred trade for {}: {id}",
                title(&prof)
            ));
        }
    }
}

/// The preset text of a profession (`presetFor`) and the `drop` setting;
/// None when the module isn't loaded (Java: `VILLAGER_TRADES == null`).
pub fn preset_for(host: &dyn Host, profession: &str) -> Option<(String, bool)> {
    let loaded = core::read(host, |s| s.loaded(&MODULE));
    if !loaded {
        return None;
    }
    let prof = profession.trim().to_lowercase();
    let text = if PROFESSIONS.contains(&prof.as_str()) {
        core::value(host, &MODULE, &prof).text().trim().to_owned()
    } else {
        String::new()
    };
    Some((text, core::value(host, &MODULE, "drop").on()))
}

/// `title()`: the profession with a capital first letter and spaces for `_`.
pub fn title(profession: &str) -> String {
    let spaced = profession.replace('_', " ");
    let mut c = spaced.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn title_fixtures() {
        let f: Value = serde_json::from_str(
            &std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../tests/fixtures/trading.json"
            ))
            .unwrap(),
        )
        .unwrap();
        for c in f["villager_title"].as_array().unwrap() {
            assert_eq!(
                title(c["profession"].as_str().unwrap()),
                c["expect"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn selected_trade() {
        use super::super::omnitool_trading::{NameText, Slot, Stack};
        let mut m = MerchantView {
            title: NameText {
                translatable: Some("entity.minecraft.villager.farmer".into()),
                siblings: Vec::new(),
            },
            ..MerchantView::default()
        };
        assert_eq!(
            selected(&m),
            Err("Omnitool: select a trade first (its result must show in the output slot)")
        );
        m.result = Slot {
            stack: Stack {
                id: "minecraft:emerald".into(),
                ..Stack::default()
            },
            count: 1,
            max_stack: 64,
        };
        assert_eq!(selected(&m), Ok(("farmer".into(), "emerald".into())));
        m.title.translatable = None;
        assert_eq!(
            selected(&m),
            Err("Omnitool: unknown villager profession (renamed?)")
        );
    }

    #[test]
    fn settings_follow_the_profession_list() {
        let ids: Vec<&str> = MODULE.settings.iter().map(|s| s.id).collect();
        assert_eq!(&ids[..14], PROFESSIONS);
        assert_eq!(ids[14], "drop");
        for s in &MODULE.settings[..14] {
            assert_eq!(s.name, title(s.id));
        }
    }
}
