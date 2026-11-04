// The craft calculator's logic (docs/spec/omnitool/calculator.md, sections 2
// and 5): `CraftCalc.calculate` (parsing and item matching are the workflow
// step's, `omnitool_workflows`) with the recipe book and inventory behind the
// `RecipeSource` trait, filled from native_recipes / native_known_recipes
// (API 2) as a `CraftData` snapshot. The UI is `/craft` plus the item panel
// (craft_ui.rs) instead of Java's inventory tab. Added to the game as `game::omnitool_craft_calc`; also a module of this crate.

use super::omnitool_craft_option::CraftOption;
use super::omnitool_craft_tree::{self as tree, Ingredient, Recipe};
use super::omnitool_learned_recipes::Learned;
use super::omnitool_workflows::{item_matches, parse_craft};
use std::collections::HashMap;

/// A recipe-book entry as the game reports it: every slot lists its
/// alternative stacks as (id, shown name).
#[derive(Clone)]
pub struct BookRecipe {
    pub result: (String, String),
    pub count: i64,
    pub slots: Vec<Vec<(String, String)>>,
}

/// Where the calculator gets its data (the game, or a fake in tests).
pub trait RecipeSource {
    /// False without a player and level.
    fn in_world(&self) -> bool;
    /// Unlocked crafting recipes in recipe book order.
    fn book(&self) -> Vec<BookRecipe>;
    /// Stacks of the inventory menu slots 9..45 (main, hotbar, offhand;
    /// not armor or the crafting grid): (id, shown name, count).
    fn inventory(&self) -> Vec<(String, String, i64)>;
}

/// What the game gave for one calculation (`Host::craft_data`).
#[derive(Clone, Default)]
pub struct CraftData {
    /// Unlocked crafting recipes in recipe book order.
    pub book: Vec<BookRecipe>,
    /// The same recipes as placeable options: (result id, option); the
    /// handle is the recipe book id.
    pub options: Vec<(String, CraftOption)>,
    /// Inventory menu slots 9..45: (id, shown name, count).
    pub inventory: Vec<(String, String, i64)>,
}

/// `CraftData` as a `RecipeSource` (always in a world: it exists only there).
pub struct Snapshot<'a>(pub &'a CraftData);

impl RecipeSource for Snapshot<'_> {
    fn in_world(&self) -> bool {
        true
    }
    fn book(&self) -> Vec<BookRecipe> {
        self.0
            .book
            .iter()
            .map(|r| BookRecipe {
                result: r.result.clone(),
                count: r.count,
                slots: r.slots.clone(),
            })
            .collect()
    }
    fn inventory(&self) -> Vec<(String, String, i64)> {
        self.0.inventory.clone()
    }
}

/// A recipe the panel shows (`Host::craft_panel`): a grid of
/// `width` x `height` cells (each lists the item ids that fit), its result,
/// a line under it and the action button (None = none, Some(false) = greyed).
#[derive(Clone, Debug, PartialEq)]
pub struct PanelRecipe {
    pub width: usize,
    pub height: usize,
    pub slots: Vec<Vec<String>>,
    pub result: (String, i64),
    pub note: String,
    pub action: Option<bool>,
}

/// Java's static `CraftCalc` state.
#[derive(Default)]
pub struct CraftCalc {
    pub result: Option<tree::Result>,
    pub error: Option<String>,
    pub names: HashMap<String, String>,
    pub inventory: HashMap<String, i64>,
}

pub const ERR_JOIN: &str = "Join a world first.";
pub const ERR_NO_RECIPE: &str = "No known crafting recipe for";

