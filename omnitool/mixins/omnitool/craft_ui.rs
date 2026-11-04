// The craft calculator's surface in Horizon (docs/spec/omnitool/calculator.md,
// section 2 and 8.5): `/craft <item> [amount]` prints the tree in chat and
// shows the crafting steps in the item panel (`native_panel_recipes`), where
// the action button puts a step's recipe into the open crafting grid
// (`native_place_recipe`). `/craft learned` lists the learned recipes the
// same way (the button forgets one); `/craft forget <item>` does it by name.
// Java has an inventory tab here; Horizon has no screen hook for that, so
// this is the panel instead. Added to the game as `game::omnitool_craft_ui`;
// also a module of this crate.

use super::omnitool_config as config;
use super::omnitool_core::{self as core, Host};
use super::omnitool_craft_calc::{
    CraftCalc, CraftData, ERR_NO_RECIPE, PanelRecipe, Snapshot, craft_steps, tree_rows,
};
use super::omnitool_craft_option::{CraftOption, usable};
use super::omnitool_craft_tree::{Node, Recipe};
use super::omnitool_learned_recipes::{Learned, Pattern};
use super::omnitool_workflows::item_matches;
use std::cell::RefCell;
use std::collections::HashMap;

/// What the panel shows now, so its action button knows what it means.
#[derive(Default)]
enum View {
    #[default]
    None,
    /// The recipe book handle of each step shown (None: not placeable now).
    Steps(Vec<Option<String>>),
    /// The learned patterns' result ids, as shown.
    Learned(Vec<String>),
}

#[derive(Default)]
struct Ui {
    calc: CraftCalc,
    learned: Option<Learned>,
    view: View,
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

pub const HELP: &[&str] = &[
    " /craft <item> [amount] shows everything you need down to the raw materials; the item panel lists the steps",
    " Use a step's button there to put its recipe into the open crafting grid",
    " /craft learned lists learned recipes (the button forgets one), /craft forget <item>",
];

/// `/craft ...`: the calculator's command (answered by the Calculator module).
pub fn command(host: &mut dyn Host, name: &str, args: &str) -> bool {
    if name != "craft" {
        return false;
    }
    let args = args.trim();
    let (first, rest) = args.split_once(' ').unwrap_or((args, ""));
    match first.to_lowercase().as_str() {
        "" => {
            host.chat("§6Omnitool craft calculator");
            for line in HELP {
                let line = core::tr(host, line.trim_start());
                host.chat(&format!("§7 {line}"));
            }
        }
        "learned" => show_learned(host),
        "forget" => forget(host, rest.trim()),
        _ => calculate(host, args),
    }
    true
}

/// The learned recipes, loaded on first use.
fn learned<'a>(host: &dyn Host, ui: &'a mut Ui) -> &'a mut Learned {
    ui.learned.get_or_insert_with(|| {
        let root = host.data_dir().and_then(|d| config::read(&d.join(FILE)));
        Learned::from_json(root.as_ref())
    })
}

const FILE: &str = "recipes.json";

fn save(host: &dyn Host, l: &Learned) {
    if let Some(dir) = host.data_dir() {
        config::write(&dir.join(FILE), &l.to_json());
    }
}

