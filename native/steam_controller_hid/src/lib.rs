//! The Steam Controller (2015) read straight over HID, as SDL and sc-controller do.
//!
//! Without Steam the controller runs in "lizard mode": its right pad moves the mouse, its
//! triggers click and its buttons type keys, and no game sees it as a gamepad (gilrs doesn't).
//! This crate opens its vendor HID interface, switches lizard mode off while the game window has
//! focus (back on when it loses it, or when the game closes, so the desktop mouse works again),
//! and parses its input reports in a thread of its own:
//!
//! - buttons, triggers and stick, for the game's normal pad path (the glue maps them);
//! - the right trackpad as a trackball (finger motion turns the view, with optional momentum),
//!   accumulated here at the controller's own report rate and taken once per frame;
//! - the gyro, optional (always, or only while a thumb rests on the right pad);
//! - haptic pulses on the two pads' actuators, for the game's rumble.
//!
//! Like Controlify (SDL3 with `SDL_HINT_JOYSTICK_HIDAPI_STEAM`), it reads the controller even
//! while Steam is open: Windows shares the HID device, so both see its reports. Steam's Desktop
//! layout then still turns the right pad into mouse motion; the game ignores mouse look while the
//! right pad is in use (`right_pad_busy`). Lizard mode is never switched back on while Steam runs,
//! or the firmware's mouse would come on top of Steam's.
//!
//! Settings: `config/steam_controller.txt` in the game folder (the instance), written with
//! defaults on first start and re-read when it changes.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

// ---------------------------------------------------------------------------------------------
// Buttons, as the controller's report has them (24 bits).

pub const RIGHT_TRIGGER_CLICK: u32 = 0x0000_0001;
pub const LEFT_TRIGGER_CLICK: u32 = 0x0000_0002;
pub const RIGHT_BUMPER: u32 = 0x0000_0004;
pub const LEFT_BUMPER: u32 = 0x0000_0008;
pub const Y: u32 = 0x0000_0010;
pub const B: u32 = 0x0000_0020;
pub const X: u32 = 0x0000_0040;
pub const A: u32 = 0x0000_0080;
/// The small button left of the Steam button ("Back").
pub const BACK: u32 = 0x0000_1000;
pub const STEAM: u32 = 0x0000_2000;
/// The small button right of the Steam button ("Start").
pub const START: u32 = 0x0000_4000;
pub const LEFT_GRIP: u32 = 0x0000_8000;
pub const RIGHT_GRIP: u32 = 0x0001_0000;
pub const LEFT_PAD_CLICK: u32 = 0x0002_0000;
pub const RIGHT_PAD_CLICK: u32 = 0x0004_0000;
pub const LEFT_PAD_TOUCH: u32 = 0x0008_0000;
pub const RIGHT_PAD_TOUCH: u32 = 0x0010_0000;
pub const STICK_CLICK: u32 = 0x0040_0000;
/// The thumb is on the left pad while the stick is tilted: reports alternate between them.
const LEFT_PAD_AND_STICK: u32 = 0x0080_0000;

const VALVE: u16 = 0x28de;
const WIRED: u16 = 0x1102;
const DONGLE: u16 = 0x1142;