impl CraftCalc {
    /// Java `name(id)`: the shown name, else the id without namespace.
    pub fn name(&self, id: &str) -> String {
        self.names
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.split_once(':').map_or(id, |(_, p)| p).to_owned())
    }

    /// Java `calculate(query)`. True when a result was computed.
    pub fn calculate(&mut self, src: &dyn RecipeSource, learned: &Learned, query: &str) -> bool {
        self.result = None;
        self.error = None;
        if !src.in_world() {
            self.error = Some(ERR_JOIN.into());
            return false;
        }
        let c = parse_craft(query);
        if c.item.is_empty() {
            return false;
        }
        self.names.clear();
        let mut book: Vec<(String, Vec<Recipe>)> = Vec::new();
        for r in src.book() {
            if let Some(recipe) = self.recipe(&r) {
                push_recipe(&mut book, recipe);
            }
        }
        for p in learned.all() {
            let mut ings: Vec<Ingredient> = Vec::new();
            for cell in p.cells.iter().filter(|c| !c.is_empty()) {
                match ings.iter_mut().find(|i| i.options[0] == *cell) {
                    Some(i) => i.count += 1,
                    None => ings.push(Ingredient {
                        options: vec![cell.clone()],
                        count: 1,
                    }),
                }
            }
            push_recipe(
                &mut book,
                Recipe {
                    result: p.result.clone(),
                    count: p.count,
                    ingredients: ings,
                },
            );
            self.names
                .entry(p.result.clone())
                .or_insert_with(|| p.name.clone());
        }
        self.inventory.clear();
        for (id, name, n) in src.inventory() {
            *self.inventory.entry(id.clone()).or_insert(0) += n;
            self.names.entry(id).or_insert(name);
        }
        let Some(id) = self.resolve(&c.item, &book) else {
            self.error = Some(ERR_NO_RECIPE.into());
            return false;
        };
        let lookup = |key: &str| -> &[Recipe] {
            book.iter()
                .find(|(k, _)| k == key)
                .map_or(&[], |(_, v)| v.as_slice())
        };
        self.result = Some(tree::calculate(
            &id,
            i64::from(c.amount.max(1)),
            &lookup,
            &self.inventory,
        ));
        true
    }

    /// Java `CraftCalc.recipe`: slots with the same alternatives merge into
    /// one ingredient with a count.
    fn recipe(&mut self, r: &BookRecipe) -> Option<Recipe> {
        if r.result.0.is_empty() || r.slots.is_empty() {
            return None;
        }
        self.names
            .entry(r.result.0.clone())
            .or_insert_with(|| r.result.1.clone());
        let mut ings: Vec<Ingredient> = Vec::new();
        for slot in &r.slots {
            let mut options: Vec<String> = Vec::new();
            for (id, name) in slot.iter().filter(|(id, _)| !id.is_empty()) {
                self.names.entry(id.clone()).or_insert_with(|| name.clone());
                if !options.contains(id) {
                    options.push(id.clone());
                }
            }
            if options.is_empty() {
                continue;
            }
            match ings.iter_mut().find(|i| i.options == options) {
                Some(i) => i.count += 1,
                None => ings.push(Ingredient { options, count: 1 }),
            }
        }
        if ings.is_empty() {
            return None;
        }
        Some(Recipe {
            result: r.result.0.clone(),
            count: r.count,
            ingredients: ings,
        })
    }

    /// Java `resolve`. Java walks a HashMap (undefined order); here ties go
    /// to the smaller id (a deliberate difference for ambiguous queries).
    fn resolve(&self, query: &str, book: &[(String, Vec<Recipe>)]) -> Option<String> {
        let mut keys: Vec<&String> = book.iter().map(|(k, _)| k).collect();
        keys.sort();
        let shown = |id: &str| self.names.get(id).map_or("", |s| s.as_str()).to_owned();
        if let Some(k) = keys.iter().find(|k| item_matches(query, k, &shown(k))) {
            return Some((*k).clone());
        }
        let q = query.trim().to_lowercase().replace(' ', "_");
        let mut best: Option<&String> = None;
        for k in keys {
            let name = shown(k).to_lowercase().replace(' ', "_");
            if (k.contains(&q) || name.contains(&q)) && best.is_none_or(|b| k.len() < b.len()) {
                best = Some(k);
            }
        }
        best.cloned()
    }
}

fn push_recipe(book: &mut Vec<(String, Vec<Recipe>)>, r: Recipe) {
    match book.iter_mut().find(|(k, _)| *k == r.result) {
        Some((_, v)) => v.push(r),
        None => book.push((r.result.clone(), vec![r])),
    }
}

