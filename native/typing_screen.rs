// The keyboard stays up over a mod's screen it was opened for.
fn steam_controller_typing_screen(game: &crate::game::Game<'_>) -> core::ops::ControlFlow<bool> {
    match crate::game::steam_controller::typing_screen(game) {
        Some(typing) => core::ops::ControlFlow::Break(typing),
        None => core::ops::ControlFlow::Continue(()),
    }
}
