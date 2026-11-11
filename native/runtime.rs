// Steam Controller's game side (`crate::game::steam_controller`): the controller's state as the
// game's own pad state, its right trackpad and gyro turning the view, rumble on its haptics.
// The controller itself (HID, lizard mode, the settings file) is the steam_controller_hid crate.
//
// The game reads one pad at a time. While the Steam Controller is in use (touched in the last
// few seconds) it is that pad; otherwise any other controller gilrs sees plays as before.

use super::Game;
use super::input_trace::InputOrigin;
use super::pad::Pads;
use horizon_client::controller_ui::Device;
use horizon_client::gamepad::{Button, GlyphStyle, PadState, Rumble};
use horizon_client::hud::{HudBatch, rgba};
use horizon_client::pvp::{Colour, Outline};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::keyboard::PhysicalKey;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use steam_controller_hid as sc;

/// How long the Steam Controller stays the playing pad after it was last touched.
const HOLD: Duration = Duration::from_secs(3);

/// In the world with no menu open: the right pad turns the view (else it is the right stick).
static PLAYING: AtomicBool = AtomicBool::new(false);
static LAST_TOUCH: Mutex<Option<Instant>> = Mutex::new(None);

/// The Steam Controller is the pad playing.
fn in_use() -> bool {
    LAST_TOUCH
        .lock()
        .ok()
        .and_then(|t| *t)
        .is_some_and(|t| t.elapsed() < HOLD)
}

/// We put the Steam Controller's state in the pads' `feed` (splitscreen's slot, which the poll
/// reads instead of gilrs); splitscreen's own feed is never touched.
static FED: AtomicBool = AtomicBool::new(false);

/// Each poll of the pads, before they are read: start the reader, tell it where the game is,
/// and while the Steam Controller is in use, hand the poll its state.
pub(in crate::game) fn frame(game: &mut Game<'_>) {
    sc::start();
    let playing = game.settings.controller.enabled
        && game.title.is_none()
        && game.player.is_some()
        && !game.pad_in_menu();
    PLAYING.store(playing, Ordering::Relaxed);
    sc::set_context(game.focused, playing);
    let config = sc::config();
    controlify_layout(game, config.clone());
    block_highlight(game, &config);
    // The Alt-Tab chord's button (the reader thread sends the keys): while it is held the d-pad
    // and its own action wait; let go without a window step, its action happens then.
    let snap = sc::snapshot();
    let modifier = chord_modifier(&config);
    let held = modifier != 0 && snap.is_some_and(|s| s.buttons & modifier != 0);
    let was = CHORD_HELD.swap(held, Ordering::Relaxed);
    let clean_release = was && !held && !sc::chord_used();
    // The Steam button set to `overlay` shows and hides the on-screen controls.
    let steam = snap.is_some_and(|s| s.buttons & sc::STEAM != 0);
    let steam_pressed = steam && !STEAM_HELD.swap(true, Ordering::Relaxed);
    if !steam {
        STEAM_HELD.store(false, Ordering::Relaxed);
    }
    let steam_acts = if modifier == sc::STEAM { clean_release } else { steam_pressed };
    if steam_acts && config.steam_button == "overlay" {
        OVERLAY_FLIPPED.fetch_xor(true, Ordering::Relaxed);
    }
    if clean_release && let Ok(mut tap) = TAP.lock() {
        *tap = if modifier == sc::BACK {
            Some(Button::Select)
        } else {
            Button::parse(&config.steam_button)
        };
    }
    radial_frame(game, &config, snap, held);
    keyboard_on_focus(game);

    if FED.swap(false, Ordering::Relaxed) {
        game.pad.feed = None;
    } else if game.pad.feed.is_some() {
        // Splitscreen is reading the pads for every player.
        return;
    }
    if let Some(state) = read(&mut game.pad) {
        game.pad.feed = Some(state);
        FED.store(true, Ordering::Relaxed);
    }
}

/// The Steam Controller's state while it is in use, else `None` (gilrs reads as before).
fn read(pads: &mut Pads) -> Option<PadState> {
    let snap = sc::snapshot()?;
    if snap.touched()
        && let Ok(mut last) = LAST_TOUCH.lock()
    {
        *last = Some(Instant::now());
    }
    if !in_use() {
        return None;
    }
    pads.style = GlyphStyle::Xbox;
    pads.device = Some(Device {
        name: "Steam Controller".into(),
        vendor: Some(0x28de),
        product: Some(snap.product),
    });
    Some(state_of(&snap, &sc::config(), PLAYING.load(Ordering::Relaxed)))
}

/// The Alt-Tab chord's modifier button bit (0 when off).
fn chord_modifier(config: &sc::Config) -> u32 {
    match config.alt_tab.as_str() {
        "steam" => sc::STEAM,
        "back" => sc::BACK,
        _ => 0,
    }
}

/// The Alt-Tab chord's button is held.
static CHORD_HELD: AtomicBool = AtomicBool::new(false);
/// A button to press for one poll: the chord's button let go without a window step.
static TAP: Mutex<Option<Button>> = Mutex::new(None);

