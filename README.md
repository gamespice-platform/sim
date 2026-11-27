Optional native (Rust GDExtension) versions of hot simulation code from the GameSpice addons. Each module gives the same answers as the script it replaces. A game without the library, and every web export, simply runs the script.

This is a GameSpice addon (MIT, built with Claude Code). `examples/sim_selftest.tscn` is the headless suite, and `cargo test` in `rust/` covers the kernels. The games in [Zee's Games](https://zeesgames.com) run on GameSpice addons and live servers, and they load this library where a build ships it. Bug reports go to the [issue tracker](https://github.com/gamespice-platform/sim/issues).

| Module | Replaces |
| --- | --- |
| bots | The sight scan of GameSpice Bot's brain |

## Install

1. Run `ci/install.sh` (needs a stable Rust toolchain). It builds this machine's library into `addons/gs_sim/bin/<platform>/` and writes `addons/gs_sim/gs_sim.gdextension`. Or unpack a release's `gs_sim-<version>-<platform>.zip` at the project root.
2. Open or import the project once so the engine lists the extension.

## Use

Nothing to call. `GSSim.has(&"bots")` tells whether a module is loaded, and GameSpice Bot uses it on its own when present.

Keep `addons/gs_sim/bin/` and `gs_sim.gdextension` out of plain-template web exports. The opt-in web build is `ci/web.sh`; see [docs/web.md](docs/web.md).

## Testing

Link the sibling addons (`addons/gs_core`, `addons/gs_bot`), install a library, then run `examples/sim_selftest.tscn` headless with Godot. Its arms section also needs gs-player-char's scan linked (`examples/linked/gs_view_arms_clear.gd`, skipped when absent), and its weapon suite (`examples/sim_weapon_selftest.tscn`) gs-weapon's addon (`addons/gs_weapon`, skipped when absent). Without a library the suite checks the fallback and says the native half was skipped. `cargo test` in `rust/` tests the kernels alone.

Design and numbers: [docs/design.md](docs/design.md). Writing a native twin and its parity suite for any addon: [GameSpice Core native-twins](../gs-core/docs/native-twins.md).

## Used by

### Zee's Games

Zee's Games is a collection of games I made on GameSpice, for demonstration and for fun. They show what the platform can do, and they run on live servers you can join. Anybody can create games on GameSpice, and each addon is its own repository and can be used separately from the games and from the rest of GameSpice.

These games vendor this addon:

- [Fatal Contact](https://zeesgames.com/fatal-contact)
- [Airdrift](https://zeesgames.com/airdrift)
- [Playlot](https://zeesgames.com/playlot)
- [Hungario](https://zeesgames.com/hungario)
- [Buses From Hell](https://zeesgames.com/buses-from-hell)
- [Smash Copter](https://zeesgames.com/smash-copter)
- [Zombie Zone](https://zeesgames.com/zombie-zone)
- [Hide n Seek](https://zeesgames.com/hide-n-seek)
- [Prop Hunt](https://zeesgames.com/prop-hunt)
- [Murder Mystery](https://zeesgames.com/murder-mystery)
- [Deathrun](https://zeesgames.com/deathrun)
- [Wipeout](https://zeesgames.com/wipeout)
- [Dangerous Delivery](https://zeesgames.com/dangerous-delivery)
- [Look At Me](https://zeesgames.com/look-at-me)

The whole collection is at [zeesgames.com](https://zeesgames.com).

## Licence

MIT licence, see [LICENSE](LICENSE). The only closed part of the [**GameSpice**](https://gamespice.net) platform is the web back end; you can use your own instead of GameSpice's.
