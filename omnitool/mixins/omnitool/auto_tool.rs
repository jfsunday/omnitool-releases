// Auto Tool (docs/spec/omnitool/automation.md, section 1): while the attack
// key is held on a block, selects the hotbar slot with the fastest tool and
// switches back after the key is released. Cheaty. Java hooks
// `startDestroyBlock`/`continueDestroyBlock`; Horizon has no stable hook
// there, so `tick` stands in: attack key held and a block in reach. The
// selection rule is pure (`best_slot`). Added to the game as
// `game::omnitool_auto_tool`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, HotbarTool, ModuleInfo};
use super::omnitool_setting::{self as setting, Kind, SettingDef};
use std::cell::Cell;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "auto_tool",
    name: "Auto Tool",
    category: Category::Player,
    description: "Picks the fastest tool from your hotbar when mining.",
    default_on: false,
    cheaty: true,
    beta: false,
    fair_variant: false,
    replaces: "",
    settings: &[SettingDef::new(
        "switch_back",
        "Switch back afterwards",
        Kind::Bool(true),
    )],
    command: None,
    key: setting::NONE,
    hold: false,
    hud: None,
    tick: Some(tick),
    on_key: None,
    hud_right: None,
    on_action: None,
};

thread_local! {
    /// Java's `previousSlot`, -1 = no switch open.
    static PREVIOUS: Cell<i32> = const { Cell::new(-1) };
}

/// `onMining` step 2-3: the slot with the strictly highest speed, starting
/// from `current`; slots in `skip` (almost broken) are not candidates.
pub fn best_slot(current: usize, speeds: &[f64], skip: &[bool]) -> usize {
    let mut best = current;
    let mut best_speed = speeds.get(current).copied().unwrap_or(1.0);
    for (i, &s) in speeds.iter().enumerate().take(9) {
        if skip.get(i).copied().unwrap_or(false) {
            continue;
        }
        if s > best_speed {
            best_speed = s;
            best = i;
        }
    }
    best
}

/// Java's `Durability.isAlmostBroken`: Durability Warning is active with
/// `prevent_break` and at most `prevent_at` durability is left.
fn almost_broken(host: &dyn Host, tool: &HotbarTool) -> bool {
    use super::omnitool_durability_warning::MODULE as DW;
    let Some(left) = tool.durability_left else {
        return false;
    };
    core::active(host, &DW)
        && core::value(host, &DW, "prevent_break").on()
        && f64::from(left) <= core::value(host, &DW, "prevent_at").num()
}

fn tick(host: &mut dyn Host) {
    let attack = host.mapping("key.attack").is_some_and(|m| m.0);
    let switch_back = core::value(host, &MODULE, "switch_back").on();
    // Java's `tick`: runs even when the module is off, to clean up.
    let previous = PREVIOUS.with(Cell::get);
    if previous >= 0 && (!switch_back || !attack) {
        if switch_back && let Some(input) = host.input() {
            input.select_slot(previous as usize);
        }
        PREVIOUS.with(|p| p.set(-1));
    }
    if !attack || host.screen_open() || !core::active(host, &MODULE) {
        return;
    }
    let Some(tools) = host.mining_hotbar() else {
        return;
    };
    let current = host.hotbar_slot().clamp(0, 8) as usize;
    let speeds: Vec<f64> = tools.iter().map(|t| t.speed).collect();
    let skip: Vec<bool> = tools.iter().map(|t| almost_broken(host, t)).collect();
    let best = best_slot(current, &speeds, &skip);
    if best != current {
        PREVIOUS.with(|p| {
            if p.get() < 0 {
                p.set(current as i32);
            }
        });
        if let Some(input) = host.input() {
            input.select_slot(best);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strictly_faster_wins_lowest_index_on_ties() {
        let speeds = [1.0, 4.0, 4.0, 8.0, 8.0, 1.0, 1.0, 1.0, 1.0];
        assert_eq!(best_slot(0, &speeds, &[false; 9]), 3);
        // The held slot wins a tie with a later one.
        assert_eq!(best_slot(4, &speeds, &[false; 9]), 4);
        assert_eq!(best_slot(1, &[1.0, 4.0, 4.0], &[false; 3]), 1);
    }

    #[test]
    fn almost_broken_tools_are_skipped_but_the_held_one_keeps_its_speed() {
        let speeds = [1.0, 8.0, 4.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        let mut skip = [false; 9];
        skip[1] = true;
        assert_eq!(best_slot(0, &speeds, &skip), 2);
        // Held (broken) slot: its speed is the start value, others must beat it.
        assert_eq!(best_slot(1, &speeds, &skip), 1);
    }

    #[test]
    fn registered_as_cheaty() {
        let m = crate::omnitool_registry::MODULES
            .iter()
            .find(|m| m.id == "auto_tool")
            .unwrap();
        assert!(m.cheaty && !m.fair_variant);
    }
}