/// The controller as a standard pad: Xbox-layout buttons, the stick as the left stick, the left
/// pad as the d-pad, the right pad as the right stick in menus, and the extra inputs as set.
fn state_of(snap: &sc::Snapshot, config: &sc::Config, playing: bool) -> PadState {
    let mut state = PadState::default();
    let modifier = chord_modifier(config);
    let chording = CHORD_HELD.load(Ordering::Relaxed);
    // The radial menu takes the left pad (and B, which closes it) while it is open, and the click
    // or B that opened, ran or closed it until it is let go.
    let (radial_click, radial_b) = radial_swallows();
    let swallowed = |bit: u32| {
        (radial_b && bit == sc::B) || (radial_click && bit == sc::LEFT_PAD_CLICK)
    };
    // The chord's button presses only on release (`TAP`), so it can lead an Alt-Tab instead.
    let held = |bit: u32| snap.buttons & bit != 0 && bit != modifier && !swallowed(bit);
    if let Some(button) = TAP.lock().ok().and_then(|mut t| t.take()) {
        state.set(button, true);
    }
    for (bit, button) in [
        (sc::A, Button::South),
        (sc::B, Button::East),
        (sc::X, Button::West),
        (sc::Y, Button::North),
        (sc::LEFT_BUMPER, Button::LeftBumper),
        (sc::RIGHT_BUMPER, Button::RightBumper),
        (sc::BACK, Button::Select),
        (sc::START, Button::Start),
        (sc::STICK_CLICK, Button::LeftStick),
    ] {
        if held(bit) {
            state.set(button, true);
        }
    }
    let mut extra = |bit: u32, name: &str| {
        if held(bit)
            && let Some(button) = Button::parse(name)
        {
            state.set(button, true);
        }
    };
    extra(sc::LEFT_GRIP, &config.left_grip);
    extra(sc::RIGHT_GRIP, &config.right_grip);
    extra(sc::RIGHT_PAD_CLICK, &config.right_pad_click);
    extra(sc::STEAM, &config.steam_button);

    // Triggers: analog, and fully pulled at their end click.
    state.triggers = [
        if held(sc::LEFT_TRIGGER_CLICK) { 1.0 } else { snap.triggers[0] },
        if held(sc::RIGHT_TRIGGER_CLICK) { 1.0 } else { snap.triggers[1] },
    ];
    state.left = snap.stick;

    // The left pad as an 8-way d-pad.
    let pressed = match config.left_pad {
        sc::LeftPad::Click => held(sc::LEFT_PAD_CLICK),
        sc::LeftPad::Touch => true,
        sc::LeftPad::Off => false,
    };
    // While the chord's button is held the left pad picks windows, not the d-pad; while the
    // radial menu is open, its slots.
    if pressed && !chording && !radial_click && let Some([x, y]) = snap.left_pad {
        let length = (x * x + y * y).sqrt();
        if length < 0.3 {
            // The middle: its own button when clicked.
            if config.left_pad == sc::LeftPad::Click {
                if let Some(button) = Button::parse(&config.left_pad_center) {
                    state.set(button, true);
                }
            }
        } else {
            // sin 22.5°: each direction covers 135°, so diagonals press two.
            let (x, y) = (x / length, y / length);
            state.set(Button::DPadUp, y > 0.383);
            state.set(Button::DPadDown, y < -0.383);
            state.set(Button::DPadRight, x > 0.383);
            state.set(Button::DPadLeft, x < -0.383);
        }
    }

    // In play the right pad is the trackball (`look`). In menus it is not a right stick: a thumb
    // resting on its edge would be a full push, and in an inventory that moves items between the
    // slot under the pointer and the hotbar, one at a time.
    let _ = playing;
    state
}

/// Once per frame in the world: the trackball's and gyro's turn since the last frame.
pub(in crate::game) fn look(game: &mut Game<'_>) {
    let [x, y] = sc::take_look();
    if x == 0.0 && y == 0.0 {
        return;
    }
    let settings = &game.settings.controller;
    if !settings.enabled || game.pad_in_menu() || game.player.is_none() || !game.focused {
        return;
    }
    let ix = if settings.invert_x { -1.0 } else { 1.0 };
    let iy = if settings.invert_y { -1.0 } else { 1.0 };
    // `Entity.turn` units: 0.15 degrees each.
    game.look_turn(f64::from(x * ix / 0.15), f64::from(y * iy / 0.15));
    game.pad.in_use = true;
}

/// A vibration (`pad_rumble`): on the Steam Controller's haptics while it is the pad playing.
/// Returns whether it took it.
pub(in crate::game) fn vibrate(game: &Game<'_>, rumble: &Rumble) -> bool {
    if !in_use() || sc::snapshot().is_none() {
        return false;
    }
    let settings = &game.settings.controller;
    if settings.enabled && settings.vibration
        && let Some(pulse) = rumble.pulse()
    {
        sc::rumble(pulse.strong, pulse.weak, pulse.millis);
    }
    true
}

/// The game is closing: give the controller back its desktop mouse (lizard mode).
pub(in crate::game) fn close() {
    sc::shutdown();
}

/// Mouse look, each frame: while the right pad turns the view, mouse motion is Steam's Desktop
/// layout echoing the pad (Steam stays open, as with Controlify), so it is thrown away. Returns
/// whether it was.
pub(in crate::game) fn mouse_look(game: &mut Game<'_>) -> bool {
    if !(sc::right_pad_busy() || (sc::steam_open() && sc::motion_busy())) {
        return false;
    }
    let look = game.look_options();
    let time = game.started.elapsed().as_secs_f64();
    // Not grabbed: the movement gathered since the last frame is dropped, not turned.
    let _ = game
        .mouse
        .handle_accumulated_movement(time, game.focused, false, look);
    true
}

// ---- Controlify's controls -------------------------------------------------------------------