fn calculate(host: &mut dyn Host, query: &str) {
    let Some(data) = host.craft_data() else {
        let msg = core::tr(host, "Join a world first.");
        return core::error(host, &msg);
    };
    let (rows, raw, missing, complete, steps, panel, view, shown) = UI.with_borrow_mut(|ui| {
        let l = learned(host, ui);
        // `learned` borrows ui; the calculator needs both, so take it out
        let l = std::mem::take(l);
        let ok = ui.calc.calculate(&Snapshot(&data), &l, query);
        *ui.learned.as_mut().unwrap() = l;
        if !ok {
            let msg = ui.calc.error.clone();
            return (
                Vec::new(),
                Vec::new(),
                Vec::new(),
                false,
                Vec::new(),
                Vec::new(),
                None,
                msg,
            );
        }
        let calc = &ui.calc;
        let r = calc.result.as_ref().unwrap();
        let have = |id: &str| calc.inventory.get(id).copied().unwrap_or(0);
        let raw: Vec<(String, i64, i64)> = r
            .raw
            .iter()
            .map(|(id, n)| (calc.name(id), *n, have(id)))
            .collect();
        let missing: Vec<(String, i64)> = r
            .missing
            .iter()
            .map(|(id, n)| (calc.name(id), *n))
            .collect();
        let (panel, handles) = steps_panel(calc, &data);
        (
            tree_rows(calc),
            raw,
            missing,
            r.complete(),
            craft_steps(r),
            panel,
            Some(handles),
            None,
        )
    });
    if let Some(err) = shown {
        let mut msg = core::tr(host, &err);
        if err == ERR_NO_RECIPE {
            msg = format!("{msg} \"{}\"", query.trim());
        }
        return core::error(host, &msg);
    }
    let (have_w, craft_w, missing_w) = (
        core::tr(host, "have"),
        core::tr(host, "craft"),
        core::tr(host, "missing"),
    );
    for (depth, label, info) in rows {
        let bad = info.contains("missing");
        let info = info
            .replace("have", &have_w)
            .replace("craft", &craft_w)
            .replace("missing", &missing_w);
        let (c1, c2) = if bad { ("§e", "§c") } else { ("§f", "§7") };
        let tail = if info.is_empty() {
            String::new()
        } else {
            format!(" {c2}({info})")
        };
        host.chat(&format!("{c1}{}{label}{tail}", "  ".repeat(depth)));
    }
    host.chat(&format!(
        "§b{}",
        core::tr(host, "Raw materials (from scratch):")
    ));
    for (name, n, have) in raw {
        let c = if have >= n { "§a" } else { "§f" };
        host.chat(&format!("{c}  {n}x {name}  ({have_w} {have})"));
    }
    if complete {
        host.chat(&format!(
            "§a{}",
            core::tr(host, "Nothing missing - you can craft it.")
        ));
    } else {
        host.chat(&format!("§c{}", core::tr(host, "Missing:")));
        for (name, n) in missing {
            host.chat(&format!("§e  {n}x {name}"));
        }
    }
    host.chat("§7Steps:");
    for s in steps {
        host.chat(&format!("§7  {s}"));
    }
    UI.with_borrow_mut(|ui| ui.view = view.map_or(View::None, View::Steps));
    let title = format!("Craft calculator: {}", query.trim());
    host.craft_panel(&title, &panel);
}

/// The recipe of each node of the tree, first one per item.
fn node_recipes(n: &Node, out: &mut HashMap<String, Recipe>) {
    if let Some(r) = &n.recipe {
        out.entry(n.item.clone()).or_insert_with(|| r.clone());
    }
    for c in &n.children {
        node_recipes(c, out);
    }
}

/// A recipe as panel cells: every ingredient `count` times, at most 3x3.
fn grid(recipe: &Recipe) -> (usize, usize, Vec<Vec<String>>) {
    let mut cells: Vec<Vec<String>> = Vec::new();
    for ing in &recipe.ingredients {
        for _ in 0..ing.count {
            cells.push(ing.options.clone());
        }
    }
    cells.truncate(9);
    let n = cells.len().max(1);
    let width = match n {
        1 => 1,
        2 => 2,
        3..=4 => 2,
        _ => 3,
    };
    let height = n.div_ceil(width);
    cells.resize(width * height, Vec::new());
    (width, height, cells)
}

/// One panel recipe per step (ingredients first) and the recipe book handle
/// that can place it now, if any (the most crafts the inventory allows first).
fn steps_panel(calc: &CraftCalc, data: &CraftData) -> (Vec<PanelRecipe>, Vec<Option<String>>) {
    let result = calc.result.as_ref().unwrap();
    let mut recipes = HashMap::new();
    node_recipes(&result.root, &mut recipes);
    let have: HashMap<String, i64> = calc.inventory.clone();
    let mut panel = Vec::new();
    let mut handles = Vec::new();
    for s in &result.steps {
        let Some(r) = recipes.get(&s.item) else {
            continue;
        };
        let (width, height, slots) = grid(r);
        let options: Vec<CraftOption> = data
            .options
            .iter()
            .filter(|(id, _)| *id == s.item)
            .map(|(_, o)| o.clone())
            .collect();
        let handle = usable(&options, &have, true)
            .first()
            .map(|o| o.handle.clone());
        panel.push(PanelRecipe {
            width,
            height,
            slots,
            result: (r.result.clone(), r.count),
            note: format!("craft {}x ({}x {})", s.crafts, s.amount, calc.name(&s.item)),
            action: Some(handle.is_some()),
        });
        handles.push(handle);
    }
    (panel, handles)
}

