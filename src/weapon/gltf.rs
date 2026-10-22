//! glTF 2.0 binary writer. All buffers/images are embedded in one atomic GLB.
use super::{Ask, Problem, allow, animation::Clip, model::Bundle, texture};
use crate::archive::{self, Asset};
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec3};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Binary {
    bytes: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}
impl Binary {
    fn raw(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while !self.bytes.len().is_multiple_of(4) {
            self.bytes.push(0);
        }
        let offset = self.bytes.len();
        self.bytes.extend_from_slice(bytes);
        let mut view = json!({"buffer":0,"byteOffset":offset,"byteLength":bytes.len()});
        if let Some(target) = target {
            view["target"] = json!(target);
        }
        let index = self.views.len();
        self.views.push(view);
        index
    }
    fn data(
        &mut self,
        bytes: &[u8],
        count: usize,
        component: u32,
        kind: &str,
        target: Option<u32>,
    ) -> usize {
        let view = self.raw(bytes, target);
        let index = self.accessors.len();
        self.accessors
            .push(json!({"bufferView":view,"componentType":component,"count":count,"type":kind}));
        index
    }
    fn floats(&mut self, values: &[f32], width: usize, kind: &str, target: Option<u32>) -> usize {
        let bytes: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.data(&bytes, values.len() / width, 5126, kind, target)
    }
    fn shorts(&mut self, values: &[u16], width: usize, kind: &str, target: Option<u32>) -> usize {
        let bytes: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.data(&bytes, values.len() / width, 5123, kind, target)
    }
    fn times(&mut self, frames: &[u16], fps: f32) -> usize {
        let values: Vec<_> = frames.iter().map(|f| *f as f32 / fps).collect();
        let accessor = self.floats(&values, 1, "SCALAR", None);
        self.accessors[accessor]["min"] = json!([values[0]]);
        self.accessors[accessor]["max"] = json!([values[values.len() - 1]]);
        accessor
    }
}

// CoD is Z-up inches; glTF is Y-up metres. The conversion is baked into every
// position, bind pose and key: a scaled parent node becomes Blender's armature
// object scale, and the importer then draws bones 1/0.0254 times too long,
// hiding the weapon behind them.
const METRES_PER_INCH: f32 = 0.0254;
fn axis() -> Quat {
    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
}
fn point(value: Vec3) -> Vec3 {
    axis() * value * METRES_PER_INCH
}
fn rotation(value: Quat) -> Quat {
    (axis() * value * axis().conjugate()).normalize()
}

fn append_duration<T: Clone>(keys: &[(u16, T)], frames: u16) -> Vec<(u16, T)> {
    let mut result = keys.to_vec();
    if let Some((frame, value)) = result.last().cloned()
        && frame < frames
    {
        result.push((frames, value));
    }
    result
}