/// Once (`controlify_layout = pending` in the settings): the game's controller bindings as
/// Controlify's defaults. Horizon's defaults already match them; older settings files carry
/// another layout (drop on B, chat on right, perspective on up), which this replaces.
fn controlify_layout(game: &mut Game<'_>, mut config: sc::Config) {
    if !config.controlify_layout_pending {
        return;
    }
    let pad = &mut game.settings.controller;
    for (action, button) in [
        ("key.jump", Some(Button::South)),
        ("key.drop", Some(Button::DPadDown)),
        ("key.swapOffhand", Some(Button::West)),
        ("key.inventory", Some(Button::North)),
        ("key.use", Some(Button::LeftTrigger)),
        ("key.attack", Some(Button::RightTrigger)),
        ("key.sprint", Some(Button::LeftStick)),
        ("key.sneak", Some(Button::RightStick)),
        ("key.togglePerspective", Some(Button::Select)),
        ("key.pickItem", Some(Button::DPadLeft)),
        ("key.chat", Some(Button::DPadUp)),
        ("key.playerlist", None),
        ("key.advancements", None),
    ] {
        pad.bind(action, button);
    }
    if let Ok(dir) = std::env::current_dir() {
        pad.save(&dir);
    }
    config.controlify_layout_pending = false;
    sc::save_config(&config);
}

// ---- Block highlight -------------------------------------------------------------------------

/// The player's own outline settings, from before the fill took over.
static OWN_OUTLINE: Mutex<Option<Outline>> = Mutex::new(None);

/// `block_highlight = fill`: the targeted block's faces tinted and its outline invisible (the
/// game's own outline-fill option, with the stroke at 0%). `outline` gives back the player's own.
fn block_highlight(game: &mut Game<'_>, config: &sc::Config) {
    let outline = &mut game.settings.extras.pvp.outline;
    let Ok(mut own) = OWN_OUTLINE.lock() else {
        return;
    };
    if config.block_highlight == "fill" {
        let colour = Colour::ALL
            .into_iter()
            .find(|c| c.name() == config.block_highlight_colour)
            .unwrap_or(Colour::White);
        let want = Outline {
            custom: true,
            colour: outline.colour,
            opacity: 0,
            thickness: outline.thickness,
            fill: Some(colour),
        };
        if *outline != want {
            if own.is_none() {
                *own = Some(*outline);
            }
            *outline = want;
        }
    } else if let Some(previous) = own.take() {
        *outline = previous;
    }
}

// ---- Steam's Desktop layout ------------------------------------------------------------------

/// Keys and mouse buttons whose press was Steam's echo: their release goes too.
static ECHO_KEYS: Mutex<Vec<PhysicalKey>> = Mutex::new(Vec::new());
static ECHO_BUTTONS: Mutex<Vec<MouseButton>> = Mutex::new(Vec::new());

/// A window event, before the game sees it: with Steam open, the keys and clicks Steam's
/// Desktop layout makes from the controller's buttons (they arrive with the button), and the
/// mouse motion and scrolling it makes from the stick and pads, are dropped. Returns whether
/// this one was.
pub(in crate::game) fn filter_event(event: &WindowEvent, origin: &InputOrigin) -> bool {
    if *origin != InputOrigin::User || sc::snapshot().is_none() || !sc::steam_open() {
        return false;
    }
    if !sc::config().filter_steam_input {
        return false;
    }
    match event {
        WindowEvent::KeyboardInput { event, .. } => {
            let Ok(mut keys) = ECHO_KEYS.lock() else {
                return false;
            };
            let key = event.physical_key;
            if event.state == ElementState::Pressed {
                if keys.contains(&key) {
                    return true;
                }
                if sc::button_echo() {
                    keys.push(key);
                    return true;
                }
                false
            } else if let Some(i) = keys.iter().position(|k| *k == key) {
                keys.remove(i);
                true
            } else {
                false
            }
        }
        WindowEvent::MouseInput { state, button, .. } => {
            let Ok(mut buttons) = ECHO_BUTTONS.lock() else {
                return false;
            };
            if *state == ElementState::Pressed {
                if sc::button_echo() {
                    buttons.push(*button);
                    return true;
                }
                false
            } else if let Some(i) = buttons.iter().position(|b| b == button) {
                buttons.remove(i);
                true
            } else {
                false
            }
        }
        WindowEvent::MouseWheel { .. } | WindowEvent::CursorMoved { .. } => sc::motion_busy(),
        _ => false,
    }
}

/// Mouse motion would take the pad out of use (and let go of what it holds): not when it is
/// Steam echoing the stick or a pad.
pub(in crate::game) fn keep_pad() -> bool {
    sc::snapshot().is_some() && sc::steam_open() && sc::motion_busy()
}

// ---- On-screen controls ----------------------------------------------------------------------

static STEAM_HELD: AtomicBool = AtomicBool::new(false);
/// The Steam button flipped the overlay from what the settings say.
static OVERLAY_FLIPPED: AtomicBool = AtomicBool::new(false);

/// A bound action's name, as the overlay shows it.
fn action_name(action: &str) -> String {
    let known = match action {
        "key.jump" => "Jump",
        "key.drop" => "Drop",
        "key.swapOffhand" => "Swap hands",
        "key.inventory" => "Inventory",
        "pad.hotbarPrevious" => "Prev slot",
        "pad.hotbarNext" => "Next slot",
        "key.use" => "Use",
        "key.attack" => "Attack",
        "key.sprint" => "Sprint",
        "key.sneak" => "Sneak",
        "key.togglePerspective" => "Perspective",
        "key.pickItem" => "Pick block",
        "key.chat" => "Chat",
        "key.advancements" => "Advancements",
        "key.playerlist" => "Players",
        "key.command" => "Command",
        "key.screenshot" => "Screenshot",
        "key.toggleGui" => "Hide HUD",
        "key.emote" => "Emotes",
        "key.smoothCamera" => "Cinematic",
        "key.fullscreen" => "Fullscreen",
        _ => "",
    };
    if !known.is_empty() {
        return known.into();
    }
    // key.hotbar.3 -> Hotbar 3; anything else by its last part, capitalised.
    let parts: Vec<&str> = action.split('.').collect();
    match parts.as_slice() {
        [.., "hotbar", n] => format!("Hotbar {n}"),
        [.., name] => {
            let mut c = name.chars();
            c.next()
                .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
        }
        _ => action.into(),
    }
}