fn show_learned(host: &mut dyn Host) {
    let patterns: Vec<Pattern> = UI.with_borrow_mut(|ui| learned(host, ui).all().to_vec());
    if patterns.is_empty() {
        let msg = core::tr(
            host,
            "Learned recipes: put a recipe into any crafting grid once and Omnitool remembers it. The Craft step uses the recipe book first and these patterns as fallback (also for recipes that are not unlocked).",
        );
        UI.with_borrow_mut(|ui| ui.view = View::None);
        return core::ok(host, &format!("No learned recipes. {msg}"));
    }
    let panel: Vec<PanelRecipe> = patterns.iter().map(learned_view).collect();
    for p in &patterns {
        host.chat(&format!("§7{}", pattern_line(p)));
    }
    UI.with_borrow_mut(|ui| {
        ui.view = View::Learned(patterns.iter().map(|p| p.result.clone()).collect())
    });
    host.craft_panel("Learned recipes (the button forgets one)", &panel);
}

/// Java's pattern row (not translated).
fn pattern_line(p: &Pattern) -> String {
    format!(
        "{}x {}  ({}x{}, craft: {})",
        p.count,
        p.name,
        p.width,
        p.height,
        p.result.strip_prefix("minecraft:").unwrap_or(&p.result)
    )
}

fn learned_view(p: &Pattern) -> PanelRecipe {
    let mut slots = Vec::new();
    for y in 0..p.height {
        for x in 0..p.width {
            let c = p.cell(x, y);
            slots.push(if c.is_empty() {
                Vec::new()
            } else {
                vec![c.to_owned()]
            });
        }
    }
    PanelRecipe {
        width: p.width,
        height: p.height,
        slots,
        result: (p.result.clone(), p.count),
        note: pattern_line(p),
        action: Some(true),
    }
}

fn forget(host: &mut dyn Host, query: &str) {
    if query.is_empty() {
        return core::fail(host, "Usage: /craft forget <item>");
    }
    let found = UI.with_borrow_mut(|ui| {
        let l = learned(host, ui);
        let hit = l
            .find(|r, n| item_matches(query, r, n))
            .map(|p| (p.result.clone(), p.name.clone()));
        if let Some((r, _)) = &hit {
            l.forget(r);
            save(host, l);
        }
        hit
    });
    match found {
        Some((_, name)) => core::ok(host, &format!("Forgot recipe: {name}")),
        None => core::fail(host, &format!("No learned recipe for \"{query}\"")),
    }
}

