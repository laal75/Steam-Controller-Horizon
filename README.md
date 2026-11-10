# Steam Controller for Horizon

Makes the original Steam Controller (2015) a full Horizon controller, with or without Steam open.

Without Steam, the controller stays in "lizard mode": the right pad moves the mouse, the triggers click and the buttons type keys, and games don't see it as a gamepad. This mod reads it directly over HID:

- **Lizard mode off** while the game window has focus. It comes back on a second after you alt-tab away, and again when the game closes, so the controller drives the desktop again.
- **Buttons, triggers and stick** act as a normal pad with an Xbox layout. Your bindings in Controls > Controller apply as usual.
- **Left trackpad** is the d-pad: 8 directions, pressed by clicking (or by touch, if you set that).
- **Right trackpad** is a trackball for the camera, with optional momentum after a flick. In menus it scrolls like a right stick.
- **Gyro aiming** is optional: always on, or only while your thumb rests on the right pad.
- **Rumble** plays on the pads' haptics.
- **Grips, the right pad click and the Steam button** can each press any pad button. By default the left grip is A (jump, so your right thumb stays on the trackpad), the right grip is LS (sprint) and the right pad click is RS (sneak).
- **Other controllers keep working.** The Steam Controller takes over while you use it and hands back 3 seconds after you stop.

Wired (USB) and the wireless dongle both work.

## Easy install

The ready-made pack is in the [`dist`](dist) folder, so there's nothing to clone:

| File | What it is | Where it goes in the game folder |
|---|---|---|
| [`steam_controller.hnmod`](dist/steam_controller.hnmod) | The mod (native code) | `horizon-native-mods/` |
| [`steam_controller.hnmod.minisig`](dist/steam_controller.hnmod.minisig) | Its signature | not needed to play |

1. Download `steam_controller.hnmod`. The game folder of the main Horizon instance is `%APPDATA%\horizon` on
   Windows; `horizon-launcher instance list` shows your instances.
2. Copy it into the game folder's `horizon-native-mods/`.
3. Turn it on in the game folder's `horizon-mods.txt`:

   ```text
   mod.steam_controller:true
   ```

4. Build the code into the instance. This also rebuilds your other installed native mods, so they stay in the
   game. The first build downloads a Rust toolchain and takes 20 to 30 minutes; later ones take a few minutes.

   ```sh
   horizon-launcher native-build --instance <instance id> --trust-native-code --allow-version-mismatch
   ```

5. Start the game with the controller on. Settings: Options > Mods > Steam Controller > Settings.

The pack is signed with the key in [`minisign.pub`](minisign.pub) (minisign format):
`minisign -Vm steam_controller.hnmod -P <the key's second line>` checks it.

## Controls (Controlify's layout)

| Input | Action |
|---|---|
| A | Jump |
| B | Back in menus (nothing in game) |
| X | Swap hands |
| Y | Inventory |
| LT / RT | Use / Attack |
| LB / RB | Previous / next hotbar slot |
| Stick, click | Move, sprint |
| Right pad, click | Look, sneak |
| Left pad (click) up / down / left | Chat / drop / pick block |
| View / Start | Perspective / pause |
| Left / right grip | Jump / sprint |
| Steam button | Show or hide the on-screen controls |

The first start of 0.3.0 sets the game's controller bindings to this layout once (`controlify_layout = pending`). After that, rebinding in Controls > Controller sticks.

## Alt-Tab

Hold the Steam button and click the left pad right (or left) to step forward (or back) through your windows, as Alt-Tab does; let go of the Steam button to switch. Alt stays held while the Steam button is, so several clicks move several windows. A short press of the Steam button on its own still shows or hides the on-screen controls.

`alt_tab = back` uses the View button instead (perspective then changes when View is let go), `off` turns it off. It works on Windows. Once you've switched away, the controller goes back to its desktop mouse after a second.

## On-screen controls

While the Steam Controller is in use, the left-hand inputs are listed bottom left and the right-hand ones bottom right, as they are bound. The Steam button shows or hides them; `overlay = false` starts with them hidden.

## Block highlight

`block_highlight = fill` (the default) tints the targeted block's faces instead of outlining it, in `block_highlight_colour`. `outline` gives back the game's own outline.

## Steam running

Like Controlify (SDL's HIDAPI Steam driver), the mod reads the controller even while Steam is open. Windows lets both read it.

Steam's Desktop layout also turns the controller into keys and mouse input: A becomes Enter, B becomes Space, the stick and pads move the mouse. With `filter_steam_input = true` (the default), the game drops keys and clicks that arrive together with a controller button, and mouse motion and scrolling while the stick or a pad is in use. A real keyboard or mouse used at the same moment can lose an input.

The cleanest setup is still to keep Steam out: add Horizon to Steam as a non-Steam game, set its Controller option to "Disable Steam Input", and start it from Steam. `mode = auto` only reads the controller while Steam is closed. While Steam runs, the mod never switches lizard mode back on.

## Settings

Options > Mods > Steam Controller > Settings has every setting below: each button steps its setting to the next value and saves it at once.

The same settings are in `config/steam_controller.txt` in the instance folder. It is written with defaults on first launch, and changes apply within 2 seconds, no restart needed.

| Setting | Default | |
|---|---|---|
| `mode` | `on` | `on` (even with Steam open), `auto` (only while Steam is closed), `off` |
| `left_pad` | `click` | `click`, `touch`, `off` |
| `trackpad_sensitivity` | `220` | degrees for a swipe across the whole pad |
| `trackpad_momentum` | `true` | keep turning after a flick |
| `momentum_friction` | `5` | higher stops sooner |
| `trackpad_invert_y` | `false` | |
| `trackpad_deadzone` | `300` | pad units the thumb moves before the view turns (the pad is 65536 wide) |
| `trackpad_smoothing` | `0.35` | 0 to 0.95, higher is smoother |
| `gyro` | `off` | `off`, `touch`, `always` |
| `gyro_sensitivity` | `1.5` | 1.0 = the view turns as far as the controller |
| `gyro_invert_x` / `gyro_invert_y` | `false` | |
| `haptics` | `true` | vibration must also be on in the game's controller settings |
| `haptic_strength` | `1.0` | 0 to 3 |
| `overlay` | `true` | on-screen controls |
| `block_highlight` | `fill` | `fill` or `outline` |
| `block_highlight_colour` | `white` | `white red orange yellow green cyan blue magenta black inverted` |
| `filter_steam_input` | `true` | drop Steam's Desktop-layout echo |
| `alt_tab` | `steam` | `steam` (hold Steam + left pad right/left), `back` (hold View), `off` |
| `controlify_layout` | `pending` | `pending` sets the game's bindings to Controlify's once, then `done` |
| `left_grip`, `right_grip`, `right_pad_click`, `left_pad_center`, `steam_button` | `south`, `ls`, `rs`, `none`, `overlay` | `south east west north lb rb lt rt select start ls rs up down left right none` |

The game's own controller settings (invert X/Y, vibration, the button bindings) apply on top.

## Linux

The controller's hidraw device needs read/write access for your user. Steam's udev rules (`60-steam-input.rules`, from the `steam-devices` package) grant it.

## Build

Native mod (mixins), pinned to Horizon `fdc87ae`:

```
horizon-launcher native-build native --instance <id> --trust-native-code
```

## License

MIT.
