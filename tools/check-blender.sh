#!/usr/bin/env bash
# Optional integration test; requires Blender on PATH. No game data is used.
set -euo pipefail
here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
temporary_root="${TMPDIR:-/tmp/opencode}"
[[ -d "$temporary_root" ]] || temporary_root="${TMPDIR:-/tmp}"
work="$(mktemp -d "$temporary_root/hell-hound-blender.XXXXXX")"
HELL_HOUND_TEST_GLB="$work/weapon.glb" cargo test --manifest-path "$here/Cargo.toml" --locked --test weapon full_weapon_export_is_valid_skinned_animated_textured_glb -- --exact
blender --background --factory-startup --disable-autoexec --python-exit-code 1 --python "$here/tools/verify_blender.py" -- "$work/weapon.glb"
