// Pure inventory helpers (Java's core/InventoryPlan and core/ClickQueue):
// tool families, hotbar layout swaps and the per-tick click budget. Used by
// Auto Refill, hotbar presets and MouseTweaks-style features once those are
// ported. Checked against tests/fixtures/workflows.json (tool_family,
// hotbar_swaps, click_queue). Added to the game as
// `game::omnitool_inventory_plan`; also a module of this crate.

use std::collections::VecDeque;

const TOOL_SUFFIXES: [&str; 6] = ["pickaxe", "axe", "shovel", "hoe", "sword", "spear"];

/// `pickaxe` for any pickaxe, else the id path itself (shears, bow, ...).
#[allow(dead_code)] // Auto Refill uses it
pub fn tool_family(id: &str) -> &str {
    let path = id.split_once(':').map_or(id, |(_, p)| p);
    let suffix = path.rsplit_once('_').map_or(path, |(_, s)| s);
    if TOOL_SUFFIXES.contains(&suffix) {
        suffix
    } else {
        path
    }
}

/// Swaps that turn the inventory into the wanted hotbar layout. `slots`:
/// item ids of menu slots 9..44 (0-26 main, 27-35 hotbar, "" = empty);
/// `wanted`: up to 9 ids ("" = leave alone). Pairs (hotbar menu slot,
/// source menu slot) in order.
#[allow(dead_code)] // hotbar presets use it
pub fn hotbar_swaps(slots: &[String], wanted: &[String]) -> Vec<(usize, usize)> {
    let mut inv = slots.to_vec();
    let mut out = Vec::new();
    for (h, w) in wanted.iter().enumerate().take(9) {
        let hi = 27 + h;
        if w.is_empty() || *w == inv[hi] {
            continue;
        }
        let from = (0..27).find(|&j| inv[j] == *w).or_else(|| {
            // another hotbar slot that doesn't need this item itself
            (27..36).find(|&k| {
                k != hi && inv[k] == *w && wanted.get(k - 27).map_or("", |s| s.as_str()) != w
            })
        });
        let Some(from) = from else { continue };
        inv.swap(hi, from);
        out.push((9 + hi, 9 + from));
    }
    out
}

/// Throttles inventory clicks: jobs run in order while the per-tick budget
/// lasts; a job returns the clicks it sent and may overdraw the budget.
pub struct ClickQueue<J> {
    jobs: VecDeque<J>,
    budget: i32,
}

impl<J> Default for ClickQueue<J> {
    fn default() -> Self {
        ClickQueue {
            jobs: VecDeque::new(),
            budget: 0,
        }
    }
}

#[allow(dead_code)] // inventory features use it
impl<J> ClickQueue<J> {
    /// Once per tick: debt carries over, a positive rest does not.
    pub fn refill(&mut self, per_tick: i32) {
        self.budget = self.budget.min(0) + per_tick.max(1);
    }

    pub fn add(&mut self, job: J) {
        self.jobs.push_back(job);
    }

    /// Runs queued jobs while there is budget; `run` sends a job's clicks.
    pub fn run(&mut self, mut run: impl FnMut(J) -> i32) {
        while self.budget > 0 {
            let Some(job) = self.jobs.pop_front() else {
                break;
            };
            self.budget -= run(job).max(0);
        }
    }

    pub fn clear(&mut self) {
        self.jobs.clear();
    }

    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    pub fn budget(&self) -> i32 {
        self.budget
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_workflows::tests::fixtures;

    fn strings(v: &serde_json::Value) -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn families_and_swaps() {
        let f = fixtures();
        for c in f["tool_family"].as_array().unwrap() {
            assert_eq!(
                tool_family(c["id"].as_str().unwrap()),
                c["family"].as_str().unwrap()
            );
        }
        for c in f["hotbar_swaps"].as_array().unwrap() {
            let got = hotbar_swaps(&strings(&c["slots"]), &strings(&c["wanted"]));
            let want: Vec<(usize, usize)> = c["swaps"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    (
                        p[0].as_u64().unwrap() as usize,
                        p[1].as_u64().unwrap() as usize,
                    )
                })
                .collect();
            assert_eq!(got, want, "{}", c["name"]);
        }
    }

    #[test]
    fn click_queue() {
        for c in fixtures()["click_queue"].as_array().unwrap() {
            let mut q: ClickQueue<(String, i32)> = ClickQueue::default();
            let mut results = Vec::new();
            for op in c["ops"].as_array().unwrap() {
                match op["op"].as_str().unwrap() {
                    "refill" => q.refill(op["per_tick"].as_i64().unwrap() as i32),
                    "add" => q.add((
                        op["id"].as_str().unwrap().to_owned(),
                        op["clicks"].as_i64().unwrap() as i32,
                    )),
                    "clear" => q.clear(),
                    "run" => {
                        let mut ran = Vec::new();
                        q.run(|(id, clicks)| {
                            ran.push(id);
                            clicks
                        });
                        results.push(serde_json::json!({
                            "ran": ran, "left": q.len(), "budget": q.budget()
                        }));
                    }
                    o => panic!("unknown op {o}"),
                }
            }
            assert_eq!(
                serde_json::Value::Array(results),
                c["results"],
                "{}",
                c["name"]
            );
        }
    }
}
