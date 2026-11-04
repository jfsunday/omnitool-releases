// CraftOption (docs/spec/omnitool/calculator.md, section 7): an unlocked
// recipe for the Craft workflow step, as pure logic checked against
// tests/fixtures/craft-option.json. Added to the game as
// `game::omnitool_craft_option`; also a module of this crate.

use std::collections::HashMap;

/// A recipe-book handle, the item ids each ingredient slot accepts, and
/// whether it needs a 3x3 grid.
#[derive(Clone, Debug)]
pub struct CraftOption {
    pub handle: String,
    pub slots: Vec<Vec<String>>,
    pub needs_table: bool,
}

impl CraftOption {
    /// The most crafts up to `limit` the stock allows (Java `maxCrafts`).
    pub fn max_crafts(&self, have: &HashMap<String, i64>, limit: i64) -> i64 {
        if self.slots.is_empty() {
            return 0;
        }
        let mut k = 0;
        while k < limit && self.possible(have, k + 1) {
            k += 1;
        }
        k
    }

    /// Greedy check: smallest slots first, each takes the option with the
    /// biggest stock that still has `k`. Ties go to the first option (Java:
    /// set order, which is undefined there).
    fn possible(&self, have: &HashMap<String, i64>, k: i64) -> bool {
        let mut left = have.clone();
        let mut slots: Vec<&Vec<String>> = self.slots.iter().collect();
        slots.sort_by_key(|s| s.len());
        for slot in slots {
            let mut best: Option<&String> = None;
            let mut best_n = 0;
            for o in slot {
                let n = left.get(o).copied().unwrap_or(0);
                if n >= k && n > best_n {
                    best = Some(o);
                    best_n = n;
                }
            }
            match best {
                Some(o) => *left.get_mut(o).unwrap() -= k,
                None => return false,
            }
        }
        true
    }
}

/// Java `usable`: craftable options, the most crafts first (stable).
pub fn usable<'a>(
    all: &'a [CraftOption],
    have: &HashMap<String, i64>,
    table: bool,
) -> Vec<&'a CraftOption> {
    let mut out: Vec<(i64, &CraftOption)> = Vec::new();
    for o in all {
        if o.needs_table && !table {
            continue;
        }
        let n = o.max_crafts(have, 64);
        if n > 0 {
            out.push((n, o));
        }
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.0));
    out.into_iter().map(|(_, o)| o).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn matches_java_fixtures() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/craft-option.json"
        );
        let fx: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for case in fx["craft_option"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let opts: Vec<CraftOption> = case["options"]
                .as_array()
                .unwrap()
                .iter()
                .map(|o| CraftOption {
                    handle: o["handle"].as_str().unwrap().to_owned(),
                    slots: o["slots"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|s| {
                            s.as_array()
                                .unwrap()
                                .iter()
                                .map(|x| x.as_str().unwrap().to_owned())
                                .collect()
                        })
                        .collect(),
                    needs_table: o["needs_table"].as_bool().unwrap(),
                })
                .collect();
            let have: HashMap<String, i64> = case["have"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.as_i64().unwrap()))
                .collect();
            let table = case["table"].as_bool().unwrap();
            let e = &case["expect"];
            let got: Vec<&str> = usable(&opts, &have, table)
                .iter()
                .map(|o| o.handle.as_str())
                .collect();
            let want: Vec<&str> = e["usable"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            assert_eq!(got, want, "{name} usable");
            if let Some(mc) = e.get("max_crafts") {
                for (handle, w) in mc.as_object().unwrap() {
                    let o = opts.iter().find(|o| &o.handle == handle).unwrap();
                    let limit = w["limit"].as_i64().unwrap();
                    assert_eq!(
                        o.max_crafts(&have, limit),
                        w["value"].as_i64().unwrap(),
                        "{name} max_crafts {handle}"
                    );
                }
            }
        }
    }

    #[test]
    fn no_slots_means_no_crafts() {
        let o = CraftOption {
            handle: "x".into(),
            slots: vec![],
            needs_table: false,
        };
        assert_eq!(o.max_crafts(&HashMap::new(), 64), 0);
    }
}