// ---------------------------------------------------------------------------------------------
// Settings.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Only while Steam is closed.
    Auto,
    On,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftPad {
    /// A d-pad that presses when the pad is clicked (as Steam's default).
    Click,
    /// A d-pad that presses on touch.
    Touch,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gyro {
    Off,
    /// Gyro aims while a thumb rests on the right pad (lift it to re-centre your hands).
    Touch,
    Always,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub mode: Mode,
    pub left_pad: LeftPad,
    /// Degrees the view turns for a swipe across the whole right pad.
    pub trackpad_sensitivity: f32,
    pub trackpad_momentum: bool,
    /// How fast a flicked trackball slows down (higher stops sooner).
    pub momentum_friction: f32,
    pub trackpad_invert_y: bool,
    /// How far (pad units, the pad is 65536 wide) the thumb has to move before the view turns:
    /// a resting thumb's wobble stays inside it.
    pub trackpad_deadzone: f32,
    /// 0 hands each move straight to the view; towards 1 spreads it over more reports.
    pub trackpad_smoothing: f32,
    pub gyro: Gyro,
    /// 1.0 turns the view as far as the controller turned.
    pub gyro_sensitivity: f32,
    pub gyro_invert_x: bool,
    pub gyro_invert_y: bool,
    pub haptics: bool,
    pub haptic_strength: f32,
    /// The pad buttons the Steam Controller's extra inputs press (south east west north lb rb lt
    /// rt select start ls rs up down left right, or none).
    pub left_grip: String,
    pub right_grip: String,
    pub right_pad_click: String,
    pub left_pad_center: String,
    /// A pad button, none, or `overlay` (shows and hides the on-screen controls).
    pub steam_button: String,
    /// The on-screen controls: left-hand inputs bottom left, right-hand ones bottom right.
    pub overlay: bool,
    /// The targeted block: `fill` (tinted faces, no outline) or `outline` (the game's own).
    pub block_highlight: String,
    pub block_highlight_colour: String,
    /// Drop the keys, clicks and mouse motion Steam's Desktop layout makes from the controller
    /// while Steam is open.
    pub filter_steam_input: bool,
    /// Alt-Tab from the controller: `steam` or `back` (that button held, left pad clicked
    /// right/left), or `off`.
    pub alt_tab: String,
    /// The game's controller bindings still have to be set to Controlify's layout (once).
    pub controlify_layout_pending: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mode: Mode::On,
            left_pad: LeftPad::Click,
            trackpad_sensitivity: 220.0,
            trackpad_momentum: true,
            momentum_friction: 5.0,
            trackpad_invert_y: false,
            trackpad_deadzone: 300.0,
            trackpad_smoothing: 0.35,
            gyro: Gyro::Off,
            gyro_sensitivity: 1.5,
            gyro_invert_x: false,
            gyro_invert_y: false,
            haptics: true,
            haptic_strength: 1.0,
            left_grip: "south".into(),
            right_grip: "ls".into(),
            right_pad_click: "rs".into(),
            left_pad_center: "none".into(),
            steam_button: "overlay".into(),
            overlay: true,
            block_highlight: "fill".into(),
            block_highlight_colour: "white".into(),
            filter_steam_input: true,
            alt_tab: "steam".into(),
            controlify_layout_pending: true,
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Self {
        let mut c = Config::default();
        let mut version = 1;
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim().to_ascii_lowercase());
            let flag = |old: bool| match value.as_str() {
                "true" | "on" | "yes" | "1" => true,
                "false" | "off" | "no" | "0" => false,
                _ => old,
            };
            let number = |old: f32| value.parse::<f32>().ok().filter(|v| v.is_finite()).unwrap_or(old);
            match key {
                "settings_version" => version = value.parse().unwrap_or(1),
                "mode" => {
                    c.mode = match value.as_str() {
                        "on" => Mode::On,
                        "off" => Mode::Off,
                        _ => Mode::Auto,
                    }
                }
                "left_pad" => {
                    c.left_pad = match value.as_str() {
                        "touch" => LeftPad::Touch,
                        "off" => LeftPad::Off,
                        _ => LeftPad::Click,
                    }
                }
                "trackpad_sensitivity" => c.trackpad_sensitivity = number(c.trackpad_sensitivity).clamp(10.0, 2000.0),
                "trackpad_momentum" => c.trackpad_momentum = flag(c.trackpad_momentum),
                "momentum_friction" => c.momentum_friction = number(c.momentum_friction).clamp(0.5, 50.0),
                "trackpad_invert_y" => c.trackpad_invert_y = flag(c.trackpad_invert_y),
                "trackpad_deadzone" => c.trackpad_deadzone = number(c.trackpad_deadzone).clamp(0.0, 4000.0),
                "trackpad_smoothing" => c.trackpad_smoothing = number(c.trackpad_smoothing).clamp(0.0, 0.95),
                "gyro" => {
                    c.gyro = match value.as_str() {
                        "touch" => Gyro::Touch,
                        "always" | "on" => Gyro::Always,
                        _ => Gyro::Off,
                    }
                }
                "gyro_sensitivity" => c.gyro_sensitivity = number(c.gyro_sensitivity).clamp(0.05, 20.0),
                "gyro_invert_x" => c.gyro_invert_x = flag(c.gyro_invert_x),
                "gyro_invert_y" => c.gyro_invert_y = flag(c.gyro_invert_y),
                "haptics" => c.haptics = flag(c.haptics),
                "haptic_strength" => c.haptic_strength = number(c.haptic_strength).clamp(0.0, 3.0),
                "left_grip" => c.left_grip = value,
                "right_grip" => c.right_grip = value,
                "right_pad_click" => c.right_pad_click = value,
                "left_pad_center" => c.left_pad_center = value,
                "steam_button" => c.steam_button = value,
                "overlay" => c.overlay = flag(c.overlay),
                "block_highlight" => c.block_highlight = value,
                "block_highlight_colour" => c.block_highlight_colour = value,
                "filter_steam_input" => c.filter_steam_input = flag(c.filter_steam_input),
                "alt_tab" => c.alt_tab = value,
                "controlify_layout" => c.controlify_layout_pending = value == "pending",
                _ => {}
            }
        }
        // Version 1 defaulted to auto (off while Steam runs); reading with Steam open is the
        // default since version 2.
        if version < 2 && c.mode == Mode::Auto {
            c.mode = Mode::On;
        }
        // Version 3: the Steam button shows the on-screen controls, and the game's bindings go
        // to Controlify's layout once.
        // Version 4: the grips swapped (left jumps, right sprints).
        // Version 5: lighter smoothing (0.7 lagged the view by ~30-55 ms).
        if version < 5 && (c.trackpad_smoothing - 0.7).abs() < 0.01 {
            c.trackpad_smoothing = 0.35;
        }
        if version < 4 && c.left_grip == "ls" && c.right_grip == "south" {
            std::mem::swap(&mut c.left_grip, &mut c.right_grip);
        }
        if version < 3 {
            if c.steam_button == "none" {
                c.steam_button = "overlay".into();
            }
            c.controlify_layout_pending = true;
        }
        c
    }

    pub fn serialize(&self) -> String {
        let mode = match self.mode {
            Mode::Auto => "auto",
            Mode::On => "on",
            Mode::Off => "off",
        };
        let left_pad = match self.left_pad {
            LeftPad::Click => "click",
            LeftPad::Touch => "touch",
            LeftPad::Off => "off",
        };
        let gyro = match self.gyro {
            Gyro::Off => "off",
            Gyro::Touch => "touch",
            Gyro::Always => "always",
        };
        format!(
            "# Steam Controller settings (Horizon mod steam_controller). Saved changes apply within\n\
             # two seconds, no restart needed.\n\
             \n\
             settings_version = 5\n\
             \n\
             # on: read the controller even while Steam is open (as Controlify does).\n\
             # auto: only while Steam is closed. off: never.\n\
             mode = {mode}\n\
             \n\
             # Left trackpad as the d-pad: click (press the pad), touch, or off.\n\
             left_pad = {left_pad}\n\
             \n\
             # Right trackpad turns the view like a trackball.\n\
             # Degrees for a swipe across the whole pad.\n\
             trackpad_sensitivity = {}\n\
             # Keep turning after a flick, slowing down by momentum_friction.\n\
             trackpad_momentum = {}\n\
             momentum_friction = {}\n\
             trackpad_invert_y = {}\n\
             # Pad units (the pad is 65536 wide) the thumb moves before the view turns; raise it\n\
             # if a resting thumb still wobbles the view, lower it for finer aim.\n\
             trackpad_deadzone = {}\n\
             # 0 to 0.95: higher is smoother but a little slower to follow.\n\
             trackpad_smoothing = {}\n\
             \n\
             # Gyro aiming: off, touch (while a thumb rests on the right pad), always.\n\
             gyro = {gyro}\n\
             gyro_sensitivity = {}\n\
             gyro_invert_x = {}\n\
             gyro_invert_y = {}\n\
             \n\
             # Rumble on the pads' haptics (also needs vibration on in Controls > Controller).\n\
             haptics = {}\n\
             haptic_strength = {}\n\
             \n\
             # What the extra inputs press, as a controller button: south east west north lb rb\n\
             # lt rt select start ls rs up down left right, or none. Which action each button does\n\
             # is set in the game's own controller settings (ls = sprint, rs = sneak by default).\n\
             left_grip = {}\n\
             right_grip = {}\n\
             right_pad_click = {}\n\
             left_pad_center = {}\n\
             steam_button = {}\n\
             \n\
             # On-screen controls: left-hand inputs bottom left, right-hand ones bottom right.\n\
             # steam_button = overlay shows and hides them in game.\n\
             overlay = {}\n\
             \n\
             # The targeted block: fill (tinted, no outline) or outline (the game's own).\n\
             # Colour: white red orange yellow green cyan blue magenta black inverted.\n\
             block_highlight = {}\n\
             block_highlight_colour = {}\n\
             \n\
             # With Steam open, drop the keys, clicks and mouse motion Steam's Desktop layout\n\
             # makes from the controller (B as Space, A as Enter, the stick as the mouse...).\n\
             filter_steam_input = {}\n\
             \n\
             # Alt-Tab: hold the Steam button (steam) or View button (back) and click the left\n\
             # pad right or left to step through windows; let go to switch. off to turn it off.\n\
             alt_tab = {}\n\
             \n\
             # pending: set the game's controller bindings to Controlify's layout at next start\n\
             # (drop on d-pad down, chat on up, perspective on View, B only for back). done: leave them.\n\
             controlify_layout = {}\n",
            self.trackpad_sensitivity,
            self.trackpad_momentum,
            self.momentum_friction,
            self.trackpad_invert_y,
            self.trackpad_deadzone,
            self.trackpad_smoothing,
            self.gyro_sensitivity,
            self.gyro_invert_x,
            self.gyro_invert_y,
            self.haptics,
            self.haptic_strength,
            self.left_grip,
            self.right_grip,
            self.right_pad_click,
            self.left_pad_center,
            self.steam_button,
            self.overlay,
            self.block_highlight,
            self.block_highlight_colour,
            self.filter_steam_input,
            self.alt_tab,
            if self.controlify_layout_pending { "pending" } else { "done" },
        )
    }
}

