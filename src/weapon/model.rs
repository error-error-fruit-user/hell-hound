use super::{bone_name, f32_at, u16_at, u32_at};
use crate::live::{self, Memory, Mode};
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec2, Vec3};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    pub global: Mat4,
}
#[derive(Clone, Debug)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub uv: Vec2,
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}
#[derive(Clone, Debug, Default)]
pub struct Material {
    pub name: String,
    pub images: BTreeMap<String, String>,
}
#[derive(Clone, Debug)]
pub struct Mesh {
    pub name: String,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
    pub material: Material,
}
#[derive(Default)]
pub struct Bundle {
    pub bones: Vec<Bone>,
    pub meshes: Vec<Mesh>,
}

impl Bundle {
    pub fn merge(&mut self, other: Self) -> Result<()> {
        let mut mapping = vec![];
        for bone in other.bones {
            let parent = bone.parent.map(|p| mapping[p]);
            if let Some(index) = self.bones.iter().position(|b| b.name == bone.name) {
                let old = &self.bones[index];
                ensure!(
                    old.parent == parent && old.global.abs_diff_eq(bone.global, 0.005),
                    "Models disagree on bind pose/parent for shared bone {:?}; cannot silently merge incompatible rigs",
                    bone.name
                );
                mapping.push(index);
            } else {
                let index = self.bones.len();
                ensure!(index < u16::MAX as usize, "Too many merged bones");
                self.bones.push(Bone { parent, ..bone });
                mapping.push(index);
            }
        }
        for mut mesh in other.meshes {
            for vertex in &mut mesh.vertices {
                for joint in &mut vertex.joints {
                    *joint = *mapping
                        .get(*joint as usize)
                        .context("Joint index outside model rig")?
                        as u16;
                }
            }
            self.meshes.push(mesh);
        }
        Ok(())
    }
}

fn material(memory: &impl Memory, address: u64) -> Result<Material> {
    let header = memory.read(address, 104)?;
    let name = live::string(memory, u32_at(&header, 0) as u64)?;
    let count = header[91] as usize;
    ensure!(count <= 64, "Excessive material image count");
    let table = u32_at(&header, 100) as u64;
    let mut images = BTreeMap::new();
    for index in 0..count {
        let entry = memory.read(table + index as u64 * 16, 16)?;
        let semantic = match u32_at(&entry, 0) {
            0xa0ab1041 => "diffuse".into(),
            0x59d30d0f => "normal".into(),
            0x34ecccb3 => "specular".into(),
            hash => format!("semantic_{hash:08x}"),
        };
        let image = u32_at(&entry, 12) as u64;
        let image_name = live::string(memory, live::word(memory, image + 32)?)?;
        images.insert(semantic, image_name);
    }
    Ok(Material { name, images })
}

