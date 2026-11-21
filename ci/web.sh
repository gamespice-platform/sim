#!/bin/bash
# Build the web side module (docs/web.md) and install it where a dlink web export loads it.
#   ci/web.sh [--threads] [--no-build]   then: ci/package.sh --version X --platform web-wasm32[-threads]
# Default: the single-threaded module, addons/gs_sim/bin/web-wasm32/gs_sim.wasm, `web.wasm32` in the
# .gdextension, for the dlink nothreads template (Extensions Support on, Thread Support off).
# --threads: the pthread module (atomics, shared memory) for the dlink threads template,
# addons/gs_sim/bin/web-wasm32-threads/gs_sim.threads.wasm, `web.threads.wasm32`. The file name differs
# because the engine dlopens a side module by its base name. Both may be installed at once: the threaded
# engine picks the `threads` line (more feature tags matched), the single-threaded one the plain line.
# All of it is gitignored. The plain web template refuses to start with any .gdextension in the pack.
# Needs: emsdk activated (emcc on PATH; 3.1.74 is the tested one), the pinned nightly with rust-src and
# the wasm32-unknown-emscripten target (NIGHTLY, default below), and GODOT4_BIN (godot-rust's
# api-custom dumps the API from the engine). Run under `heavy`: -Zbuild-std rebuilds std.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
# Newer nightlies reject -Z emscripten-wasm-eh=false (docs/web.md); move only after a browser try.
NIGHTLY="${NIGHTLY:-nightly-2026-01-10}"
TARGET=wasm32-unknown-emscripten
BUILD=1 THREADS=0
while [ $# -gt 0 ]; do
    case $1 in
        --no-build) BUILD=0; shift ;;
        --threads) THREADS=1; shift ;;
        *) echo "web.sh: unknown argument '$1'" >&2; exit 2 ;;
    esac
done
[ -f rust/Cargo.toml ] || { echo "web.sh: rust/Cargo.toml not found" >&2; exit 3; }

if [ "$BUILD" = 1 ]; then
    command -v emcc >/dev/null || { echo "web.sh: emcc not on PATH (source emsdk_env.sh)" >&2; exit 2; }
    [ -n "${GODOT4_BIN:-}" ] || { echo "web.sh: set GODOT4_BIN to a Godot 4.7 binary" >&2; exit 2; }
    rustup toolchain list | grep -q "^$NIGHTLY" ||
        { echo "web.sh: first run: rustup toolchain install $NIGHTLY -c rust-src -t $TARGET --profile minimal" >&2; exit 2; }
    # From rust/, not --manifest-path: cargo reads rust/.cargo/config.toml (the side-module flags)
    # only from the working directory, and without them the link fails on `undefined symbol: main`.
    if [ "$THREADS" = 1 ]; then
        # Its own target dir (std is rebuilt with atomics, so the two must not share one). The extra rustflags
        # add to the side-module ones in rust/.cargo/config.toml; -pthread makes the module import shared memory.
        (cd rust && CARGO_TARGET_DIR=target/web-threads cargo "+$NIGHTLY" build --locked --release -p gs-sim-gdext \
            --features web -Zbuild-std --target "$TARGET" \
            --config 'target.wasm32-unknown-emscripten.rustflags=["-C","target-feature=+atomics,+bulk-memory,+mutable-globals","-C","link-args=-pthread"]')
    else
        (cd rust && cargo "+$NIGHTLY" build --locked --release -p gs-sim-gdext --features web-nothreads \
            -Zbuild-std --target "$TARGET")
    fi
fi
# The web rows of ci/targets.tsv: install.sh copies the built gs_sim.wasm into addons/gs_sim/bin/<platform>/
# under the row's file name and lists it in the .gdextension with whatever else is installed.
if [ "$THREADS" = 1 ]; then
    bash ci/install.sh --platform web-wasm32-threads --no-build
else
    bash ci/install.sh --platform web-wasm32 --no-build
fi
