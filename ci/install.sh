#!/bin/bash
# Build the native library for one platform and install it where the addon loads it.
#   ci/install.sh [--platform P] [--profile release|debug] [--no-build] [--gdextension-only]
# P is a row of ci/targets.tsv (default: this machine's). Writes
#   addons/gs_sim/bin/<platform>/<library file>
#   addons/gs_sim/gs_sim.gdextension      (every platform whose library is present)
# Both are gitignored: the build is an artifact, never a commit. Without the library the
# addon runs its GDScript twins, so a missing or failed build is never fatal to a game.
# --no-build installs a library already built under rust/target. Cross builds need the rust
# target installed (rustup target add); the workflows do that. Exit 3: no rust/ crate yet.
# macos-universal is one fat dylib: it builds both Apple targets and joins them with lipo (macOS only; the
# row's rust target column lists both, comma separated). The .gdextension then points macos.universal and,
# where no per-arch library is installed, macos.arm64 and macos.x86_64 at it.
# --gdextension-only rewrites the .gdextension from what is installed and builds nothing (ci/web.sh).
# The cargo step is the only heavy one: run this under `heavy` (or on a box with cargo).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
PLATFORM=""; PROFILE=release; BUILD=1; INSTALL=1
while [ $# -gt 0 ]; do
    case $1 in
        --platform) PLATFORM="${2:?}"; shift 2 ;;
        --profile) PROFILE="${2:?}"; shift 2 ;;
        --no-build) BUILD=0; shift ;;
        --gdextension-only) BUILD=0; INSTALL=0; shift ;;
        *) echo "install.sh: unknown argument '$1'" >&2; exit 2 ;;
    esac
done
case $PROFILE in release|debug) ;; *) echo "install.sh: profile must be release or debug" >&2; exit 2 ;; esac

host_platform() {
    local os arch
    os="$(uname -s)"; arch="$(uname -m)"
    case "$os/$arch" in
        Linux/x86_64) echo linux-x86_64 ;;
        Linux/aarch64|Linux/arm64) echo linux-arm64 ;;
        Darwin/arm64) echo macos-arm64 ;;
        Darwin/x86_64) echo macos-x86_64 ;;
        MINGW*/*|MSYS*/*|CYGWIN*/*) echo windows-x86_64 ;;
        *) echo "install.sh: no platform for $os/$arch" >&2; return 1 ;;
    esac
}

# The library's file stem: [lib] name if set, else the package name, dashes to underscores.
# In a workspace (rust/Cargo.toml has [workspace]) it is read from the crate building the cdylib.
crate_name() {
    local n m=rust/Cargo.toml
    if grep -q '^\[workspace\]' "$m"; then
        m="$(grep -l 'cdylib' rust/crates/*/Cargo.toml | head -n 1)"
        [ -n "$m" ] || { echo "install.sh: no cdylib crate under rust/crates" >&2; return 1; }
    fi
    n="$(awk '/^\[/{s=$0} s=="[lib]" && $1=="name"{gsub(/[" ]/,"",$3); print $3; exit}' "$m")"
    [ -n "$n" ] || n="$(awk '/^\[/{s=$0} s=="[package]" && $1=="name"{gsub(/[" ]/,"",$3); print $3; exit}' "$m")"
    [ -n "$n" ] || { echo "install.sh: no crate name in rust/Cargo.toml" >&2; return 1; }
    echo "${n//-/_}"
}

[ -f rust/Cargo.toml ] || { echo "install.sh: rust/Cargo.toml not found; nothing to build (the addon runs on GDScript)" >&2; exit 3; }
CRATE="$(crate_name)"
if [ "$INSTALL" = 1 ]; then
    [ -n "$PLATFORM" ] || PLATFORM="$(host_platform)"
    R="$(awk -F'\t' -v p="$PLATFORM" '$0!~/^#/ && $1==p {print; exit}' ci/targets.tsv)"
    [ -n "$R" ] || { echo "install.sh: '$PLATFORM' is not in ci/targets.tsv" >&2; exit 2; }
    IFS=$'\t' read -r _ TARGET _ PATTERN _ <<<"$R"
    # The web side module needs the pinned nightly, -Zbuild-std and emsdk: ci/web.sh builds it, then installs
    # it through here with --no-build.
    case $PLATFORM in web-*) [ "$BUILD" = 0 ] || { echo "install.sh: build $PLATFORM with ci/web.sh" >&2; exit 2; } ;; esac
    FILE="${PATTERN//\{crate\}/$CRATE}"

    if [ "$BUILD" = 1 ]; then
        for t in ${TARGET//,/ }; do
            flags=(--manifest-path rust/Cargo.toml --target "$t" --locked)
            [ "$PROFILE" = release ] && flags+=(--release)
            cargo build "${flags[@]}"
        done
    fi
    SRC="rust/target/$TARGET/$PROFILE/$FILE"
    if [ "$PLATFORM" = macos-universal ]; then
        # lipo is a macOS tool (llvm-lipo also works); fail loudly rather than install one architecture as "universal".
        LIPO="$(command -v lipo || command -v llvm-lipo || true)"
        [ -n "$LIPO" ] || { echo "install.sh: macos-universal needs lipo (macOS) or llvm-lipo" >&2; exit 1; }
        SRC="rust/target/universal-apple-darwin/$PROFILE/$FILE"
        PARTS=()
        for t in ${TARGET//,/ }; do
            [ -f "rust/target/$t/$PROFILE/$FILE" ] || { echo "install.sh: rust/target/$t/$PROFILE/$FILE not found (build it, or drop --no-build)" >&2; exit 1; }
            PARTS+=("rust/target/$t/$PROFILE/$FILE")
        done
        mkdir -p "$(dirname "$SRC")"
        "$LIPO" -create "${PARTS[@]}" -output "$SRC"
    fi
    # The threaded side module is built in its own target dir and comes out under the crate's plain name.
    [ "$PLATFORM" != web-wasm32-threads ] || SRC="rust/target/web-threads/$TARGET/$PROFILE/$CRATE.wasm"
    [ -f "$SRC" ] || { echo "install.sh: $SRC not found (build it, or drop --no-build)" >&2; exit 1; }
    DEST="addons/gs_sim/bin/$PLATFORM"
    mkdir -p "$DEST"
    cp "$SRC" "$DEST/$FILE"
fi

# Regenerate the .gdextension from what is installed, so a partial install is still valid.
OUT=addons/gs_sim/gs_sim.gdextension
{
    echo "[configuration]"
    echo "entry_symbol = \"gdext_rust_init\""
    echo "compatibility_minimum = 4.1"
    echo "reloadable = false"
    echo
    echo "[libraries]"
    awk -F'\t' '$0!~/^#/ && NF>=5 {print $1"\t"$4"\t"$5}' ci/targets.tsv | while IFS=$'\t' read -r plat pat key; do
        f="${pat//\{crate\}/$CRATE}"
        if [ -f "addons/gs_sim/bin/$plat/$f" ]; then echo "$key = \"res://addons/gs_sim/bin/$plat/$f\""; fi
    done
    # A universal library also serves each Apple architecture that has no library of its own.
    uf="lib$CRATE.dylib"
    if [ -f "addons/gs_sim/bin/macos-universal/$uf" ]; then
        for a in arm64 x86_64; do
            [ -f "addons/gs_sim/bin/macos-$a/$uf" ] || echo "macos.$a = \"res://addons/gs_sim/bin/macos-universal/$uf\""
        done
    fi
} > "$OUT"
[ "$INSTALL" = 1 ] && echo "installed $DEST/$FILE" || echo "wrote $OUT"