/// After the whole GUI, in play: each hand's controls in its corner, as they are bound now.
pub(in crate::game) fn draw_overlay(game: &Game<'_>, batch: &mut HudBatch, gui_hidden: bool) {
    let config = sc::config();
    if !gui_hidden {
        draw_radial(game, batch, &config);
    }
    let shown = config.overlay != OVERLAY_FLIPPED.load(Ordering::Relaxed);
    if !shown || gui_hidden || !in_use() || sc::snapshot().is_none() {
        return;
    }
    if game.title.is_some() || game.screen != super::Screen::Playing || game.player.is_none() {
        return;
    }
    let pad = &game.settings.controller;
    let label = |button: Button| -> Option<String> {
        let actions = pad.actions_of(button);
        (!actions.is_empty()).then(|| {
            actions
                .iter()
                .map(|a| action_name(a))
                .collect::<Vec<_>>()
                .join(" / ")
        })
    };
    let extra = |name: &str| Button::parse(name).and_then(label);
    let fixed = |text: &str| Some(text.to_owned());
    // The direction that opens the radial menu shows it instead of its game action.
    let opener = radial_opener(&config);
    let dpad = |button: Button| {
        if opener == Some(button) { Some("Commands".to_owned()) } else { label(button) }
    };
    let left: Vec<(&str, Option<String>)> = vec![
        ("LT", label(Button::LeftTrigger)),
        ("LB", label(Button::LeftBumper)),
        ("Stick", fixed("Move")),
        ("Click", label(Button::LeftStick)),
        ("\u{2191}", dpad(Button::DPadUp)),
        ("\u{2190}", dpad(Button::DPadLeft)),
        ("\u{2192}", dpad(Button::DPadRight)),
        ("\u{2193}", dpad(Button::DPadDown)),
        ("Grip", extra(&config.left_grip)),
        ("View", label(Button::Select)),
    ];
    let right: Vec<(&str, Option<String>)> = vec![
        ("RT", label(Button::RightTrigger)),
        ("RB", label(Button::RightBumper)),
        ("Y", label(Button::North)),
        ("X", label(Button::West)),
        ("B", label(Button::East)),
        ("A", label(Button::South)),
        ("Pad", fixed("Look")),
        ("Click", extra(&config.right_pad_click)),
        ("Grip", extra(&config.right_grip)),
        ("Start", fixed("Pause")),
    ];
    let atlas = &game.gui.atlas;
    const MARGIN: f32 = 4.0;
    const LINE: f32 = 11.0;
    // The room beside the hotbar (182 wide, centred).
    let room = ((batch.width - 182.0) / 2.0 - 2.0 * MARGIN).max(48.0);
    let glyph_colour = |glyph: &str| match glyph {
        "A" => rgba(96, 200, 64, 255),
        "B" => rgba(230, 72, 64, 255),
        "X" => rgba(64, 136, 240, 255),
        "Y" => rgba(240, 200, 48, 255),
        _ => rgba(255, 255, 255, 255),
    };
    for (lines, from_right) in [(left, false), (right, true)] {
        let lines: Vec<(&str, String)> = lines
            .into_iter()
            .filter_map(|(glyph, text)| text.map(|t| (glyph, t)))
            .collect();
        let mut y = batch.height - MARGIN - LINE * lines.len() as f32;
        for (glyph, mut text) in lines {
            let glyph_w = HudBatch::text_width(atlas, glyph) + 4.0;
            // Shortened to fit beside the hotbar.
            while text.chars().count() > 3
                && glyph_w + 3.0 + HudBatch::text_width(atlas, &text) > room
            {
                let kept: String = text.trim_end_matches('.').chars().collect();
                let mut kept: Vec<char> = kept.chars().collect();
                kept.pop();
                text = kept.into_iter().collect::<String>() + ".";
            }
            let text_w = HudBatch::text_width(atlas, &text);
            let width = glyph_w + 3.0 + text_w + 2.0;
            let x = if from_right {
                batch.width - MARGIN - width
            } else {
                MARGIN
            };
            batch.fill(atlas, x, y, width, LINE - 1.0, rgba(0, 0, 0, 110));
            batch.fill(atlas, x, y, glyph_w, LINE - 1.0, rgba(0, 0, 0, 170));
            batch.text(atlas, glyph, x + 2.0, y + 1.0, glyph_colour(glyph), true);
            batch.text(atlas, &text, x + glyph_w + 3.0, y + 1.0, rgba(255, 255, 255, 255), true);
            y += LINE;
        }
    }
}

// ---- Options > Mods > Steam Controller -------------------------------------------------------

const MOD_ID: &str = "steam_controller";
/// The buttons the grips and pad clicks can press, in the order a click steps through them.
const BUTTON_CHOICES: &[&str] = &[
    "south", "east", "west", "north", "lb", "rb", "ls", "rs", "up", "down", "left", "right",
    "select", "none",
];
const SENSITIVITIES: &[f32] = &[100.0, 140.0, 180.0, 220.0, 280.0, 350.0, 450.0];
const DEADZONES: &[f32] = &[0.0, 60.0, 100.0, 150.0, 200.0, 300.0, 450.0];
const VERTICAL_DEADZONES: &[f32] = &[0.0, 300.0, 450.0, 600.0, 800.0, 1000.0, 1300.0];
const SMOOTHINGS: &[f32] = &[0.0, 0.2, 0.35, 0.5, 0.7];
const FRICTIONS: &[f32] = &[2.0, 3.5, 5.0, 8.0, 12.0];
const GYRO_SENSITIVITIES: &[f32] = &[0.5, 1.0, 1.5, 2.0, 3.0, 4.0];
const STRENGTHS: &[f32] = &[0.5, 1.0, 1.5, 2.0, 3.0];
const COLOURS: &[&str] = &[
    "white", "red", "orange", "yellow", "green", "cyan", "blue", "magenta", "black", "inverted",
];

