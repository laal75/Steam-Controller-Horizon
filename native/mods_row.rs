// Options > Mods: Steam Controller's row. Its Settings button is the page in settings.rs.
fn steam_controller_mods_row(mods: &mut Vec<crate::mod_api::NativeMod>) {
    crate::game::steam_controller::mods_row(mods);
}
