// Workflow triggers (Java's core/Triggers, beta): polled once per tick from a
// `World` snapshot, chat lines queued by the host. Each trigger fires once
// when its event happens or its condition becomes true. Checked against
// tests/fixtures/workflows.json (trigger_sim). Added to the game as
// `game::omnitool_triggers`; also a module of this crate.

use super::omnitool_conditions::{World, dimension_matches, local, server_ip};
use super::omnitool_workflows::{Binding, Trigger, seconds, ticks};
use std::collections::HashMap;

/// Join triggers wait a second so the server is ready for chat/commands.
const JOIN_DELAY: u32 = 20;
/// Chat triggers fire at most once per second per workflow.
const CHAT_COOLDOWN: i32 = 20;
/// Chat lines kept until the next tick.
const CHAT_QUEUE: usize = 100;

/// Java's `Triggers` state.
#[derive(Default)]
pub struct Triggers {
    chat: Vec<String>,
    /// Last edge value per workflow uid.
    state: HashMap<u64, bool>,
    /// Interval / chat counters per workflow uid.
    counter: HashMap<u64, i32>,
    in_world: bool,
    was_dead: bool,
    world_ticks: u32,
    dimension: Option<String>,
}

impl Triggers {
    /// A chat or system message was shown (not the action bar).
    pub fn chat(&mut self, text: &str) {
        if !text.is_empty() && self.chat.len() < CHAT_QUEUE {
            self.chat.push(text.to_owned());
        }
    }

    fn reset(&mut self) {
        self.in_world = false;
        self.was_dead = false;
        self.world_ticks = 0;
        self.dimension = None;
        self.state.clear();
        self.counter.clear();
    }

    /// True only on the tick the condition becomes true.
    fn edge(&mut self, uid: u64, now: bool) -> bool {
        let was = self.state.insert(uid, now);
        now && was != Some(true)
    }

    /// Java's `Triggers.tick`: the uids of the workflows to run, in list order.
    pub fn tick(&mut self, beta: bool, world: Option<&World>, bindings: &[Binding]) -> Vec<u64> {
        let Some(w) = world.filter(|_| beta) else {
            if self.in_world {
                self.reset();
            }
            self.chat.clear();
            return Vec::new();
        };
        if !self.in_world {
            self.reset();
            self.in_world = true;
            self.dimension = Some(w.dimension.clone());
        }
        self.world_ticks += 1;
        let dim_changed = self.dimension.as_deref() != Some(w.dimension.as_str());
        self.dimension = Some(w.dimension.clone());
        let dead = w.dead;
        let respawned = self.was_dead && !dead;
        self.was_dead = dead;
        let joined = self.world_ticks == JOIN_DELAY;
        let messages: String = self
            .chat
            .drain(..)
            .map(|m| m.to_lowercase() + "\n")
            .collect();
        let server = server_ip(&w.server);
        let world_name = w.world_name.to_lowercase();
        let mut fired = Vec::new();
        for b in bindings {
            let arg = b.trigger_arg.trim().to_lowercase();
            let uid = b.uid;
            let fire = match b.trigger {
                Trigger::None => false,
                Trigger::ServerJoin => joined && !local(&w.server) && server.contains(&arg),
                Trigger::WorldJoin => {
                    joined && (arg.is_empty() || server.contains(&arg) || world_name.contains(&arg))
                }
                Trigger::Respawn => respawned,
                Trigger::DimensionChange => {
                    dim_changed
                        && (arg.is_empty()
                            || dimension_matches(&arg, &w.dimension)
                            || w.dimension.contains(&arg))
                }
                Trigger::Death => self.edge(uid, dead),
                Trigger::HealthBelow => self.edge(uid, w.health < seconds(&arg, 0.0)),
                Trigger::HungerBelow => self.edge(uid, f64::from(w.hunger) < seconds(&arg, 0.0)),
                Trigger::InventoryFull => self.edge(uid, w.inventory_full),
                Trigger::Interval => {
                    let every = ticks(seconds(&arg, 0.0));
                    let c = self.counter.entry(uid).or_insert(0);
                    *c += 1;
                    if every <= 0 || *c < every {
                        false
                    } else {
                        *c = 0;
                        true
                    }
                }
                Trigger::Chat => {
                    let c = self.counter.entry(uid).or_insert(CHAT_COOLDOWN);
                    *c += 1;
                    if arg.is_empty() || !messages.contains(&arg) || *c < CHAT_COOLDOWN {
                        false
                    } else {
                        *c = 0;
                        true
                    }
                }
            };
            if fire {
                fired.push(uid);
            }
        }
        fired
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_safe::Server;
    use crate::omnitool_workflows::tests::fixtures;

    #[test]
    fn trigger_sim() {
        for c in fixtures()["trigger_sim"].as_array().unwrap() {
            let mut b = Binding::new("t");
            b.trigger = Trigger::by_name(c["trigger"].as_str().unwrap()).unwrap();
            b.trigger_arg = c["arg"].as_str().unwrap().to_owned();
            let beta = c["beta"].as_bool().unwrap();
            let mut t = Triggers::default();
            let mut fired = Vec::new();
            for (i, f) in c["frames"].as_array().unwrap().iter().enumerate() {
                for m in f["chat"].as_array().into_iter().flatten() {
                    t.chat(m.as_str().unwrap());
                }
                let w = World {
                    health: f["health"].as_f64().unwrap(),
                    hunger: f["hunger"].as_i64().unwrap() as i32,
                    y: 64.0,
                    holding: None,
                    inventory: Vec::new(),
                    inventory_full: f["inventory_full"].as_bool().unwrap_or(false),
                    dimension: f["dimension"].as_str().unwrap().to_owned(),
                    server: match f["server"].as_str() {
                        Some(s) => Server::Remote(s.to_owned()),
                        None => Server::Local,
                    },
                    world_name: f["world"].as_str().unwrap().to_owned(),
                    sneaking: false,
                    dead: f["dead"].as_bool().unwrap_or(false),
                    day_time: 0,
                    screen_open: false,
                };
                let world = f["in_world"].as_bool().unwrap().then_some(&w);
                if !t.tick(beta, world, std::slice::from_ref(&b)).is_empty() {
                    fired.push(i as u64);
                }
            }
            let want: Vec<u64> = c["fired"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect();
            assert_eq!(fired, want, "{}", c["name"]);
        }
    }
}
