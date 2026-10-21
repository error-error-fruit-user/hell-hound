#!/usr/bin/env bash
set -euo pipefail
here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
if [[ -x "$here/target/release/waw-zombies" ]]; then
    exec "$here/target/release/waw-zombies" "$@"
fi
exec cargo run --quiet --manifest-path "$here/Cargo.toml" --bin waw-zombies -- "$@"