/// The panel's action button (`NativePanelEvent::Action`) of recipe `index`.
pub fn panel_action(host: &mut dyn Host, index: usize) {
    enum Do {
        Place(Option<String>),
        Forget(String),
        Nothing,
    }
    let todo = UI.with_borrow(|ui| match &ui.view {
        View::Steps(h) => h.get(index).map_or(Do::Nothing, |h| Do::Place(h.clone())),
        View::Learned(r) => r.get(index).map_or(Do::Nothing, |r| Do::Forget(r.clone())),
        View::None => Do::Nothing,
    });
    match todo {
        Do::Nothing => {}
        Do::Place(None) => core::fail(host, "You lack the ingredients for this step."),
        Do::Place(Some(handle)) => {
            if let Err(e) = host.craft_place(&handle) {
                core::fail(host, &e);
            }
        }
        Do::Forget(result) => {
            UI.with_borrow_mut(|ui| {
                let l = learned(host, ui);
                if l.forget(&result) {
                    save(host, l);
                }
            });
            core::ok(host, &format!("Forgot recipe: {result}"));
            show_learned(host);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::Fake;
    use crate::omnitool_craft_calc::BookRecipe;

    fn s(id: &str, name: &str) -> (String, String) {
        (id.to_owned(), name.to_owned())
    }

    fn host() -> Fake {
        let planks = || vec![s("minecraft:oak_planks", "Oak Planks")];
        let opt = |handle: &str, slots: Vec<Vec<&str>>| CraftOption {
            handle: handle.into(),
            slots: slots
                .into_iter()
                .map(|s| s.into_iter().map(String::from).collect())
                .collect(),
            needs_table: false,
        };
        Fake {
            craft: Some(CraftData {
                book: vec![
                    BookRecipe {
                        result: s("minecraft:stick", "Stick"),
                        count: 4,
                        slots: vec![planks(), planks()],
                    },
                    BookRecipe {
                        result: s("minecraft:oak_planks", "Oak Planks"),
                        count: 4,
                        slots: vec![vec![s("minecraft:oak_log", "Oak Log")]],
                    },
                ],
                options: vec![
                    (
                        "minecraft:stick".into(),
                        opt(
                            "7",
                            vec![vec!["minecraft:oak_planks"], vec!["minecraft:oak_planks"]],
                        ),
                    ),
                    (
                        "minecraft:oak_planks".into(),
                        opt("3", vec![vec!["minecraft:oak_log"]]),
                    ),
                ],
                inventory: vec![("minecraft:oak_planks".into(), "Oak Planks".into(), 3)],
            }),
            dir: Some(crate::omnitool_config::tests::temp_dir()),
            ..Fake::default()
        }
    }

    fn run(h: &mut Fake, line: &str) -> Vec<String> {
        h.lines.clear();
        assert!(core::command(h, line));
        h.lines.clone()
    }

    #[test]
    fn calculates_and_shows_steps() {
        let mut h = host();
        let out = run(&mut h, "craft stick 8");
        assert_eq!(out[0], "§f8x Stick §7(craft 2x)");
        assert!(out.contains(&"§bRaw materials (from scratch):".to_owned()));
        assert!(out.contains(&"§cMissing:".to_owned()));
        assert!(out.contains(&"§7  craft:minecraft:stick 8".to_owned()));
        let (title, panel) = h.panels.last().unwrap();
        assert_eq!(title, "Craft calculator: stick 8");
        assert_eq!(panel.len(), 2);
        // planks first (3 logs missing makes it unplaceable, but 1 log isn't held: no stock)
        assert_eq!(panel[1].result, ("minecraft:stick".to_owned(), 4));
        assert_eq!((panel[1].width, panel[1].height), (2, 1));
        assert_eq!(panel[1].action, Some(true));
        assert_eq!(panel[0].action, Some(false));
        panel_action(&mut h, 1);
        panel_action(&mut h, 0);
        assert_eq!(h.placed, ["7"]);
        assert_eq!(
            h.lines.last().unwrap(),
            "§c[Omnitool] You lack the ingredients for this step."
        );
    }

    #[test]
    fn errors() {
        let mut h = host();
        assert_eq!(
            run(&mut h, "craft diamond 2"),
            ["§cNo known crafting recipe for \"diamond 2\""]
        );
        h.craft = None;
        assert_eq!(run(&mut h, "craft stick"), ["§cJoin a world first."]);
        assert_eq!(run(&mut h, "craft")[0], "§6Omnitool craft calculator");
    }

    #[test]
    fn learned_list_and_forget() {
        let mut h = host();
        let grid: Vec<String> = ["a", "a", "", "b"].iter().map(|x| x.to_string()).collect();
        let p = crate::omnitool_learned_recipes::trim("minecraft:gizmo", "Gizmo", 2, 2, 2, &grid)
            .unwrap();
        UI.with_borrow_mut(|ui| {
            let mut l = Learned::default();
            l.learn(p);
            ui.learned = Some(l);
        });
        let out = run(&mut h, "craft learned");
        assert_eq!(out, ["§72x Gizmo  (2x2, craft: gizmo)"]);
        let (_, panel) = h.panels.last().unwrap();
        assert_eq!(panel[0].slots, [vec!["a"], vec!["a"], vec![], vec!["b"]]);
        assert_eq!(
            run(&mut h, "craft forget gizmo"),
            ["§6[Omnitool] §fForgot recipe: Gizmo"]
        );
        assert_eq!(
            run(&mut h, "craft forget gizmo"),
            ["§c[Omnitool] No learned recipe for \"gizmo\""]
        );
    }
}
