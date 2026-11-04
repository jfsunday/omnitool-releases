// CraftTree (docs/spec/omnitool/calculator.md, section 6) as pure logic on item
// id strings, checked against tests/fixtures/craft-tree.json. The recipes come
// in as a function, so the game's recipe book can be plugged in later.
// Added to the game as `game::omnitool_craft_tree`; also a module of this crate.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

const MAX_DEPTH: usize = 12;
const MAX_DRY_RUNS: usize = 8;
const BUDGET: i32 = 20000;

/// One ingredient slot: any of `options`, `count` per craft.
#[derive(Clone, Debug, PartialEq)]
pub struct Ingredient {
    pub options: Vec<String>,
    pub count: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub result: String,
    pub count: i64,
    pub ingredients: Vec<Ingredient>,
}

impl Recipe {
    fn inputs_per_craft(&self) -> i64 {
        self.ingredients.iter().map(|i| i.count).sum()
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub item: String,
    pub amount: i64,
    pub from_stock: i64,
    pub crafts: i64,
    pub missing: i64,
    pub recipe: Option<Recipe>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub item: String,
    pub amount: i64,
    pub crafts: i64,
}

/// Item -> count in insertion order (Java's `LinkedHashMap`).
pub type Counts = Vec<(String, i64)>;

pub fn add(map: &mut Counts, key: &str, n: i64) {
    match map.iter_mut().find(|(k, _)| k == key) {
        Some((_, v)) => *v += n,
        None => map.push((key.to_owned(), n)),
    }
}

#[allow(dead_code)] // read by the tests; the panel lists `raw`/`missing` in order
pub fn get(map: &Counts, key: &str) -> i64 {
    map.iter().find(|(k, _)| k == key).map_or(0, |(_, v)| *v)
}

#[derive(Debug)]
pub struct Result {
    pub root: Node,
    pub raw: Counts,
    #[allow(dead_code)] // the Java result carries it; the panel reads the node's `from_stock`
    pub have: Counts,
    pub missing: Counts,
    pub steps: Vec<Step>,
}

impl Result {
    pub fn complete(&self) -> bool {
        self.missing.is_empty()
    }
}

type Recipes<'a> = &'a dyn Fn(&str) -> &'a [Recipe];

struct Tree<'a> {
    recipes: Recipes<'a>,
    stock: HashMap<String, i64>,
    budget: Rc<Cell<i32>>,
    path: Vec<String>,
    have: Counts,
    missing: Counts,
    steps: Vec<Step>,
}

/// Java's `CraftTree.calculate`. `recipes` returns the recipes for a result
/// (an empty slice for none).
pub fn calculate(
    item: &str,
    amount: i64,
    recipes: Recipes<'_>,
    inventory: &HashMap<String, i64>,
) -> Result {
    let budget = Rc::new(Cell::new(BUDGET));
    let amount = amount.max(1);
    let mut scratch = Tree::new(recipes, HashMap::new(), budget.clone());
    scratch.expand(item, amount, 0, true);
    // the requested item itself is crafted, not taken from the inventory
    let mut stock = inventory.clone();
    stock.remove(item);
    let mut real = Tree::new(recipes, stock, budget.clone());
    let root = real.expand(item, amount, 0, true);
    Result {
        root,
        raw: scratch.missing,
        have: real.have,
        missing: real.missing,
        steps: real.steps,
    }
}

impl<'a> Tree<'a> {
    fn new(recipes: Recipes<'a>, stock: HashMap<String, i64>, budget: Rc<Cell<i32>>) -> Self {
        Tree {
            recipes,
            stock,
            budget,
            path: Vec::new(),
            have: Vec::new(),
            missing: Vec::new(),
            steps: Vec::new(),
        }
    }

