// Toggle Sprint (docs/spec/omnitool/movement-camera.md, section 3): holds the
// sprint key every tick while the module is active and releases it once when
// it is switched off. No screen check (unlike Toggle Sneak). Added to the game
// as `game::omnitool_toggle_sprint`; also a module of this crate.

use super::omnitool_core::{self as core, Category, Host, ModuleInfo};
use std::cell::Cell;

pub const MODULE: ModuleInfo = ModuleInfo {
    id: "toggle_sprint",
    name: "Toggle Sprint",
    category: Category::Player,
    description: "Always sprint while enabled.",
    default_on: false,
    cheaty: false,
    beta: false,
    fair_variant: false,
    replaces: "",
    settings: &[],
    command: None,
    key: super::omnitool_setting::NONE,
    hold: false,
    hud: None,
    tick: Some(tick),
    on_key: None,
    hud_right: None,
    on_action: None,
};

thread_local! {
    /// Java's `applied`: the key was pressed by this module and not yet released.
    static APPLIED: Cell<bool> = const { Cell::new(false) };
}

fn tick(host: &mut dyn Host) {
    let active = core::active(host, &MODULE);
    if let Some(input) = host.input() {
        APPLIED.with(|a| {
            a.set(super::omnitool_toggle_sneak::latch(
                "sprint",
                a.get(),
                active,
                input,
            ))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_core::tests::{Fake, restart};

    #[test]
    fn registered() {
        assert!(
            crate::omnitool_registry::MODULES
                .iter()
                .any(|m| m.id == "toggle_sprint")
        );
    }

    #[test]
    fn inactive_without_input_does_nothing() {
        restart();
        APPLIED.with(|a| a.set(false));
        let mut h = Fake::default();
        core::set_enabled(&mut h, &MODULE, true);
        tick(&mut h); // Fake has no input: must not panic or latch
        assert!(!APPLIED.with(Cell::get));
    }
}