/// The next value after `now` in `list`, round to the start (the nearest one counts as `now`).
fn next_number(list: &[f32], now: f32) -> f32 {
    let at = list
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - now).abs().total_cmp(&(b.1 - now).abs()))
        .map_or(0, |(i, _)| i);
    list[(at + 1) % list.len()]
}

fn next_name(list: &[&str], now: &str) -> String {
    let at = list.iter().position(|n| *n == now);
    list[at.map_or(0, |i| (i + 1) % list.len())].to_owned()
}

fn on_off(on: bool) -> &'static str {
    if on { "On" } else { "Off" }
}

/// A controller button's name and what it does in game now ("A (Jump)"); without the game, the
/// name alone.
fn button_text(game: Option<&Game<'_>>, name: &str) -> String {
    match Button::parse(name) {
        Some(button) => {
            let glyph = button.label(GlyphStyle::Xbox);
            let actions = game.map_or(Vec::new(), |g| g.settings.controller.actions_of(button));
            if actions.is_empty() {
                glyph.to_owned()
            } else {
                let names: Vec<String> = actions.iter().map(|a| action_name(a)).collect();
                format!("{glyph} ({})", names.join(" / "))
            }
        }
        None if name == "overlay" => "On-screen controls".into(),
        None => "Nothing".into(),
    }
}

/// A row of the page: its label, and what a click on it does to the settings.
type Row = (String, Box<dyn Fn(&mut sc::Config)>);

fn row(label: String, act: impl Fn(&mut sc::Config) + 'static) -> Row {
    (label, Box::new(act))
}

