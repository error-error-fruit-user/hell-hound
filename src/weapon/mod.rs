//! Independent WaW decoders and glTF 2.0 export. Unknown layouts fail explicitly.
pub mod animation;
pub mod gltf;
pub mod model;
pub mod texture;

use crate::{
    archive,
    live::{self, Memory, Mode},
};
use anyhow::{Context, Result, ensure};
use std::path::Path;

pub fn u16_at(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        data[offset..offset + 2]
            .try_into()
            .expect("fixed-layout slice"),
    )
}
pub fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        data[offset..offset + 4]
            .try_into()
            .expect("fixed-layout slice"),
    )
}
pub fn f32_at(data: &[u8], offset: usize) -> f32 {
    f32::from_bits(u32_at(data, offset))
}

pub fn bone_name(memory: &impl Memory, mode: Mode, image_base: u64, id: u16) -> Result<String> {
    let table = match mode {
        Mode::Sp => 0x3702400,
        Mode::Mp => 0xf66b400,
    };
    live::string(
        memory,
        image_base + (table - 0x400000) + u64::from(id) * 12 + 4,
    )
}

#[derive(Default)]
pub struct Options {
    pub models: Vec<String>,
    pub animations: Vec<String>,
    pub animation_patterns: Vec<String>,
    pub no_textures: bool,
    pub allow_missing_bones: bool,
    pub overwrite: bool,
    pub dry_run: bool,
}

pub fn export(
    memory: &impl Memory,
    mode: Mode,
    image_base: u64,
    sources: &[std::path::PathBuf],
    output: &Path,
    options: &Options,
) -> Result<()> {
    ensure!(
        !options.models.is_empty(),
        "Select at least one model (add hands with a second --model)"
    );
    let extension = output.extension().and_then(|s| s.to_str()).unwrap_or("");
    ensure!(
        extension.eq_ignore_ascii_case("glb"),
        "Weapon output must end in .glb"
    );
    ensure!(
        !output.exists() || options.overwrite,
        "Output already exists; choose another file or use --overwrite"
    );
    let before = live::list(memory, mode, image_base)?;
    let lookup = |kind: &str, name: &str| -> Result<&live::LiveAsset> {
        let matches: Vec<_> = before
            .iter()
            .filter(|a| a.kind == kind && a.name == name)
            .collect();
        ensure!(
            matches.len() == 1,
            "Expected one loaded {kind} named {name:?}, found {}. Use live-list or the interactive browser first",
            matches.len()
        );
        Ok(matches[0])
    };
    let mut bundle = model::Bundle::default();
    for name in &options.models {
        let asset = lookup("model", name)?;
        bundle.merge(
            model::decode(memory, mode, image_base, asset.address)
                .with_context(|| format!("Cannot decode model {name}"))?,
        )?;
    }
    let patterns: Vec<_> = options
        .animation_patterns
        .iter()
        .map(|p| glob::Pattern::new(p))
        .collect::<std::result::Result<_, _>>()?;
    let mut names = options.animations.clone();
    for asset in before.iter().filter(|a| a.kind == "animation") {
        if patterns.iter().any(|p| p.matches(&asset.name)) && !names.contains(&asset.name) {
            names.push(asset.name.clone());
        }
    }
    ensure!(
        options.animation_patterns.is_empty() || !names.is_empty(),
        "No animations matched the requested patterns"
    );
    names.sort();
    names.dedup();
    let mut clips = vec![];
    for name in names {
        let asset = lookup("animation", &name)?;
        clips.push(
            animation::decode(memory, mode, image_base, asset.address)
                .with_context(|| format!("Cannot decode animation {name}"))?,
        );
    }
    let archives = if options.no_textures {
        vec![]
    } else {
        ensure!(
            !sources.is_empty(),
            "Texture archives not found. Supply --game-dir or explicitly use --no-textures"
        );
        archive::catalog(sources)?
    };
    let bytes = gltf::encode(
        &bundle,
        &clips,
        &archives,
        options.no_textures,
        options.allow_missing_bones,
    )?;
    // Ensure a map switch/reload did not replace the chosen assets during reads.
    let after = live::list(memory, mode, image_base)?;
    for selected in before
        .iter()
        .filter(|a| options.models.contains(&a.name) || clips.iter().any(|c| c.name == a.name))
    {
        ensure!(
            after.iter().any(|a| a.kind == selected.kind
                && a.name == selected.name
                && a.address == selected.address),
            "Game assets changed while decoding; no output written. Keep the map stable and retry"
        );
    }
    ensure!(
        !sources
            .iter()
            .any(|s| output.canonicalize().ok().as_ref() == Some(s)),
        "Output would overwrite a source"
    );
    if !options.dry_run {
        archive::atomic_write(output, &bytes, options.overwrite)
            .context("Cannot publish GLB output")?;
    }
    Ok(())
}

pub struct Cursor<'a, M: Memory> {
    pub memory: &'a M,
    pub position: u64,
}
impl<'a, M: Memory> Cursor<'a, M> {
    pub fn new(memory: &'a M, address: u32) -> Self {
        Self {
            memory,
            position: address as u64,
        }
    }
    pub fn bytes(&mut self, count: usize) -> Result<Vec<u8>> {
        ensure!(count <= 16 * 1024 * 1024, "Stream read exceeds limit");
        ensure!(
            self.position >= 0x10000 && self.position + count as u64 <= u32::MAX as u64,
            "Invalid WaW stream address"
        );
        let bytes = self.memory.read(self.position, count)?;
        self.position += count as u64;
        Ok(bytes)
    }
    pub fn byte(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    pub fn short(&mut self) -> Result<u16> {
        Ok(u16_at(&self.bytes(2)?, 0))
    }
    pub fn vector(&mut self) -> Result<glam::Vec3> {
        let bytes = self.bytes(12)?;
        let value = glam::Vec3::new(f32_at(&bytes, 0), f32_at(&bytes, 4), f32_at(&bytes, 8));
        ensure!(value.is_finite(), "Non-finite vector");
        Ok(value)
    }
}
