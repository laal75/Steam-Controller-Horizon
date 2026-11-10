// Rumble: on the Steam Controller's pad haptics while it is the pad playing.
fn steam_controller_vibrate(
    game: &crate::game::Game<'_>,
    rumble: &horizon_client::gamepad::Rumble,
) -> std::ops::ControlFlow<()> {
    if crate::game::steam_controller::vibrate(game, rumble) {
        std::ops::ControlFlow::Break(())
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
