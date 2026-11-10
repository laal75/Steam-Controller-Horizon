# Listing on the Horizon mod page

Everything the registry's submission form asks for, ready to paste:

| Field | Value |
|---|---|
| Form | https://playhorizon.dev/mods/submit (sign in with GitHub) |
| Kind | `native-mod` |
| Id | `steam_controller` |
| File | [`dist/steam_controller.hnmod`](../dist/steam_controller.hnmod) |
| Signature | [`dist/steam_controller.hnmod.minisig`](../dist/steam_controller.hnmod.minisig) |
| Public key | the second line of [`minisign.pub`](../minisign.pub) |
| Icon | [`icon.png`](icon.png) (256x256) |
| Categories | `utility`, `game-mechanics` |
| Tags | `controller`, `steam-controller`, `gamepad`, `trackpad` |
| Name, summary, description, authors, licence | [`horizon-registry.toml`](../horizon-registry.toml) |
| Source | https://github.com/laal75/Steam-Controller-Horizon |

A native mod is read by a reviewer before it's listed; later versions are checked by machine.

## Signing a new version

The secret key is `%USERPROFILE%\.horizon-mods\minisign.key` on the author's PC (never in this repository; keep a
backup, since a lost key means asking the registry to change the project's key). After `python tools/pack.py`:

```sh
cargo run --release --manifest-path tools/sign/Cargo.toml -- dist/steam_controller.hnmod
```

It writes `dist/steam_controller.hnmod.minisig` and checks it the way the launcher does.
