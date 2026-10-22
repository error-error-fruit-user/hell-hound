# Using exported weapons in Blender

Checked against Blender 4.0.2 with the default glTF importer settings.

## Import

**File → Import → glTF 2.0**, pick the `.glb`, keep the defaults. Everything (geometry, rig, textures, clips) is embedded in that one file.

## What you get

| Outliner entry | What it is |
| --- | --- |
| `Hell Hound weapon` (armature) | The merged rig of every exported model, named with the game's bone names (`j_gun`, `tag_flash`, ...). |
| One mesh object per model | e.g. `viewmodel_usa_ray_gun`, plus the hands model if you added one. Each game surface is a material slot on it. |
| `glTF_not_exported` collection with an `Icosphere` | Made by Blender's importer, not by Hell Hound: it is the custom shape used to draw bones. It is hidden and is never exported. You can leave it or delete it. |
| Actions | One per exported clip. The first one is applied on import. |

Units are metres, and Blender's axes match the game's: X forward, Y left, Z up. A viewmodel is about 0.3–0.5 m long and sits near the origin, so press **Home** (View All) or numpad **.** (View Selected) if it looks off-screen.

## Animations

Before importing, set the scene frame rate to the clip's rate (**Output Properties → Frame Rate**; WaW clips are usually 30 fps). Blender converts clip time to the scene rate when it imports, so at its default 24 fps the game's frames fall between Blender frames.

Each clip becomes an action named `<clip>_Hell Hound weapon`. To switch clips, select the armature, open **Dope Sheet → Action Editor** and pick one from the action menu. The scene's frame range is not changed, so set the end frame to the clip's length.

Blender drops per-animation metadata, so Hell Hound also writes it to the scene. **Scene Properties → Custom Properties → `hellHoundClips`** lists each clip's `fps`, `frames`, `looping`, notetrack `events` (name and time in seconds), `omittedBones` and `sourceSpace`. Blender shows it as one line of text. It is data only; nothing runs.

Viewmodel clips need the hands model as well as the gun. If the picked models lack bones a clip needs, the interface asks whether to leave them out (the batch commands fail unless you pass `--allow-missing-bones`); left-out bones are listed under `omittedBones`.

## Materials and textures

Only the diffuse texture is connected, as Base Color. The normal and specular images are embedded in the file but not connected, because CoD packs its channels differently from Blender's shader. Each material lists its source image names in its custom properties (`hellHoundSourceImages`). Names starting with `$` are made by the engine and do not exist in the game files, so they are never exported.

Blender's default **Solid** view draws every material in one flat colour, textures or not. To see them, switch the viewport to **Material Preview** (`Z`, then Material Preview, or the third shading button at the top right), or in Solid view open the shading popover and set **Color** to **Texture**.

If materials are plain grey even in Material Preview, textures were skipped (Textures set to skip in the interface, a missing texture you chose to go on without, or `--no-textures`).

## Known gaps

- Checked on real game data (Der Riese, 2026-10-22): weapon and hands merged with the gun hung on `tag_weapon`, idle, fire, reload, raise and aim clips, and diffuse textures, by rendering every Der Riese weapon from the player's eye with doomify-waw.
- Viewmodel clips are layered as in the game: they never move `tag_torso`, which the ADS clips do. Played alone, a clip shows the weapon as aimed (centred); from the hip, `tag_torso` is where `ADS_down` ends.
- Delta/root-motion clips are rejected.
- Only the highest-detail LOD is exported.

See the README for commands and `docs/WAW_LAYOUT.md` for how the data is decoded.