pub fn config_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_default()
        .join("config")
        .join("steam_controller.txt")
}

/// The settings now (re-read by the reader thread when the file changes).
pub fn config() -> Config {
    shared().lock().map(|s| s.config.clone()).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// What the game reads.

/// The active Steam Controller's input now.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// The report's button bits (the constants above).
    pub buttons: u32,
    /// 0..1, left then right.
    pub triggers: [f32; 2],
    /// -1..1, right and up positive.
    pub stick: [f32; 2],
    /// Where the thumb is on each pad (-1..1, right and up positive), while it touches.
    pub left_pad: Option<[f32; 2]>,
    pub right_pad: Option<[f32; 2]>,
    pub product: u16,
}

impl Snapshot {
    /// Anything pressed, pulled, tilted or touched.
    pub fn touched(&self) -> bool {
        self.buttons != 0
            || self.triggers.iter().any(|t| *t > 0.05)
            || self.stick.iter().any(|v| v.abs() > 0.2)
            || self.left_pad.is_some()
            || self.right_pad.is_some()
    }
}

struct Shared {
    config: Config,
    /// The controller last used, with its latest state.
    active: Option<Snapshot>,
    /// View turn waiting to be taken, in degrees (x right, y down).
    look: [f32; 2],
    /// The game window has focus.
    focused: bool,
    /// The player is in the world with no menu open: the right pad is a trackball.
    playing: bool,
    rumbles: Vec<(f32, f32, u32)>,
    /// The game is closing: give the controller its lizard mode back.
    closing: bool,
    /// When the right pad last turned the view (touched, or coasting on momentum).
    right_busy: Option<Instant>,
    /// When the stick was last tilted or the left pad touched (Steam makes mouse motion and
    /// scrolling from them).
    motion_busy: Option<Instant>,
    /// When a button, trigger click or pad click last went down or up.
    edge: Option<Instant>,
    steam: bool,
    /// The Alt-Tab chord stepped a window during this hold of its modifier.
    chord_used: bool,
    started: bool,
}

fn shared() -> &'static Mutex<Shared> {
    static SHARED: OnceLock<Mutex<Shared>> = OnceLock::new();
    SHARED.get_or_init(|| {
        Mutex::new(Shared {
            config: Config::default(),
            active: None,
            look: [0.0; 2],
            focused: true,
            playing: false,
            rumbles: Vec::new(),
            closing: false,
            right_busy: None,
            motion_busy: None,
            edge: None,
            steam: false,
            chord_used: false,
            started: false,
        })
    })
}

/// Start the reader thread (once). Cheap to call every frame.
pub fn start() {
    let Ok(mut s) = shared().lock() else {
        return;
    };
    if s.started {
        return;
    }
    s.started = true;
    s.config = load_config();
    drop(s);
    {
        let spawned = std::thread::Builder::new()
            .name("steam-controller".into())
            .spawn(reader::run);
        if let Err(e) = spawned {
            log::warn!("Steam Controller: reader thread failed to start: {e}");
        }
    }
}

/// The active controller's state, if one is connected and in use by this mod.
pub fn snapshot() -> Option<Snapshot> {
    shared().lock().ok().and_then(|s| s.active)
}

/// The game tells where it is, each frame.
pub fn set_context(focused: bool, playing: bool) {
    if let Ok(mut s) = shared().lock() {
        s.focused = focused;
        s.playing = playing;
        if !playing || !focused {
            s.look = [0.0; 2];
        }
    }
}

/// The view turn since the last call, in degrees (x right, y down).
pub fn take_look() -> [f32; 2] {
    shared()
        .lock()
        .map(|mut s| std::mem::take(&mut s.look))
        .unwrap_or_default()
}

/// The right pad is turning the view (or did a moment ago): mouse motion now is Steam's
/// Desktop layout echoing the pad, not a real mouse.
pub fn right_pad_busy() -> bool {
    shared()
        .lock()
        .ok()
        .and_then(|s| s.right_busy)
        .is_some_and(|t| t.elapsed() < Duration::from_millis(250))
}

/// The controller is making Steam move the mouse or scroll right now (stick, either pad).
pub fn motion_busy() -> bool {
    let fresh = |t: Option<Instant>| t.is_some_and(|t| t.elapsed() < Duration::from_millis(250));
    shared()
        .lock()
        .is_ok_and(|s| fresh(s.right_busy) || fresh(s.motion_busy))
}

