// Each poll of the pads: start the Steam Controller reader, tell it where the game is.
fn steam_controller_frame(game: &mut crate::game::Game<'_>) {
    crate::game::steam_controller::frame(game);
}