/// The page, top to bottom: every setting in `config/steam_controller.txt`.
fn page(game: Option<&Game<'_>>, c: &sc::Config) -> Vec<Row> {
    let mode = match c.mode {
        sc::Mode::On => "On (even with Steam open)",
        sc::Mode::Auto => "Only while Steam is closed",
        sc::Mode::Off => "Off",
    };
    let left_pad = match c.left_pad {
        sc::LeftPad::Click => "D-pad on click",
        sc::LeftPad::Touch => "D-pad on touch",
        sc::LeftPad::Off => "Off",
    };
    let gyro = match c.gyro {
        sc::Gyro::Off => "Off",
        sc::Gyro::Touch => "While touching the right pad",
        sc::Gyro::Always => "Always",
    };
    let highlight = if c.block_highlight == "fill" { "Filled" } else { "Outline (the game's)" };
    let alt_tab = match c.alt_tab.as_str() {
        "steam" => "Hold Steam + left pad right/left",
        "back" => "Hold View + left pad right/left",
        _ => "Off",
    };
    vec![
        // The controller.
        row(format!("Steam Controller: {mode}"), |c| {
            c.mode = match c.mode {
                sc::Mode::On => sc::Mode::Auto,
                sc::Mode::Auto => sc::Mode::Off,
                sc::Mode::Off => sc::Mode::On,
            }
        }),
        row(format!("Filter Steam's keys and mouse: {}", on_off(c.filter_steam_input)), |c| {
            c.filter_steam_input = !c.filter_steam_input
        }),
        // The screen.
        row(format!("Block highlight: {highlight}"), |c| {
            c.block_highlight = if c.block_highlight == "fill" { "outline" } else { "fill" }.into()
        }),
        row(format!("Highlight colour: {}", c.block_highlight_colour), |c| {
            c.block_highlight_colour = next_name(COLOURS, &c.block_highlight_colour)
        }),
        row(format!("On-screen controls: {}", on_off(c.overlay)), |c| {
            c.overlay = !c.overlay;
            // The Steam button's flip starts over from the setting.
            OVERLAY_FLIPPED.store(false, Ordering::Relaxed);
        }),
        // Looking with the right pad.
        row(format!("Look speed: {}", c.trackpad_sensitivity), |c| {
            c.trackpad_sensitivity = next_number(SENSITIVITIES, c.trackpad_sensitivity)
        }),
        row(format!("Look slack left/right: {}", c.trackpad_deadzone), |c| {
            c.trackpad_deadzone = next_number(DEADZONES, c.trackpad_deadzone)
        }),
        row(format!("Look slack up/down: {}", c.trackpad_deadzone_vertical), |c| {
            c.trackpad_deadzone_vertical = next_number(VERTICAL_DEADZONES, c.trackpad_deadzone_vertical)
        }),
        row(format!("Look smoothing: {}", c.trackpad_smoothing), |c| {
            c.trackpad_smoothing = next_number(SMOOTHINGS, c.trackpad_smoothing)
        }),
        row(format!("Look momentum: {}", on_off(c.trackpad_momentum)), |c| {
            c.trackpad_momentum = !c.trackpad_momentum
        }),
        row(format!("Momentum slow-down: {}", c.momentum_friction), |c| {
            c.momentum_friction = next_number(FRICTIONS, c.momentum_friction)
        }),
        row(format!("Invert look Y: {}", on_off(c.trackpad_invert_y)), |c| {
            c.trackpad_invert_y = !c.trackpad_invert_y
        }),
        // Buttons.
        row(format!("Left pad: {left_pad}"), |c| {
            c.left_pad = match c.left_pad {
                sc::LeftPad::Click => sc::LeftPad::Touch,
                sc::LeftPad::Touch => sc::LeftPad::Off,
                sc::LeftPad::Off => sc::LeftPad::Click,
            }
        }),
        row(format!("Left pad centre click: {}", button_text(game, &c.left_pad_center)), |c| {
            c.left_pad_center = next_name(BUTTON_CHOICES, &c.left_pad_center)
        }),
        row(format!("Left grip: {}", button_text(game, &c.left_grip)), |c| {
            c.left_grip = next_name(BUTTON_CHOICES, &c.left_grip)
        }),
        row(format!("Right grip: {}", button_text(game, &c.right_grip)), |c| {
            c.right_grip = next_name(BUTTON_CHOICES, &c.right_grip)
        }),
        row(format!("Right pad click: {}", button_text(game, &c.right_pad_click)), |c| {
            c.right_pad_click = next_name(BUTTON_CHOICES, &c.right_pad_click)
        }),
        row(format!("Steam button: {}", button_text(game, &c.steam_button)), |c| {
            let mut choices = vec!["overlay"];
            choices.extend_from_slice(BUTTON_CHOICES);
            c.steam_button = next_name(&choices, &c.steam_button);
        }),
        row(format!("Alt-Tab: {alt_tab}"), |c| {
            c.alt_tab = next_name(&["steam", "back", "off"], &c.alt_tab)
        }),
        // The radial menu.
        row(
            format!(
                "Radial menu: {}",
                match c.radial_menu.as_str() {
                    "up" | "down" | "left" | "right" => format!("left pad click {}", c.radial_menu),
                    _ => "Off".to_owned(),
                }
            ),
            |c| c.radial_menu = next_name(&["right", "up", "left", "down", "off"], &c.radial_menu),
        ),
    ]
    .into_iter()
    .chain((0..8).map(|i| {
        let slot = c.radial[i].trim();
        let shown = if slot.is_empty() { "Empty" } else { slot };
        row(format!("Radial {}: {shown}", RADIAL_PLACES[i]), move |c: &mut sc::Config| {
            // Steps through common commands; anything else is typed in the settings file.
            c.radial[i] = next_name(RADIAL_PRESETS, c.radial[i].trim());
        })
    }))
    .chain([
        row("Edit radial commands (opens the settings file)".to_owned(), |_| {
            open_settings_file()
        }),
        // Gyro.
        row(format!("Gyro aiming: {gyro}"), |c| {
            c.gyro = match c.gyro {
                sc::Gyro::Off => sc::Gyro::Touch,
                sc::Gyro::Touch => sc::Gyro::Always,
                sc::Gyro::Always => sc::Gyro::Off,
            }
        }),
        row(format!("Gyro speed: {}", c.gyro_sensitivity), |c| {
            c.gyro_sensitivity = next_number(GYRO_SENSITIVITIES, c.gyro_sensitivity)
        }),
        row(format!("Invert gyro X: {}", on_off(c.gyro_invert_x)), |c| {
            c.gyro_invert_x = !c.gyro_invert_x
        }),
        row(format!("Invert gyro Y: {}", on_off(c.gyro_invert_y)), |c| {
            c.gyro_invert_y = !c.gyro_invert_y
        }),
        // Rumble.
        row(format!("Rumble: {}", on_off(c.haptics)), |c| c.haptics = !c.haptics),
        row(format!("Rumble strength: {}", c.haptic_strength), |c| {
            c.haptic_strength = next_number(STRENGTHS, c.haptic_strength)
        }),
        // Applied by the next frame (`controlify_layout`).
        row("Reset controller buttons to Controlify's layout".to_owned(), |c| {
            c.controlify_layout_pending = true
        }),
    ])
    .collect()
}

/// The commands a radial slot's button steps through ("" is empty; `...` finishes in chat).
const RADIAL_PRESETS: &[&str] = &[
    "",
    "/gamemode creative",
    "/gamemode survival",
    "/gamemode spectator",
    "/home",
    "/sethome",
    "/back",
    "/spawn",
    "/warp ...",
    "/tpa ...",
    "/fly",
    "/time set day",
    "/time set night",
    "/weather clear",
];

/// The settings file, in the system's text editor (Notepad on Windows): the radial menu's custom
/// commands are typed there, and saved changes apply within two seconds.
fn open_settings_file() {
    let path = sc::config_path();
    #[cfg(windows)]
    let opened = std::process::Command::new("notepad").arg(&path).spawn();
    #[cfg(target_os = "macos")]
    let opened = std::process::Command::new("open").arg("-t").arg(&path).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let opened = std::process::Command::new("xdg-open").arg(&path).spawn();
    if let Err(e) = opened {
        log::warn!("Steam Controller: opening {}: {e}", path.display());
    }
}

/// Options > Mods lists Steam Controller, with a Settings button for the page.
pub(in crate::game) fn mods_row(mods: &mut Vec<crate::mod_api::NativeMod>) {
    if mods.iter().any(|m| m.id == MOD_ID) {
        return;
    }
    mods.push(crate::mod_api::NativeMod {
        id: MOD_ID.to_owned(),
        title: "Steam Controller 0.5.2".to_owned(),
        description: "The original Steam Controller as a full controller: trackpad look, \
                      Controlify's layout, on-screen controls, filled block highlight."
            .to_owned(),
        keys: Vec::new(),
    });
}

/// The page's labels, asked when it opens and after every click.
pub(in crate::game) fn settings(game: &Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    if mod_id == MOD_ID {
        buttons.extend(page(Some(game), &sc::config()).into_iter().map(|r| r.0));
    }
}

