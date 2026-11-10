// A controller button in a menu: on a mod's screen, View opens Horizon's on-screen keyboard.
fn steam_controller_mod_keyboard(
    game: &mut crate::game::Game<'_>,
    button: horizon_client::gamepad::Button,
    down: bool,
) -> core::ops::ControlFlow<Result<()>> {
    if crate::game::steam_controller::menu_button(game, button, down) {
        core::ops::ControlFlow::Break(Ok(()))
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
