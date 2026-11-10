// Mouse motion taking the pad out of use: not when it is Steam echoing the controller.
fn steam_controller_keep_pad() -> std::ops::ControlFlow<()> {
    if crate::game::steam_controller::keep_pad() {
        std::ops::ControlFlow::Break(())
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
