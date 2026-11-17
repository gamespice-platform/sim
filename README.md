Optional native (Rust GDExtension) versions of hot simulation code from the Gamespice addons. Each module gives the same answers as the script it replaces. A game without the library, and every web export, simply runs the script.

| Module | Replaces |
| --- | --- |
| bots | The sight scan of dot-bot's brain |

## Install

1. Run `ci/install.sh` (needs a stable Rust toolchain). It builds this machine's library into `addons/dot_sim/bin/<platform>/` and writes `addons/dot_sim/dot_sim.gdextension`. Or unpack a release's `dot_sim-<version>-<platform>.zip` at the project root.
2. Open or import the project once so the engine lists the extension.

## Use

Nothing to call. `DotSim.has(&"bots")` tells whether a module is loaded, and dot-bot uses it on its own when present.

Keep `addons/dot_sim/bin/` and `dot_sim.gdextension` out of plain-template web exports. The opt-in web build is `ci/web.sh`; see [docs/web.md](docs/web.md).

## Testing

Link the sibling addons (`addons/dot_core`, `addons/dot_bot`), install a library, then run `examples/sim_selftest.tscn` headless with Godot. Its arms section also needs dot-player-char's scan linked (`examples/linked/dot_view_arms_clear.gd`, skipped when absent), and its weapon suite (`examples/sim_weapon_selftest.tscn`) dot-weapon's addon (`addons/dot_weapon`, skipped when absent). Without a library the suite checks the fallback and says the native half was skipped. `cargo test` in `rust/` tests the kernels alone.

Design and numbers: [docs/design.md](docs/design.md).

## Status and licence

Part of the [Gamespice](https://gamespice.net) platform: a family of Godot addons. Written with Claude Code and maintained by `gamemann`. Every addon has headless tests, but little of this has been played by real users yet, so expect rough edges and please [open an issue](https://github.com/gamespice-platform/sim/issues) when you hit one.

MIT licence, see [LICENSE](LICENSE). The only closed part of the [Gamespice](https://gamespice.net) platform is the web back end; you can use your own instead of Gamespice's.