pub fn decode(memory: &impl Memory, mode: Mode, base: u64, address: u64) -> Result<Bundle> {
    let header = memory.read(address, 228)?;
    let name = live::string(memory, u32_at(&header, 0) as u64)?;
    ensure!(
        !["void", "defaultactor", "defaultweapon"].contains(&name.as_str()),
        "Placeholder model cannot be exported"
    );
    let bone_count = header[4] as usize;
    let roots = header[5] as usize;
    ensure!(
        bone_count > 0 && roots > 0 && roots <= bone_count,
        "Unsupported/invalid bone counts"
    );
    let lods = u16_at(&header, 196);
    ensure!((1..=4).contains(&lods), "Invalid model LOD count");
    let mut result = Bundle::default();
    for index in 0..bone_count {
        let id = u16_at(
            &memory.read(u32_at(&header, 8) as u64 + index as u64 * 2, 2)?,
            0,
        );
        let bone_name = bone_name(memory, mode, base, id)?;
        ensure!(
            !result.bones.iter().any(|b| b.name == bone_name),
            "Duplicate bone name: {bone_name}"
        );
        let parent = if index < roots {
            None
        } else {
            let distance =
                memory.read(u32_at(&header, 12) as u64 + (index - roots) as u64, 1)?[0] as usize;
            ensure!(
                distance > 0 && distance <= index,
                "Invalid bone parent distance"
            );
            Some(index - distance)
        };
        let bind = memory.read(u32_at(&header, 28) as u64 + index as u64 * 32, 32)?;
        let rotation = Quat::from_xyzw(
            f32_at(&bind, 0),
            f32_at(&bind, 4),
            f32_at(&bind, 8),
            f32_at(&bind, 12),
        );
        let translation = Vec3::new(f32_at(&bind, 16), f32_at(&bind, 20), f32_at(&bind, 24));
        ensure!(
            rotation.is_finite() && rotation.length_squared() > 0.1 && translation.is_finite(),
            "Invalid bind transform"
        );
        result.bones.push(Bone {
            name: bone_name,
            parent,
            global: Mat4::from_rotation_translation(rotation.normalize(), translation),
        });
    }
    // Highest-detail LOD only. Other LODs are not combined into duplicate geometry.
    let surfaces = u16_at(&header, 44) as usize;
    let first = u16_at(&header, 46) as usize;
    ensure!(
        surfaces > 0 && first + surfaces <= header[6] as usize,
        "Invalid LOD surface range"
    );
    let mut total_vertices = 0usize;
    for surface_index in first..first + surfaces {
        let surface = memory.read(u32_at(&header, 32) as u64 + surface_index as u64 * 64, 64)?;
        let vertices = u16_at(&surface, 2) as usize;
        let faces = u16_at(&surface, 4) as usize;
        total_vertices += vertices;
        ensure!(
            total_vertices <= 1_000_000,
            "Model exceeds the decoded vertex limit"
        );
        ensure!(vertices > 0 && faces > 0, "Empty model surface");
        let mut influences = Vec::with_capacity(vertices);
        let rigid_count = u32_at(&surface, 36) as usize;
        ensure!(rigid_count <= vertices, "Invalid rigid weight list count");
        let joint = |value: u16| -> Result<u16> {
            ensure!(
                value % 64 == 0 && (value / 64) < bone_count as u16,
                "Invalid encoded bone index {value}"
            );
            Ok(value / 64)
        };
        for rigid in 0..rigid_count {
            let item = memory.read(u32_at(&surface, 40) as u64 + rigid as u64 * 12, 12)?;
            let bone = joint(u16_at(&item, 0))?;
            let count = u16_at(&item, 2) as usize;
            ensure!(
                influences.len() + count <= vertices,
                "Rigid weights exceed vertex count"
            );
            influences.extend(std::iter::repeat_n(
                ([bone, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
                count,
            ));
        }
        let mut cursor = super::Cursor::new(memory, u32_at(&surface, 24));
        for count in 1..=4 {
            for _ in 0..u16_at(&surface, 16 + (count - 1) * 2) {
                let mut joints = [0; 4];
                let mut weights = [0.0; 4];
                joints[0] = joint(cursor.short()?)?;
                let mut sum = 0.0;
                for index in 1..count {
                    joints[index] = joint(cursor.short()?)?;
                    weights[index] = cursor.short()? as f32 / 65536.0;
                    sum += weights[index];
                }
                ensure!(sum <= 1.0001, "Invalid skin weight sum");
                weights[0] = (1.0 - sum).max(0.0);
                influences.push((joints, weights));
                ensure!(
                    influences.len() <= vertices,
                    "Blend weights exceed vertex count"
                );
            }
        }
        if influences.is_empty() && bone_count == 1 {
            influences.resize(vertices, ([0; 4], [1.0, 0.0, 0.0, 0.0]));
        }
        ensure!(
            influences.len() == vertices,
            "Skin weight count does not match vertex count; unsupported surface layout"
        );
        let data = memory.read(u32_at(&surface, 28) as u64, vertices * 32)?;
        let mut decoded = vec![];
        for (index, (joints, weights)) in influences.into_iter().enumerate() {
            let v = &data[index * 32..index * 32 + 32];
            let position = Vec3::new(f32_at(v, 0), f32_at(v, 4), f32_at(v, 8));
            let normal = Vec3::new(
                v[24] as f32 - 127.0,
                v[25] as f32 - 127.0,
                v[26] as f32 - 127.0,
            );
            let uv = Vec2::new(
                half::f16::from_bits(u16_at(v, 22)).to_f32(),
                half::f16::from_bits(u16_at(v, 20)).to_f32(),
            );
            ensure!(
                position.is_finite() && uv.is_finite() && normal.length_squared() > 0.0,
                "Invalid vertex values"
            );
            decoded.push(Vertex {
                position,
                normal: normal.normalize(),
                uv,
                joints,
                weights,
            });
        }
        let face_data = memory.read(u32_at(&surface, 12) as u64, faces * 6)?;
        let indices: Vec<_> = face_data.chunks_exact(2).map(|v| u16_at(v, 0)).collect();
        ensure!(
            indices.iter().all(|i| (*i as usize) < vertices),
            "Triangle index out of bounds"
        );
        let material_ptr = live::word(
            memory,
            u32_at(&header, 36) as u64 + surface_index as u64 * 4,
        )?;
        result.meshes.push(Mesh {
            name: format!("{name}_surface_{surface_index}"),
            vertices: decoded,
            indices,
            material: material(memory, material_ptr)?,
        });
    }
    Ok(result)
}