/// A controller button just changed: a key or click arriving now, with Steam open, is Steam's
/// Desktop layout echoing it.
pub fn button_echo() -> bool {
    shared().lock().is_ok_and(|s| {
        s.steam && s.edge.is_some_and(|t| t.elapsed() < Duration::from_millis(80))
    })
}

/// The Alt-Tab chord was used during the current (or last) hold of its modifier button: that
/// button's own action is skipped.
pub fn chord_used() -> bool {
    shared().lock().is_ok_and(|s| s.chord_used)
}

/// Steam is running.
pub fn steam_open() -> bool {
    shared().lock().is_ok_and(|s| s.steam)
}

/// Write the settings back (after the game applied a one-time step).
pub fn save_config(config: &Config) {
    if let Ok(mut s) = shared().lock() {
        s.config = config.clone();
    }
    let _ = std::fs::write(config_path(), config.serialize());
}

/// Rumble: strong on the left pad, weak on the right, 0..1, for `millis`.
pub fn rumble(strong: f32, weak: f32, millis: u32) {
    if let Ok(mut s) = shared().lock()
        && s.config.haptics
        && s.active.is_some()
    {
        s.rumbles.push((strong, weak, millis));
    }
}

/// The game is quitting: lizard mode back on (the reader thread does it, briefly waited for).
pub fn shutdown() {
    if let Ok(mut s) = shared().lock() {
        if !s.started {
            return;
        }
        s.closing = true;
    }
    for _ in 0..25 {
        std::thread::sleep(Duration::from_millis(4));
        if shared().lock().is_ok_and(|s| !s.closing) {
            break;
        }
    }
}

fn load_config() -> Config {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let c = Config::parse(&text);
            // Add settings a newer version introduced, keeping the player's values.
            let full = c.serialize();
            if full != text {
                let _ = std::fs::write(&path, full);
            }
            c
        }
        Err(_) => {
            let c = Config::default();
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, c.serialize());
            c
        }
    }
}

fn config_mtime() -> Option<SystemTime> {
    std::fs::metadata(config_path()).and_then(|m| m.modified()).ok()
}

/// Whether Steam is running, kept up to date by `watch_steam`'s thread.
static STEAM_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Check for Steam now, then every five seconds on a thread of its own (the check is slow).
fn watch_steam() {
    use std::sync::atomic::Ordering;
    STEAM_RUNNING.store(steam_running(), Ordering::Relaxed);
    let spawned = std::thread::Builder::new()
        .name("steam-controller-steam-check".into())
        .spawn(|| loop {
            std::thread::sleep(Duration::from_secs(5));
            STEAM_RUNNING.store(steam_running(), Ordering::Relaxed);
        });
    if let Err(e) = spawned {
        log::warn!("Steam Controller: the Steam check thread failed to start: {e}");
    }
}

/// Whether Steam is running (it then owns the controller).
fn steam_running() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq steam.exe", "/NH", "/FO", "CSV"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).to_ascii_lowercase().contains("\"steam.exe\""))
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_dir("/proc").is_ok_and(|dir| {
            dir.flatten().any(|e| {
                std::fs::read_to_string(e.path().join("comm"))
                    .is_ok_and(|c| c.trim() == "steam" || c.trim() == "steamwebhelper")
            })
        })
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("pgrep")
            .args(["-x", "steam_osx"])
            .output()
            .is_ok_and(|o| o.status.success())
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        false
    }
}

// ---------------------------------------------------------------------------------------------
// The reader thread.

mod reader {
    use super::*;
    use hidapi::{HidApi, HidDevice};
    use std::ffi::CString;

    /// Feature report commands.
    const CLEAR_DIGITAL_MAPPINGS: u8 = 0x81;
    const SET_DEFAULT_DIGITAL_MAPPINGS: u8 = 0x85;
    const SET_SETTINGS_VALUES: u8 = 0x87;
    const LOAD_DEFAULT_SETTINGS: u8 = 0x8e;
    const TRIGGER_HAPTIC_PULSE: u8 = 0x8f;

    /// Settings registers.
    const SETTING_LEFT_TRACKPAD_MODE: u8 = 0x07;
    const SETTING_RIGHT_TRACKPAD_MODE: u8 = 0x08;
    const SETTING_SMOOTH_ABSOLUTE_MOUSE: u8 = 0x18;
    const SETTING_ENABLE_RAW_JOYSTICK: u8 = 0x2e;
    const SETTING_IMU_MODE: u8 = 0x30;
    const SETTING_WIRELESS_PACKET_VERSION: u8 = 0x31;
    const SETTING_SLEEP_INACTIVITY_TIMEOUT: u8 = 0x32;
    const TRACKPAD_NONE: u8 = 0x07;
    /// Raw accelerometer and gyro in the input report.
    const IMU_RAW_ACCEL_AND_GYRO: u8 = 0x18;

    /// Gyro full scale: ±2000 degrees per second over ±32768.
    const GYRO_DPS: f32 = 2000.0 / 32768.0;

    struct Pad {
        dev: HidDevice,
        path: CString,
        product: u16,
        /// Lizard mode is off (we configured it).
        configured: bool,
        configured_gyro: bool,
        /// A configure was sent since it was last needed (an empty slot's failure waits).
        config_tried: bool,
        last_config: Instant,
        /// The dongle says a controller is paired and on (wired is always).
        present: bool,
        buttons: u32,
        triggers: [u8; 2],
        stick: [i16; 2],
        left_pad: [i16; 2],
        right_pad: [i16; 2],
        gyro: [i16; 3],
        /// The right pad's last point, while touched (for the trackball).
        right_last: Option<[i16; 2]>,
        /// Trackball speed in degrees per second, for momentum.
        velocity: [f32; 2],
        /// Reports since the thumb touched the right pad.
        touch_reports: u8,
        /// Turn not yet handed out (the smoothing's).
        pending: [f32; 2],
        /// When the thumb last moved past the deadzone.
        last_motion: Option<Instant>,
        last_report: Option<Instant>,
        last_touch: Option<Instant>,
        /// A button, trigger click or pad click changed.
        last_edge: Option<Instant>,
    }

