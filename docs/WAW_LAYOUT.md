# WaW format research and validation status

This is an original implementation based on researched binary-layout facts, not a Greyhound source port or formal clean-room effort. Greyhound's implementation is GPLv3 and is not included. Layout evidence currently comes from its public [WaW handler](https://github.com/Scobalula/Greyhound/blob/22c3d17f7428573042b56f43af0ed2585989bebd/src/WraithXCOD/WraithXCOD/GameWorldAtWar.cpp) and [structure definitions](https://github.com/Scobalula/Greyhound/blob/22c3d17f7428573042b56f43af0ed2585989bebd/src/WraithXCOD/WraithXCOD/DBGameAssets.h). Real-build verification remains required.

## Initial metadata inspector

All values are little-endian. WaW pointers are 32-bit, even when Wine/Proton/the extractor runs on a 64-bit OS. The initial implementation validates a PE/x86 image and known placeholder model name before interpreting tables. Table virtual addresses below assume an image base of `0x400000`; loaded data pointers are already process addresses and must NOT be relocated again.

| Mode | Asset pool pointer table | Asset pool sizes |
| --- | --- | --- |
| SP | `0x8dc828` | `0x8dc5d0` |
| MP | `0x8d0958` | `0x8d06e8` |

Indices 4 and 5 select animations and models respectively. Each table entry is four bytes. A pool starts with a free-list head pointer, followed by fixed-stride slots; unallocated slots contain links into the same pool. Researched strides: animation 88 bytes, model 228 bytes. Known initial model names: `void`, `defaultactor`, `defaultweapon`.

Metadata offsets within slots:

| Field | Offset | Representation |
| --- | --- | --- |
| Name pointer (both) | 0 | u32 |
| Model bone count | 4 | u8 |
| Animation frame count | 14 | u16 |
| Animation bone count | 27 | u8 |
| Animation frame rate | 40 | f32 |

Limits: 50,000 slots per pool, 1024 bytes per name, 16 MiB per native memory read; reject address overflow, missing reads, invalid frame rates and unknown sentinels. The game is not suspended: map changes can invalidate a traversal, and the final pointer check is not a transactional snapshot. Exporters will need stronger consistency checks and placeholder resolution.

## Implemented experimental weapon pipeline

The original Rust implementation writes **glTF 2.0 binary (.glb)**, with embedded buffers and PNG images. Blender 4.0.2 successfully imported the synthetic fixture with rig, weighted geometry, material image and a moving animated gun bone. An actual owned WaW Colt diffuse IWI also converted successfully to a 512x512 PNG. A real `viewmodel_usa_ray_gun` export decoded 8 surfaces, each rigidly bound to the bone it sits on (batteries on `tag_battery_*`, the gear on `j_gear`), which confirms the surface, rigid-list and model-space vertex layout above. Real animation, hands merging and texture output remain unverified.

Model layout facts used by the independent decoder:

- Header: bone IDs pointer +8, relative-parent table +12, global bind transforms +28, surfaces +32, material handles +36, four LOD records at +40 (28-byte stride), LOD count u16 at +196.
- Each bind transform is quaternion XYZW f32, translation XYZ f32, then one extra f32 (32-byte stride). Bone names use u16 script-string IDs, with inline strings at table + ID*12 + 4 (SP table `0x3702400`, MP `0xf66b400`, relocated with the image).
- Surface stride 64: vertex/face counts u16 at +2/+4, triangle pointer +12, four blend counts +16, blend stream +24, vertex stream +28, rigid-list count +36 and pointer +40.
- Vertex stride 32: XYZ f32 at +0, half-float UV components at +20/+22 (WaW order swapped), packed normal at +24. Triangles use three u16 indices. Bone IDs in weight streams are encoded as index*64; additional weights are u16 fractions with denominator 65536. Up to four influences are supported. Rigid lists use a 12-byte stride.
- Materials retain named diffuse/normal/specular image references. Highest-detail LOD is exported, not every LOD overlaid. Shared skeleton names are merged only when parent and bind pose agree.

Animation decoding covers none/constant/keyed planar and full quaternion rotation groups, byte and ushort min/scale translations, static translations, one/two-byte frame indices and long/inline frame tables. Signed quaternion components are normalized after decoding. Events, FPS and loop metadata are retained in GLB extras. Delta/root-motion data is explicitly rejected, not dropped. The current viewmodel convention treats tracks as absolute local transforms except relative `j_gun`/`j_gun1`; non-viewmodel tracks are interpreted bind-relative. **These source-space assumptions need comparison against actual clips.**

Texture decoding supports IWI v6/v8 and the implemented uncompressed/BC1/BC2/BC3 formats, validates dimensions and the full-resolution mip bounds, and embeds PNGs. Cubemaps/unknown formats fail. Packed normal/specular source channels are not silently interpreted as standard PBR maps; only diffuse is automatically wired in the glTF material.

## Sound aliases and weapon definitions (measured, single player)

Measured on the owner's Steam build in Der Riese by reading pools and comparing them with the IWDs' text weapon files (`weapons/sp/*`), not taken from other tools. Only single player (zombies) is decoded; MP fails explicitly.

- Pool indices in the asset table: 9 sound alias lists, 10 loaded sounds, 24 weapon definitions (4 animations and 5 models as before).
- Alias list stride 12: name pointer, variants pointer, variant count. Variant stride 184; its first field repeats the list's name pointer (checked), its sound-file pointer is at +20.
- Sound file: type byte at +0 (1 loaded, 2 streamed, 3 laid out as streamed: four music cues). Loaded: +4 points at a 12-byte record (name, data, size) whose data is a whole RIFF file, `WAVE` (often MS ADPCM) or `XWMA`. Streamed: folder name at +8 (may be empty), file name at +12, under `sound/` in the IWDs with backslashes.
- Weapon definition stride `0x9ac`: name +0, display name +4, gun model (model pointer) +0xc, hand model +0x4c; animation name strings from +0x54 (idle) to +0xd8 (ADS down) and sound alias pointers from +0x184 (pickup) to +0x264 (putaway player), in the order of `weapon/def.rs`; weapon type +0x144 (0 bullet, 1 grenade, 2 projectile, 3 binoculars, 4 gas), fire type +0x158 (0 Full Auto, 1 Single Shot, bursts after); start ammo +0x3ec, max ammo +0x404, clip size +0x408, shot count +0x40c, damage +0x420, and times in milliseconds: fire +0x448, reload +0x464, empty reload +0x46c, reload add +0x470, reload start +0x478, reload start add +0x47c, reload end +0x480, drop +0x484, raise +0x488.
- Viewmodel rigs: a gun model's root `j_gun` hangs on the hands model's `tag_weapon`; the export attaches it (bind pose and gun vertices moved there).

## Required real-weapon validation

1. Load a stable stock WaW map and verify detected PID, image base, pool layout and asset names. Do not change ptrace policy or privileges automatically if Linux denies access.
2. Export an actual weapon plus hands with idle/fire/reload animations. Check the merged hierarchy/bind pose and relative versus absolute track behavior. An explicit missing-bone override is diagnostic, not proof of a complete weapon.
3. Import in Blender; verify geometry, UVs/materials, skin deformation, FPS, loop duration and gun-bone motion against the game. Render transparent frames as the downstream Doom sprite proof.
4. Add fixtures/evidence for newly encountered variants, including root motion, placeholder aliases and packed shader channels. Do not distribute proprietary game data as test fixtures.
