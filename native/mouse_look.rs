// Mouse look: dropped while the Steam Controller's right pad turns the view (Steam's Desktop
// layout makes mouse motion from the pad).
fn steam_controller_mouse_look(game: &mut crate::game::Game<'_>) -> std::ops::ControlFlow<()> {
    if crate::game::steam_controller::mouse_look(game) {
        std::ops::ControlFlow::Break(())
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