    impl Pad {
        fn send(&self, command: u8, data: &[u8]) -> bool {
            let mut buf = [0u8; 65];
            buf[1] = command;
            buf[2] = data.len() as u8;
            buf[3..3 + data.len()].copy_from_slice(data);
            match self.dev.send_feature_report(&buf) {
                Ok(()) => true,
                Err(e) => {
                    log::debug!("Steam Controller: feature report {command:#x}: {e}");
                    false
                }
            }
        }

        /// Lizard mode off: no mouse or keys, both pads raw, the gyro as set.
        fn configure(&mut self, gyro: bool) {
            let imu = if gyro { IMU_RAW_ACCEL_AND_GYRO } else { 0 };
            let ok = self.send(CLEAR_DIGITAL_MAPPINGS, &[])
                && self.send(
                    SET_SETTINGS_VALUES,
                    &[
                        SETTING_SLEEP_INACTIVITY_TIMEOUT, 0x84, 0x03, // 900 s
                        SETTING_SMOOTH_ABSOLUTE_MOUSE, 0, 0,
                        SETTING_WIRELESS_PACKET_VERSION, 2, 0,
                        SETTING_RIGHT_TRACKPAD_MODE, TRACKPAD_NONE, 0,
                        SETTING_LEFT_TRACKPAD_MODE, TRACKPAD_NONE, 0,
                        SETTING_IMU_MODE, imu, 0,
                        SETTING_ENABLE_RAW_JOYSTICK, 0, 0,
                    ],
                );
            if ok && !self.configured {
                log::info!("Steam Controller: lizard mode off");
            }
            self.configured = ok;
            self.config_tried = true;
            self.configured_gyro = gyro;
            self.last_config = Instant::now();
        }

        /// Lizard mode back on: the right pad is the mouse again.
        fn restore(&mut self) {
            if self.configured {
                self.send(SET_DEFAULT_DIGITAL_MAPPINGS, &[]);
                self.send(LOAD_DEFAULT_SETTINGS, &[]);
                self.configured = false;
                self.config_tried = false;
                self.right_last = None;
                self.velocity = [0.0; 2];
                log::info!("Steam Controller: lizard mode back on");
            }
        }

        /// One haptic pulse train on one pad (0 right, 1 left).
        fn pulse(&self, side: u8, level: f32, millis: u32) {
            if level <= 0.02 || millis == 0 {
                return;
            }
            // A 250 Hz buzz whose on-time sets the strength.
            const CYCLE: u32 = 4000;
            let on = ((level.clamp(0.0, 1.0) * 3400.0) as u32).clamp(120, 3600);
            let off = CYCLE - on;
            let count = (millis * 1000 / CYCLE).clamp(1, 0xffff);
            let [on0, on1] = (on as u16).to_le_bytes();
            let [off0, off1] = (off as u16).to_le_bytes();
            let [n0, n1] = (count as u16).to_le_bytes();
            self.send(TRIGGER_HAPTIC_PULSE, &[side, on0, on1, off0, off1, n0, n1]);
        }

        fn snapshot(&self) -> Snapshot {
            let unit = |v: i16| (f32::from(v) / 32767.0).clamp(-1.0, 1.0);
            let left_touch = self.buttons & LEFT_PAD_TOUCH != 0;
            let right_touch = self.buttons & RIGHT_PAD_TOUCH != 0;
            Snapshot {
                buttons: self.buttons,
                triggers: self.triggers.map(|t| f32::from(t) / 255.0),
                stick: self.stick.map(unit),
                left_pad: left_touch.then(|| self.left_pad.map(unit)),
                right_pad: right_touch.then(|| self.right_pad.map(unit)),
                product: self.product,
            }
        }

        /// Parse one report; returns whether it carried input.
        fn report(&mut self, raw: &[u8]) -> bool {
            // Some backends keep the (zero) report id in front.
            let d = if raw.len() >= 65 && raw[0] == 0 { &raw[1..] } else { raw };
            if d.len() < 36 || d[0] != 0x01 {
                return false;
            }
            match d[2] {
                // Input.
                0x01 => {}
                // Wireless connection change.
                0x03 => {
                    match d[4] {
                        0x01 => {
                            log::info!("Steam Controller: wireless controller off");
                            self.present = false;
                            self.configured = false;
                            self.config_tried = false;
                            self.buttons = 0;
                            self.triggers = [0; 2];
                            self.stick = [0; 2];
                        }
                        0x02 => {
                            log::info!("Steam Controller: wireless controller on");
                            self.present = true;
                            self.configured = false;
                            self.config_tried = false;
                        }
                        _ => {}
                    }
                    return false;
                }
                _ => return false,
            }
            self.present = true;
            let i16_at = |i: usize| i16::from_le_bytes([d[i], d[i + 1]]);
            let buttons = u32::from(d[8]) | u32::from(d[9]) << 8 | u32::from(d[10]) << 16;
            self.triggers = [d[11], d[12]];
            let shared_xy = [i16_at(16), i16_at(18)];
            if buttons & LEFT_PAD_TOUCH != 0 {
                self.left_pad = shared_xy;
                // Thumb on the pad with the stick centred: the stick is at rest. With both,
                // reports alternate and the stick keeps its last value meanwhile.
                if buttons & LEFT_PAD_AND_STICK == 0 {
                    self.stick = [0; 2];
                }
            } else {
                self.stick = shared_xy;
            }
            self.right_pad = [i16_at(20), i16_at(22)];
            // After the redundant analog triggers (24..28) and the accelerometer (28..34).
            self.gyro = [i16_at(34), i16_at(36), i16_at(38)];
            const TOUCHES: u32 = LEFT_PAD_TOUCH | RIGHT_PAD_TOUCH | LEFT_PAD_AND_STICK;
            if (buttons ^ self.buttons) & !TOUCHES != 0 {
                self.last_edge = Some(Instant::now());
            }
            self.buttons = buttons;
            true
        }

        /// One report read: parsed, its look added, its touch noted.
        fn ingest(&mut self, data: &[u8], config: &Config, look_live: bool, look: &mut [f32; 2]) {
            if !self.report(data) {
                return;
            }
            let t = Instant::now();
            let dt = self
                .last_report
                .map_or(0.0, |l| t.duration_since(l).as_secs_f32().min(0.05));
            self.last_report = Some(t);
            if self.configured {
                self.look(config, dt, look_live, look);
            }
            if self.snapshot().touched() {
                self.last_touch = Some(t);
            }
        }

