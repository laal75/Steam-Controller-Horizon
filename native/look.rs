// Each frame in the world: the right trackpad (trackball) and gyro turn the view.
fn steam_controller_look(game: &mut crate::game::Game<'_>) {
    crate::game::steam_controller::look(game);
}
