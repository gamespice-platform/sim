#!/bin/bash
# Offline checks for the build matrix; needs no toolchain, so it runs anywhere (PH included).
#   ci/check.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; cd "$ROOT"
fail=0; bad() { echo "check: $*" >&2; fail=1; }
for f in ci/*.sh; do bash -n "$f" || bad "$f does not parse"; done
awk -F'\t' '$0!~/^#/ && NF!=5 {exit 1}' ci/targets.tsv || bad "targets.tsv: every row needs 5 tab-separated columns"
dups="$(awk -F'\t' '$0!~/^#/{print $1}' ci/targets.tsv | sort | uniq -d)"
[ -z "$dups" ] || bad "targets.tsv: duplicate platform $dups"
# The web rows are the dlink side modules, single-threaded and pthread, and ci/web.sh builds them (docs/web.md).
# The threaded file needs its own base name: the engine dlopens a side module by base name.
[ "$(awk -F'\t' '$0!~/^#/ && $1 ~ /^web/' ci/targets.tsv)" = "$(printf 'web-wasm32\twasm32-unknown-emscripten\tubuntu-22.04\t{crate}.wasm\tweb.wasm32\nweb-wasm32-threads\twasm32-unknown-emscripten\tubuntu-22.04\t{crate}.threads.wasm\tweb.threads.wasm32')" ] ||
    bad "targets.tsv: the web rows must be web-wasm32 ({crate}.wasm, web.wasm32) and web-wasm32-threads ({crate}.threads.wasm, web.threads.wasm32)"
grep -q 'emscripten-wasm-eh=false' rust/.cargo/config.toml 2>/dev/null || bad "rust/.cargo/config.toml lost the side-module flags"
for wf in .github/workflows/ci.yml .github/workflows/release.yml; do
    for p in $(awk -F'\t' '$0!~/^#/{print $1}' ci/targets.tsv); do
        grep -q "platform: $p\$" "$wf" || bad "$wf: matrix has no row for $p"
    done
done
exit $fail