        /// The trackball and gyro, at report rate, into the shared look.
        fn look(&mut self, config: &Config, dt: f32, playing: bool, look: &mut [f32; 2]) {
            let touching = self.buttons & RIGHT_PAD_TOUCH != 0;
            if !playing {
                self.right_last = None;
                self.velocity = [0.0; 2];
                self.pending = [0.0; 2];
                return;
            }
            // Degrees per pad unit: the whole width is 65536 units.
            let scale = config.trackpad_sensitivity / 65536.0;
            let invert = if config.trackpad_invert_y { -1.0 } else { 1.0 };
            if touching {
                let now = self.right_pad;
                match self.right_last {
                    // Touch-down: the anchor, nothing turns yet.
                    None => {
                        self.right_last = Some(now);
                        self.touch_reports = 0;
                        self.velocity = [0.0; 2];
                        self.last_motion = None;
                    }
                    Some(anchor) => {
                        self.touch_reports = self.touch_reports.saturating_add(1);
                        let dx = f32::from(now[0]) - f32::from(anchor[0]);
                        let dy = f32::from(now[1]) - f32::from(anchor[1]);
                        if dx.abs() >= 16000.0 || dy.abs() >= 16000.0 || self.touch_reports <= 2 {
                            // A jump this big is a new touch the firmware didn't report as one;
                            // and the first reports after touch-down are the thumb settling.
                            self.right_last = Some(now);
                        } else if dx.hypot(dy) >= config.trackpad_deadzone {
                            // Past the deadzone from where the thumb rested: the whole way
                            // turns, and the rest point moves along. A still thumb's jitter
                            // never gets this far.
                            self.right_last = Some(now);
                            // Finger up looks up: the game's y turns the view down.
                            let turn = [dx * scale, -dy * scale * invert];
                            self.pending[0] += turn[0];
                            self.pending[1] += turn[1];
                            let at = Instant::now();
                            let span = self
                                .last_motion
                                .map_or(dt, |t| at.duration_since(t).as_secs_f32())
                                .clamp(0.004, 0.05);
                            self.last_motion = Some(at);
                            let speed = [turn[0] / span, turn[1] / span];
                            // Smoothed, so the last report alone doesn't decide a flick.
                            self.velocity = [
                                self.velocity[0] * 0.6 + speed[0] * 0.4,
                                self.velocity[1] * 0.6 + speed[1] * 0.4,
                            ];
                        }
                    }
                }
            } else {
                if self.right_last.take().is_some() {
                    // A thumb that stopped before it lifted, or moved slowly, doesn't coast.
                    let moving = self
                        .last_motion
                        .is_some_and(|t| t.elapsed() < Duration::from_millis(40));
                    let fast = self.velocity.iter().any(|v| v.abs() > 40.0);
                    if !config.trackpad_momentum || !moving || !fast {
                        self.velocity = [0.0; 2];
                    }
                }
                if config.trackpad_momentum && dt > 0.0 {
                    look[0] += self.velocity[0] * dt;
                    look[1] += self.velocity[1] * dt;
                    let decay = (-config.momentum_friction * dt).exp();
                    self.velocity = self.velocity.map(|v| v * decay);
                    if self.velocity.iter().all(|v| v.abs() < 2.0) {
                        self.velocity = [0.0; 2];
                    }
                }
            }
            // The trackpad's turn comes out smoothed: a part of what's waiting each report.
            let out = 1.0 - config.trackpad_smoothing;
            look[0] += self.pending[0] * out;
            look[1] += self.pending[1] * out;
            self.pending = self.pending.map(|p| {
                let rest = p * config.trackpad_smoothing;
                if rest.abs() < 0.001 { 0.0 } else { rest }
            });
            let gyro_on = match config.gyro {
                Gyro::Off => false,
                Gyro::Always => true,
                Gyro::Touch => touching,
            };
            if gyro_on && dt > 0.0 {
                // x: pitch rate (nose up positive), z: yaw rate (turning left positive).
                let pitch = f32::from(self.gyro[0]) * GYRO_DPS;
                let yaw = f32::from(self.gyro[2]) * GYRO_DPS;
                // Below half a degree a second is drift, not aim.
                let live = |v: f32| if v.abs() < 0.5 { 0.0 } else { v };
                let ix = if config.gyro_invert_x { -1.0 } else { 1.0 };
                let iy = if config.gyro_invert_y { -1.0 } else { 1.0 };
                look[0] += -live(yaw) * dt * config.gyro_sensitivity * ix;
                look[1] += -live(pitch) * dt * config.gyro_sensitivity * iy;
            }
        }
    }

    fn matches(info: &hidapi::DeviceInfo) -> bool {
        if info.vendor_id() != VALVE || !matches!(info.product_id(), WIRED | DONGLE) {
            return false;
        }
        // The vendor collection carries the controller reports; the others are lizard mode's
        // keyboard and mouse. Backends that don't say the usage page go by interface: the
        // wired controller's is 2, the dongle's controllers are 1 to 4.
        match info.usage_page() {
            0xff00 => true,
            0 => match info.product_id() {
                WIRED => info.interface_number() == 2,
                _ => (1..=4).contains(&info.interface_number()),
            },
            _ => false,
        }
    }

    /// What the Alt-Tab chord did this round.
    enum Chord {
        Idle,
        /// Its modifier went down: a fresh hold.
        Started,
        /// A window step was sent during this hold.
        Used,
    }

    /// Alt-Tab from the controller: hold the modifier (the Steam or View button), click the left
    /// pad right or left to step forward or back through the windows (Alt stays held, as on a
    /// keyboard); letting go of the modifier lets go of Alt, which switches.
    #[derive(Default)]
    struct AltTab {
        alt_down: bool,
        held: bool,
        dir: i8,
    }

