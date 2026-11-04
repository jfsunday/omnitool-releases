// LearnedRecipes (docs/spec/omnitool/calculator.md, section 8): crafting
// patterns the player put into a grid, their trimming and the recipes.json
// format, as pure logic. Reading the open crafting menu needs a hook Horizon
// does not have yet (`learn` takes the grid as plain ids). Added to the game
// as `game::omnitool_learned_recipes`; also a module of this crate.

use super::omnitool_json::{Json, Obj};

#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    pub result: String,
    pub name: String,
    pub count: i64,
    pub width: usize,
    pub height: usize,
    /// Row-major item ids, "" = empty.
    pub cells: Vec<String>,
}

impl Pattern {
    pub fn cell(&self, x: usize, y: usize) -> &str {
        &self.cells[y * self.width + x]
    }
}

/// Java `trim`: the smallest box around the non-empty cells of a `gw`x`gh`
/// grid; None if the grid is empty.
#[allow(dead_code)] // learning needs the open crafting grid, which no Horizon hook gives yet
pub fn trim(
    result: &str,
    name: &str,
    count: i64,
    gw: usize,
    gh: usize,
    grid: &[String],
) -> Option<Pattern> {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (usize::MAX, usize::MAX, 0, 0);
    let mut any = false;
    for y in 0..gh {
        for x in 0..gw {
            if grid.get(y * gw + x).is_some_and(|c| !c.is_empty()) {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if !any {
        return None;
    }
    let mut cells = Vec::new();
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            cells.push(grid[y * gw + x].clone());
        }
    }
    Some(Pattern {
        result: result.to_owned(),
        name: name.to_owned(),
        count,
        width: max_x - min_x + 1,
        height: max_y - min_y + 1,
        cells,
    })
}

/// The learned patterns plus entries this version could not read.
#[derive(Default)]
pub struct Learned {
    by_result: Vec<Pattern>,
    unparsed: Vec<(String, Json)>,
}

impl Learned {
    /// Java `load`: an entry with missing fields or wrong types, or
    /// `width*height != cells`, is kept as is for the next write.
    pub fn from_json(root: Option<&Obj>) -> Learned {
        let mut l = Learned::default();
        let Some(root) = root else { return l };
        for (key, v) in root.iter() {
            match parse_pattern(key, v) {
                Some(p) => l.put(p),
                None => l.unparsed.push((key.to_owned(), v.clone())),
            }
        }
        l
    }

    /// Java `save`: patterns in insertion order, then the unparsed entries
    /// whose key is not a pattern now.
    pub fn to_json(&self) -> Obj {
        let mut o = Obj::new();
        for p in &self.by_result {
            let mut e = Obj::new();
            e.insert("name", Json::str(&p.name));
            e.insert("count", Json::Int(p.count));
            e.insert("width", Json::Int(p.width as i64));
            e.insert("height", Json::Int(p.height as i64));
            e.insert(
                "cells",
                Json::Arr(p.cells.iter().map(|c| Json::str(c)).collect()),
            );
            o.insert(&p.result, Json::Obj(e));
        }
        for (k, v) in &self.unparsed {
            if !o.has(k) {
                o.insert(k, v.clone());
            }
        }
        o
    }

    /// Replaces a pattern with the same result in place, else appends.
    fn put(&mut self, p: Pattern) {
        match self.by_result.iter_mut().find(|q| q.result == p.result) {
            Some(q) => *q = p,
            None => self.by_result.push(p),
        }
    }

    /// Java `tick` after the checks: stores `p` unless it equals the known
    /// pattern. True when it is new (save the file, show the action bar).
    #[allow(dead_code)] // learning needs the open crafting grid, which no Horizon hook gives yet
    pub fn learn(&mut self, p: Pattern) -> bool {
        if self.by_result.contains(&p) {
            return false;
        }
        self.put(p);
        true
    }

    /// Java `all()`.
    pub fn all(&self) -> &[Pattern] {
        &self.by_result
    }

    /// Java `find(query)`: the first pattern that `matches(result, name)`.
    pub fn find(&self, matches: impl Fn(&str, &str) -> bool) -> Option<&Pattern> {
        self.by_result.iter().find(|p| matches(&p.result, &p.name))
    }

    /// Java `forget`: true if it existed (save the file).
    pub fn forget(&mut self, result: &str) -> bool {
        let n = self.by_result.len();
        self.by_result.retain(|p| p.result != result);
        self.by_result.len() != n
    }
}

/// The action bar text after learning.
#[allow(dead_code)] // learning needs the open crafting grid, which no Horizon hook gives yet
pub fn learned_message(p: &Pattern) -> String {
    format!("Omnitool learned recipe: {}", p.name)
}

fn parse_pattern(key: &str, v: &Json) -> Option<Pattern> {
    let o = v.as_obj()?;
    let int = |k: &str| match o.get(k)? {
        Json::Int(i) => Some(*i),
        _ => None,
    };
    let name = match o.get("name")? {
        Json::Str(s) => s.clone(),
        _ => return None,
    };
    let (count, width, height) = (int("count")?, int("width")?, int("height")?);
    let Json::Arr(cells) = o.get("cells")? else {
        return None;
    };
    let cells: Vec<String> = cells
        .iter()
        .map(|c| match c {
            Json::Str(s) => Some(s.clone()),
            _ => None,
        })
        .collect::<Option<_>>()?;
    if width < 0 || height < 0 || (width * height) as usize != cells.len() {
        return None;
    }
    Some(Pattern {
        result: key.to_owned(),
        name,
        count,
        width: width as usize,
        height: height as usize,
        cells,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(cells: &[&str]) -> Vec<String> {
        cells.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn trim_cuts_empty_edges() {
        let p = trim("stick", "Stick", 4, 2, 2, &g(&["", "", "", "stick"])).unwrap();
        assert_eq!((p.width, p.height), (1, 1));
        assert_eq!(p.cells, g(&["stick"]));
        let p = trim(
            "x",
            "X",
            1,
            3,
            3,
            &g(&["", "", "", "planks", "planks", "", "", "", ""]),
        )
        .unwrap();
        assert_eq!((p.width, p.height), (2, 1));
        assert_eq!(p.cells, g(&["planks", "planks"]));
        assert_eq!(p.cell(1, 0), "planks");
        assert!(trim("x", "X", 1, 2, 2, &g(&["", "", "", ""])).is_none());
    }

    #[test]
    fn json_round_trip_keeps_unparsed_entries() {
        let text = r#"{
          "minecraft:stick": {"name": "Stick", "count": 4, "width": 1, "height": 2,
                              "cells": ["minecraft:oak_planks", "minecraft:oak_planks"]},
          "future:thing": {"shape": "round"},
          "bad:size": {"name": "B", "count": 1, "width": 2, "height": 2, "cells": ["a"]}
        }"#;
        let root = Json::parse(text).unwrap();
        let mut l = Learned::from_json(root.as_obj());
        assert_eq!(l.all().len(), 1);
        let out = l.to_json();
        assert_eq!(out.len(), 3);
        assert_eq!(
            out.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            ["minecraft:stick", "future:thing", "bad:size"]
        );
        // a parsed pattern shadows an unparsed entry with the same key
        let mut p = l.all()[0].clone();
        p.result = "future:thing".into();
        assert!(l.learn(p));
        let out = l.to_json();
        assert_eq!(out.len(), 3);
        assert!(out.obj("future:thing").unwrap().has("cells"));
    }

    #[test]
    fn learn_replaces_in_place_and_ignores_equal() {
        let mut l = Learned::default();
        let a = trim("a", "A", 1, 1, 1, &g(&["x"])).unwrap();
        let b = trim("b", "B", 1, 1, 1, &g(&["y"])).unwrap();
        assert!(l.learn(a.clone()));
        assert!(l.learn(b));
        assert!(!l.learn(a.clone()));
        let a2 = Pattern { count: 2, ..a };
        assert!(l.learn(a2));
        assert_eq!(l.all()[0].count, 2);
        assert_eq!(l.all()[0].result, "a");
        assert!(l.forget("a"));
        assert!(!l.forget("a"));
        assert_eq!(l.find(|r, _| r == "b").unwrap().result, "b");
    }
}