/// The tree rows the inventory tab shows (Java `treeRow`, at most 80),
/// depth first: (depth, label, info). Info joins `have`/`craft`/`missing`.
pub fn tree_rows(calc: &CraftCalc) -> Vec<(usize, String, String)> {
    fn walk(
        calc: &CraftCalc,
        n: &tree::Node,
        depth: usize,
        out: &mut Vec<(usize, String, String)>,
    ) {
        if out.len() >= 80 {
            return;
        }
        let mut info = Vec::new();
        if n.from_stock > 0 {
            info.push(format!("have {}", n.from_stock));
        }
        if n.crafts > 0 {
            info.push(format!("craft {}x", n.crafts));
        }
        if n.missing > 0 {
            info.push(format!("missing {}", n.missing));
        }
        out.push((
            depth,
            format!("{}x {}", n.amount, calc.name(&n.item)),
            info.join(", "),
        ));
        for c in &n.children {
            walk(calc, c, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    if let Some(r) = &calc.result {
        walk(calc, &r.root, 0, &mut out);
    }
    out
}

/// The workflow step texts "Craft it" runs, ingredients first.
pub fn craft_steps(result: &tree::Result) -> Vec<String> {
    result
        .steps
        .iter()
        .map(|s| format!("craft:{} {}", s.item, s.amount))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::omnitool_learned_recipes::trim;
    use super::*;

    struct Fake {
        world: bool,
        book: Vec<BookRecipe>,
        inv: Vec<(String, String, i64)>,
    }
    impl RecipeSource for Fake {
        fn in_world(&self) -> bool {
            self.world
        }
        fn book(&self) -> Vec<BookRecipe> {
            self.book
                .iter()
                .map(|r| BookRecipe {
                    result: r.result.clone(),
                    count: r.count,
                    slots: r.slots.clone(),
                })
                .collect()
        }
        fn inventory(&self) -> Vec<(String, String, i64)> {
            self.inv.clone()
        }
    }

    fn s(id: &str, name: &str) -> (String, String) {
        (id.to_owned(), name.to_owned())
    }

    fn fake() -> Fake {
        let planks = || vec![s("minecraft:oak_planks", "Oak Planks")];
        Fake {
            world: true,
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
            inv: vec![s3("minecraft:oak_planks", "Oak Planks", 3)],
        }
    }
    fn s3(id: &str, name: &str, n: i64) -> (String, String, i64) {
        (id.to_owned(), name.to_owned(), n)
    }

    #[test]
    fn calculates_with_book_and_inventory() {
        let mut c = CraftCalc::default();
        assert!(c.calculate(&fake(), &Learned::default(), "stick 8"));
        let r = c.result.as_ref().unwrap();
        // 8 sticks = 2 crafts = 4 planks, 3 in the inventory, 1 more from one log craft
        assert_eq!(tree::get(&r.have, "minecraft:oak_planks"), 3);
        assert_eq!(tree::get(&r.missing, "minecraft:oak_log"), 1);
        assert_eq!(tree::get(&r.raw, "minecraft:oak_log"), 1);
        assert!(!r.complete());
        assert_eq!(
            craft_steps(r),
            ["craft:minecraft:oak_planks 1", "craft:minecraft:stick 8"]
        );
        assert_eq!(c.name("minecraft:oak_log"), "Oak Log");
        assert_eq!(c.name("minecraft:unknown"), "unknown");
        let rows = tree_rows(&c);
        assert_eq!(rows[0], (0, "8x Stick".into(), "craft 2x".into()));
    }

    #[test]
    fn errors_and_fallbacks() {
        let mut c = CraftCalc::default();
        let mut f = fake();
        f.world = false;
        assert!(!c.calculate(&f, &Learned::default(), "stick"));
        assert_eq!(c.error.as_deref(), Some(ERR_JOIN));
        assert!(!c.calculate(&fake(), &Learned::default(), "  "));
        assert!(c.error.is_none() && c.result.is_none());
        assert!(!c.calculate(&fake(), &Learned::default(), "diamond"));
        assert_eq!(c.error.as_deref(), Some(ERR_NO_RECIPE));
        // substring fallback picks the shortest id
        assert!(c.calculate(&fake(), &Learned::default(), "stic"));
        assert_eq!(c.result.as_ref().unwrap().root.item, "minecraft:stick");
    }

    #[test]
    fn learned_patterns_become_recipes() {
        let grid: Vec<String> = ["a", "a", "", "b"].iter().map(|x| x.to_string()).collect();
        let p = trim("minecraft:gizmo", "Gizmo", 2, 2, 2, &grid).unwrap();
        let mut l = Learned::default();
        assert!(l.learn(p));
        let mut c = CraftCalc::default();
        let mut f = fake();
        f.book.clear();
        assert!(c.calculate(&f, &l, "gizmo"));
        let r = c.result.as_ref().unwrap();
        assert_eq!(tree::get(&r.raw, "a"), 2);
        assert_eq!(tree::get(&r.raw, "b"), 1);
        assert_eq!(c.name("minecraft:gizmo"), "Gizmo");
    }
}