    impl AltTab {
        fn step(&mut self, pad: Option<&Pad>, config: &Config) -> Chord {
            let modifier = match config.alt_tab.as_str() {
                "steam" => STEAM,
                "back" => BACK,
                _ => 0,
            };
            let held = modifier != 0 && pad.is_some_and(|p| p.buttons & modifier != 0);
            let dir = pad
                .filter(|p| p.buttons & LEFT_PAD_CLICK != 0)
                .map_or(0, |p| match p.left_pad[0] {
                    x if x > 12000 => 1,
                    x if x < -12000 => -1,
                    _ => 0,
                });
            let started = held && !self.held;
            self.held = held;
            let mut chord = if started { Chord::Started } else { Chord::Idle };
            if held && dir != 0 && dir != self.dir {
                if !self.alt_down {
                    keys::press(keys::ALT, true);
                    self.alt_down = true;
                }
                if dir < 0 {
                    keys::press(keys::SHIFT, true);
                }
                keys::press(keys::TAB, true);
                keys::press(keys::TAB, false);
                if dir < 0 {
                    keys::press(keys::SHIFT, false);
                }
                chord = Chord::Used;
            }
            self.dir = if held { dir } else { 0 };
            if !held {
                self.release();
            }
            chord
        }

        fn release(&mut self) {
            if self.alt_down {
                keys::press(keys::ALT, false);
                self.alt_down = false;
            }
        }
    }

    /// Key presses sent to the system, as a keyboard's (Windows' SendInput).
    mod keys {
        pub(super) const ALT: u16 = 0x12;
        pub(super) const SHIFT: u16 = 0x10;
        pub(super) const TAB: u16 = 0x09;

        #[cfg(windows)]
        pub(super) fn press(vk: u16, down: bool) {
            #[repr(C)]
            #[derive(Clone, Copy)]
            struct MouseInput {
                dx: i32,
                dy: i32,
                data: u32,
                flags: u32,
                time: u32,
                extra: usize,
            }
            #[repr(C)]
            #[derive(Clone, Copy)]
            struct KeybdInput {
                vk: u16,
                scan: u16,
                flags: u32,
                time: u32,
                extra: usize,
            }
            #[repr(C)]
            union Payload {
                mouse: MouseInput,
                key: KeybdInput,
            }
            #[repr(C)]
            struct Input {
                kind: u32,
                payload: Payload,
            }
            #[link(name = "user32")]
            unsafe extern "system" {
                fn SendInput(count: u32, inputs: *const Input, size: i32) -> u32;
            }
            const INPUT_KEYBOARD: u32 = 1;
            const KEYEVENTF_KEYUP: u32 = 0x2;
            let input = Input {
                kind: INPUT_KEYBOARD,
                payload: Payload {
                    key: KeybdInput {
                        vk,
                        scan: 0,
                        flags: if down { 0 } else { KEYEVENTF_KEYUP },
                        time: 0,
                        extra: 0,
                    },
                },
            };
            // SAFETY: one INPUT of the size Windows expects, alive for the call.
            let sent = unsafe { SendInput(1, &input, std::mem::size_of::<Input>() as i32) };
            if sent != 1 {
                log::debug!("Steam Controller: SendInput failed for key {vk:#x}");
            }
        }

        #[cfg(not(windows))]
        pub(super) fn press(_vk: u16, _down: bool) {}
    }