pub fn encode(
    bundle: &Bundle,
    clips: &[Clip],
    archives: &[Asset],
    no_textures: bool,
    ask: &mut Ask,
) -> Result<Vec<u8>> {
    ensure!(
        !bundle.bones.is_empty() && !bundle.meshes.is_empty(),
        "Cannot export an empty weapon"
    );
    ensure!(
        bundle.bones.len() < 65536
            && bundle
                .meshes
                .iter()
                .map(|m| m.vertices.len())
                .sum::<usize>()
                <= 2_000_000,
        "Weapon exceeds export limits"
    );
    let mut binary = Binary::default();
    let globals: Vec<_> = bundle
        .bones
        .iter()
        .map(|b| {
            let (_, r, t) = b.global.to_scale_rotation_translation();
            Mat4::from_rotation_translation(rotation(r), point(t))
        })
        .collect();
    let mut nodes = vec![json!({"name":"Hell Hound weapon"})];
    let mut local_poses = vec![];
    for (index, bone) in bundle.bones.iter().enumerate() {
        if let Some(parent) = bone.parent {
            ensure!(parent < index, "Bone parent must precede child");
        }
        let local = bone
            .parent
            .map_or(globals[index], |p| globals[p].inverse() * globals[index]);
        let (scale, rotation, translation) = local.to_scale_rotation_translation();
        ensure!(
            scale.is_finite() && rotation.is_finite() && translation.is_finite(),
            "Invalid local bind pose"
        );
        local_poses.push((rotation, translation));
        let children: Vec<_> = bundle
            .bones
            .iter()
            .enumerate()
            .filter(|(_, b)| b.parent == Some(index))
            .map(|(i, _)| i + 1)
            .collect();
        let mut node = json!({"name":bone.name,"translation":translation.to_array(),"rotation":rotation.normalize().to_array(),"scale":scale.to_array()});
        if !children.is_empty() {
            node["children"] = json!(children);
        }
        nodes.push(node);
    }
    let mut roots: Vec<_> = bundle
        .bones
        .iter()
        .enumerate()
        .filter(|(_, b)| b.parent.is_none())
        .map(|(i, _)| i + 1)
        .collect();
    let inverse: Vec<_> = globals
        .iter()
        .flat_map(|g| g.inverse().to_cols_array())
        .collect();
    ensure!(
        inverse.iter().all(|v| v.is_finite()),
        "Singular inverse bind matrix"
    );
    let inverse_accessor = binary.floats(&inverse, 16, "MAT4", None);
    let mut meshes = vec![];
    let mut materials = vec![];
    let mut images = vec![];
    let mut textures = vec![];
    let mut image_cache: BTreeMap<String, usize> = BTreeMap::new();
    let mut skipped = BTreeSet::new();
    let mut material_cache: BTreeMap<String, (BTreeMap<String, String>, usize)> = BTreeMap::new();
    // One mesh per source model, one primitive (material slot) per surface,
    // so Blender shows a weapon object and a hands object, not loose surfaces.
    let mut models: Vec<(String, Vec<Value>)> = vec![];
    for mesh in &bundle.meshes {
        let material_index = if let Some((previous, index)) =
            material_cache.get(&mesh.material.name)
        {
            ensure!(
                previous == &mesh.material.images,
                "Conflicting definitions for material {}",
                mesh.material.name
            );
            *index
        } else {
            let mut material = json!({"name":mesh.material.name,"pbrMetallicRoughness":{"metallicFactor":0.0,"roughnessFactor":0.8},
                "extras":{"hellHoundSourceImages":mesh.material.images}});
            let mut mapped = BTreeMap::new();
            if !no_textures {
                for (semantic, name) in &mesh.material.images {
                    // Export colour/normal/specular source images, but only wire
                    // diffuse automatically: CoD packed normal/specular channels
                    // need game-specific shader conversion, not guessed PBR maps.
                    // `$` names are engine-generated (e.g. $identitynormalmap)
                    // and never stored in IWDs; their names stay in the extras.
                    if !["diffuse", "normal", "specular"].contains(&semantic.as_str())
                        || name.starts_with('$')
                    {
                        continue;
                    }
                    let image_index = if let Some(index) = image_cache.get(name) {
                        *index
                    } else {
                        let base = name.strip_prefix("images/").unwrap_or(name);
                        let member = if base.ends_with(".iwi") {
                            format!("images/{base}")
                        } else {
                            format!("images/{base}.iwi")
                        };
                        let Some(asset) = archives.iter().find(|a| a.member == member) else {
                            if !skipped.contains(&member) {
                                allow(ask, Problem::MissingTexture(&member))?;
                                skipped.insert(member);
                            }
                            continue;
                        };
                        let png =
                            texture::iwi_png(&archive::read_asset(asset, archive::DEFAULT_LIMIT)?)
                                .with_context(|| format!("Cannot convert texture {member}"))?;
                        let view = binary.raw(&png, None);
                        let index = images.len();
                        images.push(json!({"name":name,"bufferView":view,"mimeType":"image/png"}));
                        textures.push(json!({"source":index}));
                        image_cache.insert(name.clone(), index);
                        index
                    };
                    mapped.insert(semantic.clone(), image_index);
                    if semantic == "diffuse" {
                        material["pbrMetallicRoughness"]["baseColorTexture"] =
                            json!({"index":image_index});
                    }
                }
            }
            material["extras"]["hellHoundImageIndices"] = json!(mapped);
            let index = materials.len();
            materials.push(material);
            material_cache.insert(
                mesh.material.name.clone(),
                (mesh.material.images.clone(), index),
            );
            index
        };
        ensure!(
            !mesh.vertices.is_empty() && mesh.indices.len() % 3 == 0,
            "Invalid mesh primitive"
        );
        let converted: Vec<_> = mesh.vertices.iter().map(|v| point(v.position)).collect();
        let positions: Vec<_> = converted.iter().flat_map(|p| p.to_array()).collect();
        let normals: Vec<_> = mesh
            .vertices
            .iter()
            .flat_map(|v| (axis() * v.normal).normalize().to_array())
            .collect();
        let uv: Vec<_> = mesh.vertices.iter().flat_map(|v| v.uv.to_array()).collect();
        let joints: Vec<_> = mesh.vertices.iter().flat_map(|v| v.joints).collect();
        let weights: Vec<_> = mesh.vertices.iter().flat_map(|v| v.weights).collect();
        ensure!(
            positions
                .iter()
                .chain(&normals)
                .chain(&uv)
                .chain(&weights)
                .all(|v| v.is_finite()),
            "Non-finite mesh attribute"
        );
        ensure!(
            joints.iter().all(|j| (*j as usize) < bundle.bones.len()),
            "Joint index outside exported skeleton"
        );
        ensure!(
            mesh.indices
                .iter()
                .all(|i| (*i as usize) < mesh.vertices.len()),
            "Mesh index out of bounds"
        );
        let position = binary.floats(&positions, 3, "VEC3", Some(34962));
        let minimum = converted
            .iter()
            .fold(Vec3::splat(f32::INFINITY), |a, p| a.min(*p));
        let maximum = converted
            .iter()
            .fold(Vec3::splat(f32::NEG_INFINITY), |a, p| a.max(*p));
        binary.accessors[position]["min"] = json!(minimum.to_array());
        binary.accessors[position]["max"] = json!(maximum.to_array());
        let normal = binary.floats(&normals, 3, "VEC3", Some(34962));
        let uv = binary.floats(&uv, 2, "VEC2", Some(34962));
        let joints = binary.shorts(&joints, 4, "VEC4", Some(34962));
        let weights = binary.floats(&weights, 4, "VEC4", Some(34962));
        // CoD (Direct3D) faces are clockwise, glTF's counter-clockwise: as written, every
        // triangle faced inward and Blender culled the near side of the model.
        let wound: Vec<u16> = mesh
            .indices
            .chunks_exact(3)
            .flat_map(|t| [t[0], t[2], t[1]])
            .collect();
        let indices = binary.shorts(&wound, 1, "SCALAR", Some(34963));
        let primitive = json!({"attributes":{"POSITION":position,"NORMAL":normal,"TEXCOORD_0":uv,"JOINTS_0":joints,"WEIGHTS_0":weights},
            "indices":indices,"material":material_index,"extras":{"surface":mesh.name}});
        match models.iter_mut().find(|(name, _)| *name == mesh.model) {
            Some((_, primitives)) => primitives.push(primitive),
            None => models.push((mesh.model.clone(), vec![primitive])),
        }
    }
    for (name, primitives) in models {
        roots.push(nodes.len());
        nodes.push(json!({"name":name,"mesh":meshes.len(),"skin":0}));
        meshes.push(json!({"name":name,"primitives":primitives}));
    }
    nodes[0]["children"] = json!(roots);
    // The bones of the gun: j_gun (or j_gun1) and its descendants.
    let mut on_gun = vec![false; bundle.bones.len()];
    for (index, bone) in bundle.bones.iter().enumerate() {
        on_gun[index] = ["j_gun", "j_gun1"].contains(&bone.name.as_str())
            || bone.parent.is_some_and(|p| on_gun[p]);
    }
    let mut animations = vec![];
    // Blender drops animation extras but keeps scene extras as custom properties.
    let mut clip_extras = serde_json::Map::new();
    for clip in clips {
        ensure!(
            clip.fps.is_finite() && clip.fps > 0.0,
            "Invalid animation rate"
        );
        let mut channels = vec![];
        let mut samplers = vec![];
        let mut missing = vec![];
        for track in &clip.tracks {
            let Some(bone) = bundle.bones.iter().position(|b| b.name == track.name) else {
                missing.push(track.name.clone());
                continue;
            };
            // Viewmodel clips key the hands absolutely, but the gun (j_gun and everything on it:
            // the clip, bolt, magazine) relative to its bind pose.
            let relative = !clip.viewmodel || on_gun[bone];
            if !track.rotations.is_empty() {
                let keys = append_duration(&track.rotations, clip.frames);
                ensure!(keys.len() <= 65537, "Excessive animation keys");
                let times: Vec<_> = keys.iter().map(|(f, _)| *f).collect();
                let values: Vec<_> = keys
                    .iter()
                    .flat_map(|(_, q)| {
                        let q = rotation(*q);
                        if relative {
                            (local_poses[bone].0 * q).normalize().to_array()
                        } else {
                            q.to_array()
                        }
                    })
                    .collect();
                let input = binary.times(&times, clip.fps);
                let output = binary.floats(&values, 4, "VEC4", None);
                let sampler = samplers.len();
                samplers.push(json!({"input":input,"output":output,"interpolation":"LINEAR"}));
                channels
                    .push(json!({"sampler":sampler,"target":{"node":bone+1,"path":"rotation"}}));
            }
            if !track.translations.is_empty() {
                let keys = append_duration(&track.translations, clip.frames);
                let times: Vec<_> = keys.iter().map(|(f, _)| *f).collect();
                let values: Vec<_> = keys
                    .iter()
                    .flat_map(|(_, v)| {
                        if relative {
                            (local_poses[bone].1 + point(*v)).to_array()
                        } else {
                            point(*v).to_array()
                        }
                    })
                    .collect();
                let input = binary.times(&times, clip.fps);
                let output = binary.floats(&values, 3, "VEC3", None);
                let sampler = samplers.len();
                samplers.push(json!({"input":input,"output":output,"interpolation":"LINEAR"}));
                channels
                    .push(json!({"sampler":sampler,"target":{"node":bone+1,"path":"translation"}}));
            }
        }
        if !missing.is_empty() {
            allow(
                ask,
                Problem::MissingBones {
                    clip: &clip.name,
                    bones: &missing,
                },
            )?;
        }
        ensure!(
            !channels.is_empty(),
            "Animation {} has no tracks matching the selected rig",
            clip.name
        );
        let extras = json!({"fps":clip.fps,"frames":clip.frames,"looping":clip.looping,"events":clip.events,"omittedBones":missing,"sourceSpace":if clip.viewmodel {"viewmodel-absolute-with-relative-gun"}else{"bind-relative"}});
        clip_extras.insert(clip.name.clone(), extras.clone());
        animations.push(
            json!({"name":clip.name,"samplers":samplers,"channels":channels,"extras":extras}),
        );
    }
    let mut document = json!({"asset":{"version":"2.0","generator":"Hell Hound (original Rust implementation)"},
        "scene":0,"scenes":[{"nodes":[0],"extras":{"hellHoundClips":clip_extras}}],"nodes":nodes,"meshes":meshes,"materials":materials,
        "skins":[{"joints":(1..=bundle.bones.len()).collect::<Vec<_>>(),"inverseBindMatrices":inverse_accessor,"skeleton":0}],
        "buffers":[{"byteLength":binary.bytes.len()}],"bufferViews":binary.views,"accessors":binary.accessors,
        "extras":{"experimentalWaWExport":true,"normalSpecularConversion":"source images preserved; packed shader channels not converted to PBR"}});
    if !images.is_empty() {
        document["images"] = json!(images);
        document["textures"] = json!(textures);
    }
    if !animations.is_empty() {
        document["animations"] = json!(animations);
    }
    let mut json = serde_json::to_vec(&document)?;
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    while binary.bytes.len() % 4 != 0 {
        binary.bytes.push(0);
    }
    let length = 12 + 8 + json.len() + 8 + binary.bytes.len();
    ensure!(length <= u32::MAX as usize, "GLB exceeds 4 GiB");
    let mut glb = vec![];
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(length as u32).to_le_bytes());
    glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&json);
    glb.extend_from_slice(&(binary.bytes.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&binary.bytes);
    Ok(glb)
}
