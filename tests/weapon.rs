use anyhow::{Context, Result};
use hell_hound::{
    live::{Memory, Mode},
    weapon,
};
use std::{collections::BTreeMap, fs, io::Write, path::Path};

#[derive(Default)]
struct Snapshot(BTreeMap<u64, u8>);
impl Snapshot {
    fn put(&mut self, address: u64, bytes: &[u8]) {
        for (i, byte) in bytes.iter().enumerate() {
            self.0.insert(address + i as u64, *byte);
        }
    }
    fn u32(&mut self, address: u64, value: u32) {
        self.put(address, &value.to_le_bytes());
    }
    fn u16(&mut self, address: u64, value: u16) {
        self.put(address, &value.to_le_bytes());
    }
    fn float(&mut self, address: u64, value: f32) {
        self.put(address, &value.to_le_bytes());
    }
}
impl Memory for Snapshot {
    fn read(&self, address: u64, size: usize) -> Result<Vec<u8>> {
        (0..size)
            .map(|i| {
                self.0.get(&(address + i as u64)).copied().with_context(|| {
                    format!("Unmapped synthetic address {:#x}", address + i as u64)
                })
            })
            .collect()
    }
}

const MODEL: u64 = 0x200004 + 228;
const ANIM: u64 = 0x100004;
fn fixture() -> Snapshot {
    let mut m = Snapshot::default();
    m.put(0x400000, &[0; 256]);
    m.put(0x400000, b"MZ");
    m.u32(0x40003c, 128);
    m.put(0x400080, b"PE\0\0\x4c\x01");
    m.put(0x400098, &[0x0b, 0x01]);
    m.u32(0x8dc828 + 4 * 4, 0x100000);
    m.u32(0x8dc828 + 5 * 4, 0x200000);
    m.u32(0x8dc5d0 + 4 * 4, 1);
    m.u32(0x8dc5d0 + 5 * 4, 2);
    m.put(0x200004, &[0; 456]);
    m.u32(0x200004, 0x300000);
    m.put(0x300000, b"void\0");
    m.u32(MODEL, 0x300100);
    m.put(0x300100, b"viewmodel_test_gun\0");
    m.put(MODEL + 4, &[2, 1, 1, 0]);
    for (offset, value) in [
        (8, 0x510000),
        (12, 0x510100),
        (28, 0x510200),
        (32, 0x520000),
        (36, 0x520100),
    ] {
        m.u32(MODEL + offset, value);
    }
    m.u16(MODEL + 44, 1);
    m.u16(MODEL + 46, 0);
    m.u16(MODEL + 196, 1);
    m.put(0x510000, &[1, 0, 2, 0]);
    m.put(0x510100, &[1]);
    m.put(0x3702400 + 12 + 4, b"tag_origin\0");
    m.put(0x3702400 + 24 + 4, b"j_gun\0");
    m.put(0x3702400 + 36 + 4, b"fire\0");
    m.put(0x510200, &[0; 64]);
    m.float(0x510200 + 12, 1.0);
    m.float(0x510200 + 32 + 12, 1.0);
    m.float(0x510200 + 32 + 16, 1.0);
    m.put(0x520000, &[0; 64]);
    m.u16(0x520002, 3);
    m.u16(0x520004, 1);
    m.u32(0x52000c, 0x521000);
    m.u16(0x520010, 3);
    m.u32(0x520018, 0x521100);
    m.u32(0x52001c, 0x522000);
    m.put(0x521000, &[0, 0, 1, 0, 2, 0]);
    m.put(0x521100, &[0, 0, 64, 0, 64, 0]);
    m.put(0x522000, &[0; 96]);
    for (index, position) in [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        .iter()
        .enumerate()
    {
        for (axis, value) in position.iter().enumerate() {
            m.float(0x522000 + index as u64 * 32 + axis as u64 * 4, *value);
        }
        m.put(0x522000 + index as u64 * 32 + 24, &[127, 127, 254, 0]);
    }
    m.u32(0x520100, 0x530000);
    m.put(0x530000, &[0; 104]);
    m.u32(0x530000, 0x530100);
    m.put(0x530100, b"test_material\0");
    m.put(0x530000 + 91, &[1]);
    m.u32(0x530000 + 100, 0x530200);
    m.put(0x530200, &[0; 16]);
    m.u32(0x530200, 0xa0ab1041);
    m.u32(0x53020c, 0x530300);
    m.u32(0x530300 + 32, 0x530400);
    m.put(0x530400, b"test_diffuse\0");
    m.put(ANIM, &[0; 88]);
    m.u32(ANIM, 0x300200);
    m.put(0x300200, b"viewmodel_test_fire\0");
    m.u16(ANIM + 14, 2);
    m.put(ANIM + 18, &[1, 0, 1, 0, 0, 0, 0, 1, 0, 2, 1]);
    m.float(ANIM + 40, 30.0);
    for (offset, value) in [
        (48, 0x510000),
        (52, 0x540000),
        (56, 0x540100),
        (60, 0x540200),
        (64, 0x540300),
        (80, 0x540500),
    ] {
        m.u32(ANIM + offset, value);
    }
    m.put(0x540000, &[0, 2, 1]);
    m.u16(0x540100, 1);
    m.put(0x540200, &[0; 12]);
    m.float(0x540204, 0.5);
    m.put(0x540300, &[0; 16]);
    m.u16(0x540306, 32767);
    m.u16(0x54030c, 23170);
    m.u16(0x54030e, 23170);
    m.put(0x540500, &[0; 8]);
    m.u16(0x540500, 3);
    m.float(0x540504, 0.5);
    m
}

fn dir() -> tempfile::TempDir {
    if Path::new("/tmp/opencode").is_dir() {
        tempfile::tempdir_in("/tmp/opencode").unwrap()
    } else {
        tempfile::tempdir().unwrap()
    }
}

#[test]
fn native_model_decoder_preserves_skin_hierarchy_geometry_and_materials() {
    let model = weapon::model::decode(&fixture(), Mode::Sp, 0x400000, MODEL).unwrap();
    assert_eq!(model.bones.len(), 2);
    assert_eq!(model.bones[1].parent, Some(0));
    assert_eq!(model.bones[1].name, "j_gun");
    assert_eq!(model.meshes[0].vertices.len(), 3);
    assert_eq!(model.meshes[0].indices, vec![0, 1, 2]);
    assert_eq!(model.meshes[0].vertices[1].joints[0], 1);
    assert_eq!(model.meshes[0].vertices[1].weights[0], 1.0);
    assert_eq!(model.meshes[0].material.images["diffuse"], "test_diffuse");
}

#[test]
fn animation_decodes_keyed_rotations_static_translation_and_events() {
    let clip = weapon::animation::decode(&fixture(), Mode::Sp, 0x400000, ANIM).unwrap();
    assert_eq!(clip.frames, 2);
    assert_eq!(clip.fps, 30.0);
    assert!(clip.viewmodel);
    assert_eq!(clip.tracks[1].rotations.len(), 2);
    assert_eq!(clip.tracks[1].rotations[1].0, 2);
    assert!((clip.tracks[1].rotations[1].1.z - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.001);
    assert_eq!(
        clip.tracks[1].translations[0].1,
        glam::Vec3::new(0.0, 0.5, 0.0)
    );
    assert_eq!(clip.events[0].0, "fire");
}

#[test]
fn full_weapon_export_is_valid_skinned_animated_textured_glb() {
    let root = dir();
    let source = root.path().join("test.iwd");
    let mut iwi = vec![0; 36];
    iwi[..4].copy_from_slice(b"IWi\x08");
    iwi[8] = 1;
    iwi[10..12].copy_from_slice(&1u16.to_le_bytes());
    iwi[12..14].copy_from_slice(&1u16.to_le_bytes());
    iwi[32..].copy_from_slice(&[5, 100, 200, 255]);
    let mut writer = zip::ZipWriter::new(fs::File::create(&source).unwrap());
    writer
        .start_file(
            "images/test_diffuse.iwi",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    writer.write_all(&iwi).unwrap();
    writer.finish().unwrap();
    let output = root.path().join("weapon.glb");
    let options = weapon::Options {
        models: vec!["viewmodel_test_gun".into()],
        animations: vec!["viewmodel_test_fire".into()],
        ..Default::default()
    };
    weapon::export(
        &fixture(),
        Mode::Sp,
        0x400000,
        &[source.canonicalize().unwrap()],
        &output,
        &options,
    )
    .unwrap();
    let bytes = fs::read(&output).unwrap();
    let glb = gltf::Gltf::from_slice(&bytes).unwrap();
    let data = glb.blob.as_deref().unwrap();
    assert_eq!(glb.skins().count(), 1);
    assert_eq!(glb.skins().next().unwrap().joints().count(), 2);
    let primitive = glb.meshes().next().unwrap().primitives().next().unwrap();
    let reader = primitive.reader(|_| Some(data));
    assert_eq!(reader.read_positions().unwrap().count(), 3);
    // Units/axes are baked into the data; a scaled root makes Blender draw
    // bones 39x too long, hiding the weapon.
    let root_node = glb.scenes().next().unwrap().nodes().next().unwrap();
    assert_eq!(
        root_node.transform().decomposed(),
        ([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3])
    );
    let positions: Vec<_> = reader.read_positions().unwrap().collect();
    for (actual, expected) in positions[1..]
        .iter()
        .zip([[0.0254, 0.0, 0.0], [0.0, 0.0, -0.0254]])
    {
        assert!(
            actual
                .iter()
                .zip(expected)
                .all(|(a, e)| (a - e).abs() < 1e-6)
        );
    }
    let gun = glb.nodes().find(|n| n.name() == Some("j_gun")).unwrap();
    assert!((gun.transform().decomposed().0[0] - 0.0254).abs() < 1e-6);
    assert_eq!(
        reader.read_joints(0).unwrap().into_u16().nth(1).unwrap()[0],
        1
    );
    assert_eq!(glb.animations().count(), 1);
    assert_eq!(glb.animations().next().unwrap().channels().count(), 3);
    assert_eq!(glb.images().count(), 1);
    // Optional explicit synthetic output for an external headless Blender import test.
    if let Some(path) = std::env::var_os("HELL_HOUND_TEST_GLB") {
        let mut target = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        target.write_all(&bytes).unwrap();
    }
}

#[test]
fn delta_animation_bad_indices_and_missing_bones_fail_instead_of_silent_truncation() {
    let mut m = fixture();
    m.put(ANIM + 17, &[1]);
    assert!(weapon::animation::decode(&m, Mode::Sp, 0x400000, ANIM).is_err());
    let mut m = fixture();
    m.u16(0x521002, 99);
    assert!(weapon::model::decode(&m, Mode::Sp, 0x400000, MODEL).is_err());
    let m = fixture();
    let bundle = weapon::model::decode(&m, Mode::Sp, 0x400000, MODEL).unwrap();
    let mut clip = weapon::animation::decode(&m, Mode::Sp, 0x400000, ANIM).unwrap();
    clip.tracks[1].name = "missing_hands".into();
    assert!(weapon::gltf::encode(&bundle, &[clip.clone()], &[], true, false).is_err());
    assert!(weapon::gltf::encode(&bundle, &[clip], &[], true, true).is_ok());
}

#[test]
fn export_is_deterministic_no_clobber_and_dry_run_writes_nothing() {
    let m = fixture();
    let root = dir();
    let output = root.path().join("test.glb");
    let mut options = weapon::Options {
        models: vec!["viewmodel_test_gun".into()],
        animation_patterns: vec!["*test_fire".into()],
        no_textures: true,
        dry_run: true,
        ..Default::default()
    };
    weapon::export(&m, Mode::Sp, 0x400000, &[], &output, &options).unwrap();
    assert!(!output.exists());
    options.dry_run = false;
    weapon::export(&m, Mode::Sp, 0x400000, &[], &output, &options).unwrap();
    let first = fs::read(&output).unwrap();
    assert!(weapon::export(&m, Mode::Sp, 0x400000, &[], &output, &options).is_err());
    options.overwrite = true;
    weapon::export(&m, Mode::Sp, 0x400000, &[], &output, &options).unwrap();
    assert_eq!(first, fs::read(&output).unwrap());
}

#[test]
fn animation_handles_short_and_long_frame_tables() {
    let mut m = fixture();
    m.u16(ANIM + 14, 300);
    m.put(0x540000, &[1]);
    m.put(0x540100, &[1, 0, 0, 0, 44, 1]);
    let clip = weapon::animation::decode(&m, Mode::Sp, 0x400000, ANIM).unwrap();
    assert_eq!(clip.tracks[1].rotations[1].0, 300);
    let mut m = fixture();
    m.u16(ANIM + 14, 300);
    m.put(ANIM + 28, &[0]);
    m.put(0x540000, &[1]);
    m.put(0x540100, &[64, 0, 0, 0, 0, 0]);
    m.u32(ANIM + 76, 0x540800);
    let mut rotations = vec![0; 65 * 8];
    for index in 0..65 {
        rotations[index * 8 + 6..index * 8 + 8].copy_from_slice(&32767u16.to_le_bytes());
        m.u16(0x540800 + index as u64 * 2, index as u16);
    }
    m.put(0x540300, &rotations);
    let clip = weapon::animation::decode(&m, Mode::Sp, 0x400000, ANIM).unwrap();
    assert_eq!(clip.tracks[1].rotations.len(), 65);
    assert_eq!(clip.tracks[1].rotations[64].0, 64);
}

#[test]
fn animation_decodes_byte_and_precise_quantized_translations() {
    for precise in [false, true] {
        let mut m = fixture();
        m.put(
            ANIM + 23,
            &[
                if precise { 0 } else { 1 },
                if precise { 1 } else { 0 },
                0,
                1,
            ],
        );
        m.put(0x540000, &[0, 2, 1, 0, 2]);
        m.put(0x540100, &[1, 0, 1, 0]);
        m.put(0x540200, &[0; 24]);
        m.float(0x54020c, 0.5);
        m.float(0x540210, 0.25);
        m.float(0x540214, 1.0);
        if precise {
            m.put(0x540310, &[0, 0, 0, 0, 0, 0, 2, 0, 4, 0, 6, 0]);
        } else {
            m.u32(ANIM + 68, 0x540400);
            m.put(0x540400, &[0, 0, 0, 2, 4, 6]);
        }
        let clip = weapon::animation::decode(&m, Mode::Sp, 0x400000, ANIM).unwrap();
        assert_eq!(clip.tracks[1].translations.len(), 2);
        assert_eq!(
            clip.tracks[1].translations[1].1,
            glam::Vec3::new(1.0, 1.0, 6.0)
        );
    }
}

#[test]
fn animation_decodes_static_quaternion_categories() {
    let mut m = fixture();
    m.put(ANIM + 18, &[0, 0, 0, 1, 1]);
    m.put(0x540000, &[1]);
    m.put(0x540100, &[0, 0, 255, 127, 0, 0, 0, 0, 130, 90, 130, 90]);
    let clip = weapon::animation::decode(&m, Mode::Sp, 0x400000, ANIM).unwrap();
    assert_eq!(clip.tracks[0].rotations.len(), 1);
    assert!(
        clip.tracks[0].rotations[0]
            .1
            .abs_diff_eq(glam::Quat::IDENTITY, 0.001)
    );
    assert!(clip.tracks[1].rotations[0].1.z > 0.7);
}

#[test]
fn rigid_and_blended_model_weights_are_preserved() {
    let mut m = fixture();
    m.u16(0x520010, 0);
    m.u32(0x520024, 1);
    m.u32(0x520028, 0x521200);
    m.put(0x521200, &[0; 12]);
    m.u16(0x521200, 64);
    m.u16(0x521202, 3);
    let model = weapon::model::decode(&m, Mode::Sp, 0x400000, MODEL).unwrap();
    assert!(
        model.meshes[0]
            .vertices
            .iter()
            .all(|v| v.joints[0] == 1 && v.weights[0] == 1.0)
    );
    let mut m = fixture();
    m.u16(0x520010, 2);
    m.u16(0x520016, 1);
    m.put(
        0x521100,
        &[0, 0, 64, 0, 0, 0, 64, 0, 0, 64, 0, 0, 0, 64, 64, 0, 0, 64],
    );
    let model = weapon::model::decode(&m, Mode::Sp, 0x400000, MODEL).unwrap();
    assert_eq!(model.meshes[0].vertices[2].weights, [0.25; 4]);
}

#[test]
fn model_merge_rejects_incompatible_shared_rigs() {
    let m = fixture();
    let mut bundle = weapon::model::Bundle::default();
    bundle
        .merge(weapon::model::decode(&m, Mode::Sp, 0x400000, MODEL).unwrap())
        .unwrap();
    let mut other = weapon::model::decode(&m, Mode::Sp, 0x400000, MODEL).unwrap();
    other.bones[1].global = glam::Mat4::from_translation(glam::Vec3::X * 99.0);
    assert!(bundle.merge(other).is_err());
}

#[test]
#[ignore = "requires WAW_GAME_DIR pointing at an owned installation; reads only"]
fn installed_waw_diffuse_texture_converts_to_valid_png() {
    let root = std::env::var_os("WAW_GAME_DIR").expect("Set WAW_GAME_DIR");
    let sources = hell_hound::discovery::archives_in(Path::new(&root)).unwrap();
    let assets = hell_hound::archive::catalog(&sources).unwrap();
    let asset = assets
        .iter()
        .find(|a| a.member == "images/colt45_c.iwi")
        .expect("Stock WaW Colt diffuse texture");
    let png = weapon::texture::iwi_png(
        &hell_hound::archive::read_asset(asset, hell_hound::archive::DEFAULT_LIMIT).unwrap(),
    )
    .unwrap();
    let image = image::load_from_memory(&png).unwrap();
    assert!(image.width() > 0 && image.height() > 0);
    println!(
        "Converted owned WaW diffuse texture to PNG: {}x{}",
        image.width(),
        image.height()
    );
}