    pub(super) fn run() {
        let mut api = match HidApi::new() {
            Ok(api) => api,
            Err(e) => {
                log::warn!("Steam Controller: HID is unavailable: {e}");
                return;
            }
        };
        watch_steam();
        let mut pads: Vec<Pad> = Vec::new();
        let mut active: Option<usize> = None;
        let mut last_scan = Instant::now() - Duration::from_secs(10);
        let mut last_config_check = Instant::now();
        let mut config_seen = config_mtime();
        let mut last_steam_check = Instant::now() - Duration::from_secs(10);
        let mut steam = false;
        let mut unfocused_since: Option<Instant> = None;
        let mut alt_tab = AltTab::default();
        let mut buf = [0u8; 65];

        loop {
            let now = Instant::now();
            let (config, focused, playing, closing) = match shared().lock() {
                Ok(s) => (s.config.clone(), s.focused, s.playing, s.closing),
                Err(_) => return,
            };

            if closing {
                alt_tab.release();
                if !steam {
                    for pad in &mut pads {
                        pad.restore();
                    }
                }
                if let Ok(mut s) = shared().lock() {
                    s.active = None;
                    s.closing = false;
                }
                return;
            }

            if now.duration_since(last_config_check) > Duration::from_secs(2) {
                last_config_check = now;
                let seen = config_mtime();
                if seen != config_seen {
                    config_seen = seen;
                    let fresh = load_config();
                    log::info!("Steam Controller: settings re-read");
                    if let Ok(mut s) = shared().lock() {
                        s.config = fresh;
                    }
                    config_seen = config_mtime();
                    // The gyro setting goes to the controller.
                    for pad in &mut pads {
                        pad.configured = false;
                        pad.config_tried = false;
                    }
                    continue;
                }
            }

            // Asked of a thread of its own: the check takes ~130 ms (a process list), which here
            // would freeze the controller's input that long every few seconds.
            if now.duration_since(last_steam_check) > Duration::from_millis(250) {
                last_steam_check = now;
                let running = STEAM_RUNNING.load(std::sync::atomic::Ordering::Relaxed);
                if running != steam {
                    steam = running;
                    log::info!(
                        "Steam Controller: Steam is {}",
                        if steam { "running: leaving the controller to it" } else { "closed: reading the controller" }
                    );
                }
            }
            let wanted = match config.mode {
                Mode::On => true,
                Mode::Off => false,
                Mode::Auto => !steam,
            };
            if !wanted {
                if !steam {
                    for pad in &mut pads {
                        pad.restore();
                    }
                }
                pads.clear();
                active = None;
                if let Ok(mut s) = shared().lock() {
                    s.active = None;
                    s.rumbles.clear();
                }
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }

            // New controllers (plugged in, dongle inserted).
            if now.duration_since(last_scan) > Duration::from_secs(2) {
                last_scan = now;
                if api.refresh_devices().is_ok() {
                    let found: Vec<(CString, u16)> = api
                        .device_list()
                        .filter(|info| matches(info))
                        .map(|info| (info.path().to_owned(), info.product_id()))
                        .collect();
                    for (path, product) in found {
                        if pads.iter().any(|p| p.path == path) {
                            continue;
                        }
                        match api.open_path(&path) {
                            Ok(dev) => {
                                log::info!(
                                    "Steam Controller: opened {}",
                                    if product == WIRED { "wired controller" } else { "wireless dongle slot" }
                                );
                                pads.push(Pad {
                                    dev,
                                    path,
                                    product,
                                    configured: false,
                                    configured_gyro: false,
                                    config_tried: false,
                                    last_config: now,
                                    // A dongle slot is configured from the start too: a
                                    // controller in lizard mode sends no reports through it
                                    // (only keys and mouse) until lizard mode is off, so
                                    // waiting for one would wait for ever.
                                    present: true,
                                    buttons: 0,
                                    triggers: [0; 2],
                                    stick: [0; 2],
                                    left_pad: [0; 2],
                                    right_pad: [0; 2],
                                    gyro: [0; 3],
                                    right_last: None,
                                    velocity: [0.0; 2],
                                    touch_reports: 0,
                                    pending: [0.0; 2],
                                    last_motion: None,
                                    last_report: None,
                                    last_touch: None,
                                    last_edge: None,
                                });
                            }
                            Err(e) => log::debug!("Steam Controller: open failed: {e}"),
                        }
                    }
                }
            }

            // Lizard mode follows focus: off while the game has it, back on a second after
            // it's lost so the controller drives the desktop again. Not while an Alt-Tab is
            // being chosen: the controller has to keep reporting until it's let go.
            let hold = if focused {
                unfocused_since = None;
                true
            } else {
                let since = *unfocused_since.get_or_insert(now);
                now.duration_since(since) < Duration::from_secs(1) || alt_tab.alt_down
            };
            let want_gyro = config.gyro != Gyro::Off;
            for pad in &mut pads {
                if !pad.present {
                    continue;
                }
                if hold {
                    // Sent when needed, not on a timer: each is a blocking USB request, and the
                    // dongle has four slots. Again when a slot that was reporting has gone quiet
                    // (a controller that slept or reconnected forgets it); a failed send (an
                    // empty slot) is retried only every few seconds.
                    let since = now.duration_since(pad.last_config);
                    let quiet = pad
                        .last_report
                        .is_some_and(|t| now.duration_since(t) > Duration::from_secs(2));
                    let due = since > Duration::from_secs(5);
                    if (!pad.configured && (!pad.config_tried || due))
                        || pad.configured_gyro != want_gyro
                        || (quiet && due)
                    {
                        pad.configure(want_gyro);
                    }
                } else if !steam {
                    pad.restore();
                }
            }

            // Read everything waiting.
            let look_live = hold && playing && focused;
            let mut look = [0.0f32; 2];
            let mut gone = Vec::new();
            for (i, pad) in pads.iter_mut().enumerate() {
                loop {
                    match pad.dev.read_timeout(&mut buf, 0) {
                        Ok(0) => break,
                        Ok(n) => pad.ingest(&buf[..n], &config, look_live, &mut look),
                        Err(e) => {
                            log::info!("Steam Controller: disconnected ({e})");
                            gone.push(i);
                            break;
                        }
                    }
                }
            }
            for i in gone.into_iter().rev() {
                pads.remove(i);
                active = None;
            }
            // The controller touched last is the one playing.
            if let Some((i, _)) = pads
                .iter()
                .enumerate()
                .filter(|(_, p)| p.present && p.configured)
                .filter_map(|(i, p)| p.last_touch.map(|t| (i, t)))
                .max_by_key(|(_, t)| *t)
            {
                active = Some(i);
            }
            if active.is_some_and(|i| !pads.get(i).is_some_and(|p| p.present && p.configured)) {
                active = None;
            }

            let current = active.and_then(|i| pads.get(i));
            let busy = current.is_some_and(|p| {
                p.buttons & RIGHT_PAD_TOUCH != 0 || p.velocity != [0.0; 2]
            });
            let moving = current.is_some_and(|p| {
                p.buttons & LEFT_PAD_TOUCH != 0
                    || p.stick.iter().any(|v| v.unsigned_abs() > 4000)
            });
            let edge = pads.iter().filter_map(|p| p.last_edge).max();
            let chord = alt_tab.step(current, &config);
            let rumbles = match shared().lock() {
                Ok(mut s) => {
                    match chord {
                        Chord::Started => s.chord_used = false,
                        Chord::Used => s.chord_used = true,
                        Chord::Idle => {}
                    }
                    let now = Instant::now();
                    if busy {
                        s.right_busy = Some(now);
                    }
                    if moving {
                        s.motion_busy = Some(now);
                    }
                    s.edge = edge;
                    s.steam = steam;
                    s.look[0] += look[0];
                    s.look[1] += look[1];
                    s.active = active.map(|i| pads[i].snapshot());
                    std::mem::take(&mut s.rumbles)
                }
                Err(_) => return,
            };
            if let Some(pad) = active.and_then(|i| pads.get(i)) {
                for (strong, weak, millis) in rumbles {
                    let k = config.haptic_strength;
                    pad.pulse(1, strong * k, millis);
                    pad.pulse(0, weak * k, millis);
                }
            }

            // The controller reports every few milliseconds: wait for the next report of the one
            // playing (else the one that reported last), so it is read the moment it comes; any
            // other's are read at the top of the next round.
            let wait_on = active
                .or_else(|| {
                    pads.iter()
                        .enumerate()
                        .filter_map(|(i, p)| p.last_report.map(|t| (i, t)))
                        .max_by_key(|(_, t)| *t)
                        .map(|(i, _)| i)
                })
                .or_else(|| pads.iter().position(|p| p.present));
            match wait_on.and_then(|i| pads.get_mut(i)) {
                Some(pad) => {
                    if let Ok(n) = pad.dev.read_timeout(&mut buf, 4)
                        && n > 0
                    {
                        let mut look = [0.0f32; 2];
                        pad.ingest(&buf[..n], &config, look_live, &mut look);
                        if let Ok(mut s) = shared().lock() {
                            s.look[0] += look[0];
                            s.look[1] += look[1];
                        }
                    }
                }
                None => std::thread::sleep(Duration::from_millis(if pads.is_empty() { 50 } else { 8 })),
            }
        }
    }
}
