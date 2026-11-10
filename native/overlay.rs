// After the whole GUI: the on-screen controls, left hand bottom left, right hand bottom right.
fn steam_controller_overlay(
    game: &crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
    gui_hidden: bool,
) {
    crate::game::steam_controller::draw_overlay(game, batch, gui_hidden);
}
