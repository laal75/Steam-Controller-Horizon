// Options > Mods > Steam Controller > Settings: the page's labels, asked when it opens and after
// every click.
fn steam_controller_settings(game: &crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    crate::game::steam_controller::settings(game, mod_id, buttons);
}
