# GameSpice Sim on the web

**Status: works, being shipped (decided 2026-11-13).** The real dot-sim extension, with all its modules, loads and runs in a Godot 4.7.2 web export built from the single-threaded dlink template. Its native paths are 3 to 7 times faster than the script twins in the browser, and their bytes equal the script's on wasm too. The script path stays as the complete fallback: every module has a script twin that produces the same bytes, and a web export without the side module runs it.

## Build

| Piece | Version |
| --- | --- |
| Engine | 4.7.2-stable, official export templates (they ship `web_dlink_*` and `web_dlink_nothreads_*`; no source build needed) |
| Template emscripten | 4.0.20 (the engine's own console line) |
| emsdk for the side module | 3.1.74 |
| rustc | nightly-2026-01-10 (godot 0.5.5 needs 1.94 or newer) |
| godot crate | 0.5.5, features `api-custom`, `experimental-wasm`, `lazy-function-tables`, `experimental-wasm-nothreads` |

`ci/web.sh` is the build. It is opt-in and needs emsdk activated, `GODOT4_BIN` set and the pinned nightly. It writes `addons/dot_sim/bin/web-wasm32/dot_sim.wasm` and a `.gdextension` with a `web.wasm32` line, and both are gitignored. It builds from `rust/` because cargo reads `rust/.cargo/config.toml` only from the working directory. Built with `--manifest-path` from the root, the side-module flags are lost and the link fails on `undefined symbol: main`. The config holds these flags under `[target.wasm32-unknown-emscripten]`:

- `-sSIDE_MODULE=2`
- emscripten C++ exceptions off
- hidden default visibility
- no native libraries
- `-Z emscripten-wasm-eh=false`

The export preset ticks **Extensions Support**, sets **Thread Support** off and uses the dlink nothreads template. Godot's release web templates take no scene path on the command line, so the tests below ran through a main scene that loads the scene named in `location.hash`.

## Measured (2026-11-13, PH, headless Chromium with SwiftShader; staging02 was down)

| Suite on the dlink export | Result |
| --- | --- |
| `sim_selftest` (bots, brain parity, sight) | 9 passed, 0 failed |
| `snapshot_selftest` (wire parity, codec) | 34 passed, 0 failed (33/1 before the `to_int` fix below) |

| Bench (same box, runs interleaved) | Script, plain export | Script, dlink export | Native, dlink export |
| --- | --- | --- | --- |
| sight scan, us | 56.5 / 60.3 / 62.0 | 61.2 / 63.2 / 65.6 | 8.0 (7.4x faster) |
| snapshot encode, 128 entities, ms | 13.04 / 13.33 | 11.26 / 10.71 | 2.95 / 2.91 (about 4x) |
| snapshot decode+apply, 128 entities, ms | 13.58 / 14.01 | 12.57 / 11.63 | 3.92 / 3.78 (about 3.3x) |

The dlink engine is built position-independent. On the sight scan this made the script path about 6% slower than on the plain engine. On the snapshot bench the dlink script runs were faster than the plain ones. So for the script path, the cost of the dlink engine is noise-sized either way.

| File | Raw bytes | gzip -9 bytes |
| --- | --- | --- |
| plain `index.wasm` | 39,514,754 | 10,084,297 |
| dlink `index.wasm` (main module) | 1,508,095 | 582,004 |
| dlink `index.side.wasm` (engine) | 44,078,870 | 10,486,172 |
| `dot_sim.wasm` (all modules) | 699,755 | 225,807 |

The dlink export with GameSpice Sim is about 1.2 MB larger after gzip than the plain export (about +12%).

## Blockers before shipping

1. **dot-net gated the codec off on the web.** `DotNetSimCodec.available()` checked `not OS.has_feature("web")`; dot-net now checks only `ClassDB.class_exists` (the games link dot-net's checkout, so the one change reaches all of them).
2. **Encode parity on wasm: fixed.** GDScript's `int(float)` is the platform's conversion. On x86-64 (`cvttsd2si`) a NaN or out-of-range value is `i64::MIN`; under Emscripten (`trunc_sat`) a NaN is 0 and the rest saturates, measured in Chromium: `int(nan)` 0, `int(1e300)` `i64::MAX`, `int(-1e300)` `i64::MIN`. `to_int` (dot-sim-snapshot) hard-coded the x86 answer, so a NaN or 1e300 in an unsigned field encoded differently (13 of the probe's edge cases). It now follows the target: Rust's saturating `as` on wasm, the x86 emulation elsewhere. Native x86 output is unchanged.
3. **The FC web export has to change.** It needs the dlink nothreads template with Extensions Support, and the CDN must serve `index.side.wasm` and `dot_sim.wasm`. A plain template with any `.gdextension` in the pack refuses to start. That includes godot-sqlite's, which has web entries.
4. **Release asset and CI: done.** `ci/targets.tsv` has a `web-wasm32` row, and the `web` jobs in `ci.yml` and `release.yml` build it with `ci/web.sh` (the pinned nightly with `rust-src` and the emscripten target, emsdk 3.1.74). Every tag attaches `dot_sim-X.Y.Z-web-wasm32.zip` beside the desktop zips, and dot-server-deploy's `tools/fetch-natives.sh --platform web-wasm32` installs it like the others.

## Threads (`ci/web.sh --threads`, 2026-11-16)

The threaded dlink template (`web_dlink_*`, Thread Support on) needs its own side module: its memory is shared, and a module built single-threaded imports unshared memory, so it cannot link. `ci/web.sh --threads` builds one with the same flags plus `-C target-feature=+atomics,+bulk-memory,+mutable-globals` and `-pthread`, without `experimental-wasm-nothreads`, into its own target dir (`rust/target/web-threads`). It is installed as `bin/web-wasm32-threads/dot_sim.threads.wasm` with a `web.threads.wasm32` line (`ci/targets.tsv` row `web-wasm32-threads`; CI and release build both web rows, so every tag attaches `dot_sim-X.Y.Z-web-wasm32-threads.zip` too).

One `.gdextension` with both lines serves both engines: Godot picks the library whose tags match most, so the threaded engine (feature `threads`) takes `web.threads.wasm32` and the single-threaded one keeps `web.wasm32`. An export copies only the file it picked beside the page (checked: `gdextensionLibs: ["dot_sim.threads.wasm"]` on the threaded export, `["dot_sim.wasm"]` on the other, the same pck). The file name differs because the engine loads a side module by its base name.

Measured on PH, headless Chromium with SwiftShader, served with COOP/COEP (`crossOriginIsolated` true), both builds from the same tree:

| | Threaded dlink, `dot_sim.threads.wasm` | Single-threaded dlink, `dot_sim.wasm` |
| --- | --- | --- |
| side module, raw bytes | 1,047,316 | 1,053,690 |
| memory import | `env.memory`, shared | `env.memory`, not shared |
| `sim_selftest` | 14 passed, 3 failed | 13 passed, 3 failed (the same 3) |
| sight scan, script / native, us | 65.20 / 9.84 (6.6x) | 63.32 / 8.23 (7.7x) |

The 3 failures come from the older test project used for this run (LOD ray-count and think-tier probes the current suite changed), and fail the same on both engines; nothing failed on threads alone. GameSpice Sim spawns no threads of its own, so the threaded build buys nothing inside GameSpice Sim: it exists so a game that needs the threaded engine (FC's `bot_threads`) keeps native GameSpice Sim instead of dropping to the script path.

## Rules

- **A plain-template web export never carries `addons/dot_sim/dot_sim.gdextension`.** With the plain template, that file alone stops the game's scripts (`GDExtension libraries are not supported by this engine version`). Only a dlink export may carry it, and the `web.wasm32` line appears only when `bin/web-wasm32/dot_sim.wasm` is installed (by `ci/web.sh` or the release zip).
- Every native class is reached through `ClassDB.class_exists` (see `DotSim.has`), never by name in a typed script, so the scripts compile where it is absent.
- The newest nightly breaks the build. It rejects `-Z emscripten-wasm-eh=false`, and without that flag the page fails with `LinkError: ... "__cpp_exception": tag import requires a WebAssembly.Tag`. Move `NIGHTLY` in `ci/web.sh` only after a browser try.
- `ci/targets.tsv` has exactly two web rows, `web-wasm32` and `web-wasm32-threads` (`ci/check.sh` enforces that). `ci/install.sh` refuses to build them and only lays them out; `ci/web.sh` (`--threads` for the second) is the build.
- A threaded engine never gets the single-threaded module, nor the other way round: a `.gdextension` with only a `web.wasm32` line on a threaded export loads nothing that works. Ship the threads build or ship no threaded engine (GameSpice Server Deploy's export-web checks this).

## Not tested

- Firefox and Safari
- the threaded build under a real FC 64-player client (only the selftests and the sight bench ran)
- a GPU browser (SwiftShader only)
- the motor (GameSpice Player Controller) on the web
- a full FC client
