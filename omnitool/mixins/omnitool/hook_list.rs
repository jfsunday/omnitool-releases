// Omnitool's row in Options > Mods.
fn omnitool_listed(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    crate::game::omnitool_glue::listed(mods);
}
