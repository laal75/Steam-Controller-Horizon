// A window event, before the game sees it: Steam's Desktop-layout echo of the controller (keys,
// clicks, mouse motion, scrolling) is dropped while Steam is open.
fn steam_controller_filter(
    event: &winit::event::WindowEvent,
    origin: &crate::game::input_trace::InputOrigin,
) -> std::ops::ControlFlow<()> {
    if crate::game::steam_controller::filter_event(event, origin) {
        std::ops::ControlFlow::Break(())
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