/// Row `index` clicked: its setting steps to the next value and is saved at once.
pub(in crate::game) fn settings_click(mod_id: &str, index: usize) {
    if mod_id != MOD_ID {
        return;
    }
    let mut c = sc::config();
    let Some((_, act)) = page(None, &c).into_iter().nth(index) else {
        return;
    };
    act(&mut c);
    sc::save_config(&c);
}

// ---- Horizon's keyboard on mods' screens -----------------------------------------------------

/// We opened Horizon's on-screen keyboard over a mod's screen (Machine Tech's Terminal and Quest
/// Book, ...): its keys go to that screen as typed characters, Backspace and Enter.
static MOD_KEYBOARD: AtomicBool = AtomicBool::new(false);

/// A search box had focus at the last poll.
static TEXT_FOCUS: AtomicBool = AtomicBool::new(false);

/// A mod says one of its own text boxes has focus (Machine Tech's Terminal search sets
/// `HORIZON_PAD_TEXT_FOCUS=1` while its box takes typing, and clears it after): the mod API has no
/// call for it yet.
fn mod_text_focus() -> bool {
    std::env::var_os("HORIZON_PAD_TEXT_FOCUS").is_some_and(|v| v == "1")
}

/// Each poll: a search box that just got focus while the controller plays (the recipe book's, or
/// a mod's that says so) opens Horizon's on-screen keyboard at the bottom, whichever button or
/// click focused it. Closed with B, it stays closed until the box gets focus again.
fn keyboard_on_focus(game: &mut Game<'_>) {
    let mod_focus = mod_screen_open(game) && mod_text_focus();
    let recipe_focus = game.recipe_search_focused();
    let focus = mod_focus || recipe_focus;
    let was = TEXT_FOCUS.swap(focus, Ordering::Relaxed);
    if focus && !was && in_use() && game.pad.menu.osk.is_none() {
        game.pad.menu.osk = Some(horizon_client::osk::Keyboard::default());
        // The recipe book's search is the game's own: the game types into it.
        MOD_KEYBOARD.store(mod_focus && !recipe_focus, Ordering::Relaxed);
    }
}

/// The mod whose screen is open, if any.
fn mod_screen_open(game: &Game<'_>) -> bool {
    <Game<'_> as horizon_client::mod_api::Host>::mod_screen(game).is_some()
}

/// A controller button in a menu (`pad_menu_button`): on a mod's screen, View opens the keyboard
/// across the bottom of the screen (the D-pad or stick moves over it, A types, X deletes, Y is a
/// space, Start is Enter, B closes it). Returns whether the button was taken.
pub(in crate::game) fn menu_button(game: &mut Game<'_>, button: Button, down: bool) -> bool {
    if !MOD_KEYBOARD.load(Ordering::Relaxed) && game.pad.menu.osk.is_some() {
        return false;
    }
    if game.pad.menu.osk.is_none() {
        MOD_KEYBOARD.store(false, Ordering::Relaxed);
    }
    if !down || button != Button::Select || game.pad.menu.osk.is_some() || !mod_screen_open(game) {
        return false;
    }
    game.pad.menu.osk = Some(horizon_client::osk::Keyboard::default());
    MOD_KEYBOARD.store(true, Ordering::Relaxed);
    true
}

/// Whether the screen under the keyboard can still be typed in: a mod's screen can, while the
/// keyboard we opened for it is up. `None` leaves the game's own answer.
pub(in crate::game) fn typing_screen(game: &Game<'_>) -> Option<bool> {
    (MOD_KEYBOARD.load(Ordering::Relaxed) && mod_screen_open(game)).then_some(true)
}

/// A key of the keyboard we opened over a mod's screen, sent to that screen as the keyboard's
/// own key would be. Returns whether it was ours.
pub(in crate::game) fn keyboard_output(game: &mut Game<'_>, out: &horizon_client::osk::Output) -> bool {
    use horizon_client::osk::Output;
    use winit::keyboard::KeyCode;
    if !MOD_KEYBOARD.load(Ordering::Relaxed) {
        return false;
    }
    let close = |game: &mut Game<'_>| {
        game.pad.menu.osk = None;
        MOD_KEYBOARD.store(false, Ordering::Relaxed);
    };
    if !mod_screen_open(game) {
        close(game);
        return true;
    }
    match out {
        Output::Text(text) => game.screen_chars_typed(Some(text.as_str())),
        Output::Backspace => {
            game.screen_key_pressed(KeyCode::Backspace, false);
        }
        Output::Enter => {
            game.screen_key_pressed(KeyCode::Enter, false);
            close(game);
        }
        Output::Close => close(game),
    }
    true
}

// ---- Radial menu (left pad) ------------------------------------------------------------------

/// The radial menu: open, the slot under the thumb, and the clicks it keeps from the game.
struct Radial {
    open: bool,
    selected: Option<usize>,
    last_click: bool,
    last_b: bool,
    /// The click (or B) that opened, ran or closed it, kept from the game until let go.
    swallow_click: bool,
    swallow_b: bool,
}

static RADIAL: Mutex<Radial> = Mutex::new(Radial {
    open: false,
    selected: None,
    last_click: false,
    last_b: false,
    swallow_click: false,
    swallow_b: false,
});

/// The slots' places, clockwise from the top.
const RADIAL_PLACES: [&str; 8] = [
    "Up", "Up-right", "Right", "Down-right", "Down", "Down-left", "Left", "Up-left",
];

/// The d-pad direction that opens the menu, if it's on.
fn radial_opener(config: &sc::Config) -> Option<Button> {
    match config.radial_menu.as_str() {
        "up" => Some(Button::DPadUp),
        "down" => Some(Button::DPadDown),
        "left" => Some(Button::DPadLeft),
        "right" => Some(Button::DPadRight),
        _ => None,
    }
}

