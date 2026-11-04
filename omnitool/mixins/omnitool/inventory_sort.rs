// Inventory Sort (docs/spec/omnitool/inventory.md, section 2): sorts the
// given menu slots by item id with ordinary pickup clicks. `plan` is pure and
// checked against tests/fixtures/inventory-sort.json. The module switches
// itself off after sorting, so its key acts as a button. Added to the game as
// `game::omnitool_inventory_sort`; also a module of this crate.

use super::omnitool_auto_refill::Stack;
use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use super::omnitool_setting::{Kind, SettingDef};

/// GLFW `KEY_R`.
const KEY_R: i32 = 82;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "inventory_sort",
    name: "Inventory Sort",
    category: Category::Misc,
    description: "Press the key to sort your inventory by item.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "",
    settings: &[SettingDef::new(
        "hotbar",
        "Also sort the hotbar",
        Kind::Bool(false),
    )],
    command: None,
    key: KEY_R,
    hold: false,
    hud: None,
    tick: Some(tick),
    on_key: None,
    hud_right: None,
    on_action: None,
};

fn same_item_same_components(a: &Stack, b: &Stack) -> bool {
    a.id == b.id && a.comp == b.comp
}

/// `same`: both empty, or the same item, components and count.
fn same(a: &Stack, b: &Stack) -> bool {
    (a.is_empty() && b.is_empty())
        || (!a.is_empty() && !b.is_empty() && same_item_same_components(a, b) && a.count == b.count)
}

/// `sortSlots`: the slot ids passed to a pickup click, in order, for the
/// stacks `items` of the slots `ids` (same length, `ids` = target order).
/// Also returns the model's final stacks.
pub fn plan(ids: &[i32], items: &[Stack]) -> (Vec<i32>, Vec<Stack>) {
    let mut current = items.to_vec();
    let mut wanted = items.to_vec();
    // stable: empty last, item id (string order), count descending
    wanted.sort_by(|a, b| {
        a.is_empty()
            .cmp(&b.is_empty())
            .then_with(|| a.id.encode_utf16().cmp(b.id.encode_utf16()))
            .then_with(|| b.count.cmp(&a.count))
    });
    let mut clicks = Vec::new();
    for i in 0..current.len() {
        let want = &wanted[i];
        if same(&current[i], want) {
            continue;
        }
        for j in i + 1..current.len() {
            if !same(&current[j], want) {
                continue;
            }
            // the game would merge the stacks instead of swapping them
            if !current[i].is_empty() && same_item_same_components(&current[i], &current[j]) {
                break;
            }
            clicks.push(ids[j]);
            clicks.push(ids[i]);
            if !current[i].is_empty() {
                clicks.push(ids[j]);
            }
            current.swap(i, j);
            break;
        }
    }
    (clicks, current)
}

/// `sort()`: sorts menu slots 9..35 (and the hotbar 36..44 with `hotbar`).
/// False when the cursor holds a stack or there is no inventory.
pub fn sort(host: &mut dyn Host, hotbar: bool) -> bool {
    let Some(view) = host.refill_view() else {
        return false;
    };
    if !host.carried_empty() {
        host.action_bar("Omnitool: close other containers to sort");
        return false;
    }
    let last = if hotbar { 44 } else { 35 };
    let ids: Vec<i32> = (9..=last).collect();
    let items: Vec<Stack> = ids
        .iter()
        .map(|&i| view.slots.get(i as usize - 9).cloned().unwrap_or_default())
        .collect();
    for slot in plan(&ids, &items).0 {
        host.click_menu_slot(slot as usize);
    }
    true
}

fn tick(host: &mut dyn Host) {
    if !core::enabled(host, &MODULE) {
        return;
    }
    // onEnable: sort, then switch off again
    if core::active(host, &MODULE) {
        let hotbar = core::value(host, &MODULE, "hotbar").on();
        sort(host, hotbar);
    }
    core::set_enabled(host, &MODULE, false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn stack(v: &Value) -> Stack {
        if v.is_null() {
            return Stack::default();
        }
        Stack {
            id: v["id"].as_str().unwrap().to_owned(),
            count: v["count"].as_i64().unwrap() as i32,
            max_stack: 64,
            comp: v["components"].as_str().unwrap().to_owned(),
            ..Stack::default()
        }
    }

    #[test]
    fn sort_fixtures() {
        let f: Value = serde_json::from_str(
            &std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../tests/fixtures/inventory-sort.json"
            ))
            .unwrap(),
        )
        .unwrap();
        for c in f["cases"].as_array().unwrap() {
            let ids: Vec<i32> = c["slotIds"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i32)
                .collect();
            let items: Vec<Stack> = c["items"].as_array().unwrap().iter().map(stack).collect();
            let (clicks, fin) = plan(&ids, &items);
            let want: Vec<i32> = c["clicks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i32)
                .collect();
            assert_eq!(clicks, want, "{}", c["name"]);
            let want_fin: Vec<Stack> = c["finalSlots"]
                .as_array()
                .unwrap()
                .iter()
                .map(stack)
                .collect();
            assert_eq!(fin, want_fin, "{}", c["name"]);
        }
    }
}
