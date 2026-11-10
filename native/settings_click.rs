// A click on the Steam Controller page's button `button`.
fn steam_controller_settings_click(mod_id: &str, button: usize) {
    crate::game::steam_controller::settings_click(mod_id, button);
}