/// The main d-pad direction of a left pad point, past its middle.
fn pad_direction([x, y]: [f32; 2]) -> Option<Button> {
    if (x * x + y * y).sqrt() < 0.3 {
        None
    } else if x.abs() > y.abs() {
        Some(if x > 0.0 { Button::DPadRight } else { Button::DPadLeft })
    } else {
        Some(if y > 0.0 { Button::DPadUp } else { Button::DPadDown })
    }
}

/// The slot (0 at the top, clockwise) a left pad point is on, past its middle.
fn radial_slot([x, y]: [f32; 2]) -> Option<usize> {
    if (x * x + y * y).sqrt() < 0.35 {
        return None;
    }
    // Clockwise from up: atan2(x, y).
    let turns = x.atan2(y).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
    Some((turns * 8.0).round() as usize % 8)
}

/// What the menu keeps from the game this poll: the left pad (and its click), and B.
fn radial_swallows() -> (bool, bool) {
    RADIAL
        .lock()
        .map(|r| (r.open || r.swallow_click, r.open || r.swallow_b))
        .unwrap_or((false, false))
}

/// The radial menu is open (for the overlay).
fn radial_open() -> Option<Option<usize>> {
    RADIAL.lock().ok().filter(|r| r.open).map(|r| r.selected)
}

/// Each poll: open the menu on its d-pad click, follow the thumb, run the slot clicked, close on
/// B or a click in the middle; closed whenever the player isn't in play with the controller.
fn radial_frame(game: &mut Game<'_>, config: &sc::Config, snap: Option<sc::Snapshot>, chording: bool) {
    let Ok(mut r) = RADIAL.lock() else {
        return;
    };
    let Some(s) = snap else {
        r.open = false;
        return;
    };
    let click = s.buttons & sc::LEFT_PAD_CLICK != 0;
    let b = s.buttons & sc::B != 0;
    let click_edge = click && !r.last_click;
    let b_edge = b && !r.last_b;
    r.last_click = click;
    r.last_b = b;
    if !click {
        r.swallow_click = false;
    }
    if !b {
        r.swallow_b = false;
    }
    let playing = PLAYING.load(Ordering::Relaxed) && in_use();
    if !playing {
        r.open = false;
        return;
    }
    if !r.open {
        let opener = radial_opener(config);
        if click_edge
            && !chording
            && opener.is_some()
            && s.left_pad.and_then(pad_direction) == opener
        {
            r.open = true;
            r.selected = None;
            r.swallow_click = true;
        }
        return;
    }
    if let Some(point) = s.left_pad {
        r.selected = radial_slot(point);
    }
    if b_edge {
        r.open = false;
        r.swallow_b = true;
        return;
    }
    if click_edge {
        r.open = false;
        r.swallow_click = true;
        let command = r.selected.map(|i| config.radial[i].clone()).unwrap_or_default();
        drop(r);
        run_radial(game, &command);
    }
}

/// A slot's command, as if typed in chat and sent; one ending in `...` (or a space) opens chat
/// with it typed instead, for the player to finish.
fn run_radial(game: &mut Game<'_>, command: &str) {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return;
    }
    let finish = command.ends_with("...") || command.ends_with(' ');
    let result = if finish {
        let text = format!("{} ", trimmed.trim_end_matches("...").trim_end());
        game.native_open_chat(&text)
    } else {
        game.native_send_chat(trimmed)
    };
    if let Err(e) = result {
        log::warn!("Steam Controller: radial menu {trimmed}: {e}");
    }
}

/// The wheel, in the middle of the screen while the menu is open.
fn draw_radial(game: &Game<'_>, batch: &mut HudBatch, config: &sc::Config) {
    let Some(selected) = radial_open() else {
        return;
    };
    let atlas = &game.gui.atlas;
    let (cx, cy) = (batch.width / 2.0, batch.height / 2.0);
    let radius = (batch.height * 0.28).clamp(48.0, 90.0);
    // The middle: what to do.
    let hint = "Click: run   B: close";
    let hw = HudBatch::text_width(atlas, hint);
    batch.fill(atlas, cx - hw / 2.0 - 3.0, cy - 6.0, hw + 6.0, 12.0, rgba(0, 0, 0, 150));
    batch.text(atlas, hint, cx - hw / 2.0, cy - 4.0, rgba(200, 200, 200, 255), true);
    for (i, slot) in config.radial.iter().enumerate() {
        let angle = i as f32 / 8.0 * std::f32::consts::TAU;
        let (x, y) = (cx + radius * angle.sin(), cy - radius * angle.cos());
        let mut text: String = if slot.trim().is_empty() {
            "-".to_owned()
        } else {
            slot.trim().trim_start_matches('/').to_owned()
        };
        while text.chars().count() > 3 && HudBatch::text_width(atlas, &text) > 88.0 {
            let mut kept: Vec<char> = text.trim_end_matches('.').chars().collect();
            kept.pop();
            text = kept.into_iter().collect::<String>() + ".";
        }
        let w = HudBatch::text_width(atlas, &text) + 8.0;
        let chosen = selected == Some(i);
        let (back, colour) = if chosen {
            (rgba(255, 255, 255, 200), rgba(32, 32, 32, 255))
        } else if slot.trim().is_empty() {
            (rgba(0, 0, 0, 110), rgba(120, 120, 120, 255))
        } else {
            (rgba(0, 0, 0, 170), rgba(255, 255, 255, 255))
        };
        batch.fill(atlas, x - w / 2.0, y - 7.0, w, 14.0, back);
        batch.text(atlas, &text, x - w / 2.0 + 4.0, y - 4.0, colour, !chosen);
    }
}