    fn dry(&self) -> Tree<'a> {
        let mut t = Tree::new(self.recipes, self.stock.clone(), self.budget.clone());
        t.path = self.path.clone();
        t
    }

    fn stock_of(&self, item: &str) -> i64 {
        self.stock.get(item).copied().unwrap_or(0)
    }

    fn missing_total(&self) -> i64 {
        self.missing.iter().map(|(_, v)| *v).sum()
    }

    fn expand(&mut self, item: &str, need: i64, depth: usize, record: bool) -> Node {
        self.budget.set(self.budget.get() - 1);
        let mut n = Node {
            item: item.to_owned(),
            amount: need,
            from_stock: 0,
            crafts: 0,
            missing: 0,
            recipe: None,
            children: Vec::new(),
        };
        let take = self.stock_of(item).min(need);
        if take > 0 {
            *self.stock.entry(item.to_owned()).or_insert(0) -= take;
            if record {
                add(&mut self.have, item, take);
            }
            n.from_stock = take;
        }
        let rest = need - take;
        if rest <= 0 {
            return n;
        }
        let recipe = if depth < MAX_DEPTH {
            self.choose(item, rest, depth)
        } else {
            None
        };
        let Some(r) = recipe else {
            n.missing = rest;
            add(&mut self.missing, item, rest);
            return n;
        };
        n.crafts = (rest + r.count - 1) / r.count;
        self.path.push(item.to_owned());
        for ing in &r.ingredients {
            let count = ing.count * n.crafts;
            let opt = self.option(ing, count, depth);
            n.children.push(self.expand(&opt, count, depth + 1, record));
        }
        self.path.pop();
        let surplus = n.crafts * r.count - rest;
        if surplus > 0 {
            *self.stock.entry(item.to_owned()).or_insert(0) += surplus;
        }
        if record {
            match self.steps.last_mut() {
                Some(last) if last.item == item => {
                    last.amount += rest;
                    last.crafts += n.crafts;
                }
                _ => self.steps.push(Step {
                    item: item.to_owned(),
                    amount: rest,
                    crafts: n.crafts,
                }),
            }
        }
        n.recipe = Some(r);
        n
    }

    /// Recipes without cycles into the current path. Reversible pairs (ingot
    /// and block) are only used for the requested item itself or when the
    /// stock has the ingredient; else the item counts as raw.
    fn candidates(&self, item: &str, depth: usize) -> Vec<Recipe> {
        let mut out = Vec::new();
        'recipes: for r in (self.recipes)(item) {
            let mut ings = Vec::new();
            for ing in &r.ingredients {
                let opts: Vec<String> = ing
                    .options
                    .iter()
                    .filter(|o| {
                        !(o.as_str() == item
                            || self.path.contains(o)
                            || (depth > 0 && self.stock_of(o) <= 0 && self.reverses(o, item)))
                    })
                    .cloned()
                    .collect();
                if opts.is_empty() {
                    continue 'recipes;
                }
                ings.push(Ingredient {
                    options: opts,
                    count: ing.count,
                });
            }
            if !ings.is_empty() && r.count > 0 {
                out.push(Recipe {
                    result: r.result.clone(),
                    count: r.count,
                    ingredients: ings,
                });
            }
        }
        out
    }

    /// True if `ingredient` is itself crafted from `item`.
    fn reverses(&self, ingredient: &str, item: &str) -> bool {
        (self.recipes)(ingredient).iter().any(|r| {
            r.ingredients
                .iter()
                .any(|i| i.options.iter().any(|o| o == item))
        })
    }

    fn choose(&mut self, item: &str, need: i64, depth: usize) -> Option<Recipe> {
        let mut c = self.candidates(item, depth);
        if c.len() <= 1 {
            return c.pop();
        }
        let mut best = None;
        let mut best_score = i64::MAX;
        for r in c.iter().take(MAX_DRY_RUNS) {
            let crafts = (need + r.count - 1) / r.count;
            let mut miss = 0;
            if self.budget.get() > 0 {
                let mut dry = self.dry();
                dry.path.push(item.to_owned());
                for ing in &r.ingredients {
                    let count = ing.count * crafts;
                    let opt = dry.option(ing, count, depth);
                    dry.expand(&opt, count, depth + 1, false);
                }
                miss = dry.missing_total();
            }
            // fewest missing items first, then fewest inputs per crafted item
            let score = miss * 1_000_000 + r.inputs_per_craft() * 1000 / r.count;
            if score < best_score {
                best_score = score;
                best = Some(r.clone());
            }
        }
        best
    }

    /// The option of an ingredient: enough in stock, else the one with the fewest missing items.
    fn option(&self, ing: &Ingredient, count: i64, depth: usize) -> String {
        let opts = &ing.options;
        if opts.len() == 1 {
            return opts[0].clone();
        }
        let mut most: Option<&String> = None;
        let mut most_count = 0;
        for o in opts {
            let s = self.stock_of(o);
            if s > most_count {
                most = Some(o);
                most_count = s;
            }
        }
        if let Some(m) = most.filter(|_| most_count >= count) {
            return m.clone();
        }
        if self.budget.get() <= 0 {
            return most.unwrap_or(&opts[0]).clone();
        }
        let mut best = &opts[0];
        let mut best_miss = i64::MAX;
        for o in opts.iter().take(MAX_DRY_RUNS) {
            let mut dry = self.dry();
            dry.expand(o, count, depth + 1, false);
            let miss = dry.missing_total();
            if miss < best_miss {
                best_miss = miss;
                best = o;
            }
        }
        best.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    pub fn parse_recipes(v: &Value) -> HashMap<String, Vec<Recipe>> {
        let mut book: HashMap<String, Vec<Recipe>> = HashMap::new();
        for r in v.as_array().unwrap() {
            let recipe = Recipe {
                result: r["result"].as_str().unwrap().to_owned(),
                count: r["count"].as_i64().unwrap(),
                ingredients: r["ingredients"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| Ingredient {
                        options: i["options"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|o| o.as_str().unwrap().to_owned())
                            .collect(),
                        count: i["count"].as_i64().unwrap(),
                    })
                    .collect(),
            };
            book.entry(recipe.result.clone()).or_default().push(recipe);
        }
        book
    }

    fn as_map(v: &Value) -> HashMap<String, i64> {
        v.as_object()
            .unwrap()
            .iter()
            .map(|(k, n)| (k.clone(), n.as_i64().unwrap()))
            .collect()
    }

    fn counts(c: &Counts) -> HashMap<String, i64> {
        c.iter().cloned().collect()
    }

    #[test]
    fn matches_java_fixtures() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/craft-tree.json"
        );
        let fx: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for case in fx["craft_tree"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let book = parse_recipes(&case["recipes"]);
            let lookup = |id: &str| -> &[Recipe] { book.get(id).map_or(&[], |v| v.as_slice()) };
            let res = calculate(
                case["item"].as_str().unwrap(),
                case["amount"].as_i64().unwrap(),
                &lookup,
                &as_map(&case["inventory"]),
            );
            let e = &case["expect"];
            if e.get("raw").is_some() {
                assert_eq!(counts(&res.raw), as_map(&e["raw"]), "{name} raw");
            }
            if e.get("have").is_some() {
                assert_eq!(counts(&res.have), as_map(&e["have"]), "{name} have");
            }
            if e.get("missing").is_some() {
                assert_eq!(
                    counts(&res.missing),
                    as_map(&e["missing"]),
                    "{name} missing"
                );
            }
            if let Some(c) = e.get("complete") {
                assert_eq!(res.complete(), c.as_bool().unwrap(), "{name} complete");
            }
            if let Some(steps) = e.get("steps") {
                let want: Vec<Step> = steps
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| Step {
                        item: s["item"].as_str().unwrap().to_owned(),
                        amount: s["amount"].as_i64().unwrap(),
                        crafts: s["crafts"].as_i64().unwrap(),
                    })
                    .collect();
                assert_eq!(res.steps, want, "{name} steps");
            }
            if let Some(c) = e.get("root_crafts") {
                assert_eq!(res.root.crafts, c.as_i64().unwrap(), "{name} root_crafts");
            }
        }
    }

    #[test]
    fn missing_item_amount_counts_as_one() {
        let lookup = |_: &str| -> &[Recipe] { &[] };
        let res = calculate("thing", 0, &lookup, &HashMap::new());
        assert_eq!(res.missing, vec![("thing".to_owned(), 1)]);
    }
}
