# Hell Hound

> **AI notice:** this project is developed with AI assistance under the owner's direction and review. This implementation was written with ChatGPT (OpenAI) via OpenCode and [Claude](https://claude.ai) (Anthropic).

An original **0BSD Rust CLI** for Call of Duty assets, primarily World at War, targeting Windows and Linux. The goal is complete weapons usable in Blender—not just texture dumping.

## Start with the running game

```sh
cargo build --release --locked --bins
./target/release/hell-hound
```

On Windows run `target/release/hell-hound.exe`. With no arguments, Hell Hound waits for a recognized CoD process and prompts you to select it, like Greyhound's game-selection workflow. It finds the installation's IWDs without asking you for archive filenames. No game executable is launched by discovery.

The terminal browser offers:

- Image, sound and other archive-file search by name substring/glob.
- Numbered selection and extraction (`1,3-5` or `all`) into a chosen folder.
- Loaded WaW model and animation searches.
- **Experimental weapon export:** search weapon/hands models and animation clips, select them by number/range, and write a single Blender-importable `.glb`. No copying exact names is necessary in the browser; batch commands/recipes can still use exact names.

`r` refreshes/chooses another game, `q` quits, Ctrl+C cancels waiting. If an installation cannot be located, give `open --game-dir DIRECTORY`, not an IWD filename. Interactive mode needs a terminal; automation should use the subcommands below.

## What is actually supported?

**Implemented and fixture-tested:**

- Native process discovery on Linux (`/proc`, including Wine/Proton paths) and Windows (Toolhelp/process image queries).
- ZIP/IWD listing/extraction, exact names/globs, safe folder routing and archive manifest locks.
- WaW read-only process access, validated stock SP/MP model/animation pools and metadata.
- Independent WaW model decoding: highest-detail LOD, geometry, normals, UVs, rigid/blended weights (up to four influences), bone hierarchy/bind pose and material/image references.
- Independent animation decoding: identity/constant/keyed 2D/3D quaternions, byte/ushort quantized translations, static translations, short/long frame indices and notification metadata.
- IWI v6/v8 texture conversion: supported uncompressed BGRA/BGR/alpha and BC1/BC2/BC3 formats to embedded PNG.
- GLB export with skinned geometry, named rig, animation clips, source event/FPS/loop metadata and diffuse textures. Repeated `--model` merges compatible weapon/hands rigs.

**Still experimental:** a real ray gun viewmodel export imported with correct geometry and rig; nothing else has been checked on real game data. The live layouts, permission boundaries, complete real weapon/hands assembly and animation-space conventions need validation on actual WaW. Synthetic fixtures and a Blender import test do not prove every real asset works.

**Explicit limitations:** delta/root-motion animation channels, unknown/other build layouts, other IWI formats (including BC5), cubemaps and `.ff` parsing are rejected. CoD packed normal/specular source images are preserved in the GLB with metadata, but not guessed into a PBR shader. Some viewmodel/gun-relative behavior may need correction after real-asset comparison. Missing rig bones fail unless explicitly allowed; incomplete output is not silently labeled successful.

Recognized legacy executable names include WaW, CoD/UO, CoD2, CoD4, MW2/MW3, Black Ops and Black Ops II. **Recognition is not decoder compatibility:** only WaW has a live decoder. Other games can use archive extraction only if ZIP-based IWDs exist; their FF/live formats are not automatically supported.

## Commands without IWD filenames

After `cargo install --path . --locked`, use `hell-hound` directly, or substitute `cargo run --bin hell-hound --`:

```sh
hell-hound games --json
hell-hound games --wait --timeout 60
hell-hound open --game waw
hell-hound list --game waw --kind image --match "*colt*"
hell-hound list --game waw --kind model --match "*colt*"
hell-hound list --game waw --kind animation --match "viewmodel_*colt*"
hell-hound extract --game waw --kind sound --match "*colt*" --output output/colt
```

With more than one matching game, the terminal prompts for a choice. `--pid PID` selects explicitly; a single candidate can be selected automatically in noninteractive commands. `--wait` listens for startup; `--timeout SECONDS` bounds waiting (0 means unlimited). Prompts go to stderr so `--json` stdout stays machine-readable. Plain `games` is a one-shot scan and returns an empty list if none are found.

Linux may deny reading an unrelated Proton process. The extractor reports this; it does not change ptrace settings, request root, inject code or write game memory. Process discovery can work even when live memory access is denied.

If native Linux memory access is denied, an **unverified alternative** is the Windows CLI build inside the game's existing Proton environment, using Protontricks rather than a separate Wine prefix:

```sh
flatpak run --command=protontricks-launch com.github.Matoking.protontricks \
  --appid 10090 "/absolute/path/to/hell-hound.exe" open --game waw
```

Flatpak must have access to the executable's folder. This uses the Windows process-reading backend through Wine, not native `/proc` access; whether it fixes this setup's process visibility/reading must still be tested. If the launcher does not attach an interactive console, use batch `list`/`weapon` commands instead of `open`. It is not permission to disable host security. Build the helper with `cargo build --release --locked --target x86_64-pc-windows-gnu --bins` when that target/toolchain is installed; binaries are then under `target/x86_64-pc-windows-gnu/release/`.

## Export a weapon and its animations

First load a stable WaW map and discover the **actual names** through the browser or `list` commands. Do not assume weapon, animation and hands names are identical. Example placeholders below must be replaced:

```sh
hell-hound weapon --game waw \
  --model "EXACT_WEAPON_MODEL" --model "EXACT_HANDS_MODEL" \
  --animation "EXACT_IDLE_ANIMATION" --animation "EXACT_FIRE_ANIMATION" \
  --animation "EXACT_RELOAD_ANIMATION" --output output/weapon.glb
```

For exploration, `--anim-match "viewmodel_*colt*"` selects matching loaded clips; a match containing unsupported delta data fails rather than discarding that data. Exact names are better for repeatable jobs. `--no-textures` isolates mesh/animation decoding from image issues. `--allow-missing-bones` explicitly permits omitted tracks, and records omitted names in the GLB; normally include the required hands model instead. Incompatible shared bind poses/parents fail rather than corrupting the combined rig. `--overwrite` is required to replace an existing GLB.

Import through Blender **File → Import → glTF 2.0**. The file embeds buffers, textures, skin and clips. CoD Z-up inches are baked into the data as glTF Y-up metres, so the armature keeps unit scale. In Blender the axes match the game's, with Z up. Notetracks and looping/FPS are stored as custom properties; nothing in them runs. [`docs/BLENDER.md`](docs/BLENDER.md) explains what the import contains, how to play clips and the importer's `glTF_not_exported` icosphere.

`live-list` remains available for detailed diagnostics, with `--mode sp|mp` and `--image-base 0x...` overrides. Linux discovery attempts to find the mapped image base; the stock default is `0x400000` when unavailable. Unknown images/pools fail validation. Keep the map stable during reads; pointer rechecks are not a transactional snapshot of a running engine.

## TOML jobs

Recipes are data, never arbitrary code; unknown keys are rejected. Paths are relative to the recipe's directory. Archive jobs can identify the running game directly:

```toml
version = 1
game = "waw"
output = "../output/colt-images"

[[extract]]
include = ["images/*colt*"]
kind = "image"
into = "weapons/colt"
```

Use exactly one of `game`, `game_dir` or advanced `sources`. `names` requires every exact member; `include` supplies globs; `exclude` and `kind` filter afterwards; `required = false` permits missing optional assets. `into` preserves original member paths below the chosen subfolder. Names/globs are case-sensitive on both platforms and `*` matches `/` too.

For decoded weapons, edit `examples/weapon.toml`:

```toml
version = 1
game = "waw"
output = "../output/weapon.glb"

[weapon]
models = ["EXACT_WEAPON_MODEL", "EXACT_HANDS_MODEL"]
animations = ["EXACT_IDLE_ANIMATION", "EXACT_FIRE_ANIMATION"]
no_textures = false
allow_missing_bones = false
```

```sh
hell-hound run examples/waw.toml --dry-run
hell-hound run examples/weapon.toml --dry-run
hell-hound run examples/weapon.toml
```

Weapon jobs use exact names (or optional `animation_patterns`) and reject missing assets; `--dry-run` decodes/validates but writes nothing. They do not yet support archive JSON locks. Runtime asset data/build changes can alter exported bytes even when names stay the same.

## Advanced archive control and locked replay

Explicit source paths remain available for offline extraction and precise patch ordering:

```sh
hell-hound list --source "/path/to/WaW/main/*.iwd" --match "images/*colt*"
hell-hound extract --game-dir "/path/to/WaW" \
  --name "images/colt45_c.iwi" --output output --into weapons/colt
hell-hound run examples/waw.toml --lock output/waw/hell-hound-manifest.json
```

`images/colt45_c.iwi` was found locally; use names from your own installation. Source globs expand lexically in argument order; later archives override earlier exact member paths. Repeat `--source` to control precedence. Discovery loads root IWDs then `main/` IWDs, not arbitrary mod directories.

Raw archive extraction preserves bytes; it does not automatically convert IWI. Its deterministic `hell-hound-manifest.json` records names, destinations, archive filenames/hashes and extracted SHA-256 values. `--lock` requires an exact match before writing; preserve a copy for long-term replay. Identical extraction is safe; changed files/manifests need a different destination or `--overwrite`. No stale files are deleted. Each file is atomic, not the entire batch: I/O failure may leave partial output.

Traversal, drive paths, Windows device aliases, symlink members/output paths, collisions and source/output overlap are rejected. The default raw selected-asset limit is 256 MiB (`--max-bytes`); there is no aggregate output quota. Use trusted output directories, keep sources unchanged and review unfamiliar archives before extraction.

## Build, tests and Blender verification

```sh
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
bash tools/check-blender.sh
```

The optional last command needs Blender on PATH. It exports a wholly synthetic skinned/animated/textured fixture, imports it headlessly and checks rig, weights, material image and bone motion. Its Python file uses Blender's Python API solely as a test harness; the extractor is Rust. Fixtures ship no proprietary game data. Windows cross-compilation is checked locally, but native Windows execution still needs testing.

## WaW zombies launcher

A Steam launcher for stock solo zombies (`waw-zombies.py`) is in development. It is kept out of this repository until it works.

## Licensing and research

Original Rust code is 0BSD. The owner explicitly chose an independent implementation, **not a GPL Greyhound code port**. Adjacent Greyhound includes GPLv3 binaries/package data, not source. Hell Hound does not execute or incorporate those binaries. Public source was consulted for format/layout facts and capability boundaries; this is not a formal clean-room claim. See `docs/WAW_LAYOUT.md` for provenance, assumptions and validation limits.

Neither game ownership nor an extractor's software license grants redistribution rights to extracted assets. Only use data you are entitled to access. No copyrighted game assets are distributed here.
