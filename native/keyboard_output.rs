// A key of the on-screen keyboard over a mod's screen goes to that screen.
fn steam_controller_keyboard_output(
    game: &mut crate::game::Game<'_>,
    out: &horizon_client::osk::Output,
) -> core::ops::ControlFlow<()> {
    if crate::game::steam_controller::keyboard_output(game, out) {
        core::ops::ControlFlow::Break(())
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
