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
use std::{fmt, path::Path};

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
    pub dry_run: bool,
}

/// Something the export can work around, if whoever runs it agrees: the TUI asks, the batch
/// commands answer from their flags.
pub enum Problem<'a> {
    /// A clip animates bones the selected models don't have; going on leaves those tracks out.
    MissingBones { clip: &'a str, bones: &'a [String] },
    /// A material's image isn't in the loaded IWDs; going on exports the material without it.
    MissingTexture(&'a str),
    /// No IWDs were found to read textures from; going on exports without textures.
    NoTextureArchives,
    /// Going on replaces the file.
    OutputExists(&'a Path),
}

impl Problem<'_> {
    /// What going on does, to ask about.
    pub fn workaround(&self) -> &'static str {
        match self {
            Self::MissingBones { .. } => "Leave those bones out",
            Self::MissingTexture(_) => "Export without it",
            Self::NoTextureArchives => "Export without textures",
            Self::OutputExists(_) => "Overwrite it",
        }
    }
}

impl fmt::Display for Problem<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::MissingBones { clip, bones } => write!(
                f,
                "Animation {clip} needs bones the selected models don't have: {}",
                bones.join(", ")
            ),
            Self::MissingTexture(member) => write!(f, "Texture {member} is not in the loaded IWDs"),
            Self::NoTextureArchives => write!(f, "No IWDs were found to read textures from"),
            Self::OutputExists(path) => write!(f, "{} already exists", path.display()),
        }
    }
}

/// Answers whether to work around a [`Problem`]: `Ok(false)` stops the export with the problem
/// as its error, and an error stops it with that error instead.
pub type Ask<'a> = dyn FnMut(&Problem) -> Result<bool> + 'a;

/// Asks about `problem`, failing with it unless the answer is to go on.
pub fn allow(ask: &mut Ask, problem: Problem) -> Result<()> {
    ensure!(ask(&problem)?, "{problem}");
    Ok(())
}

pub fn export(
    memory: &impl Memory,
    mode: Mode,
    image_base: u64,
    sources: &[std::path::PathBuf],
    output: &Path,
    options: &Options,
    ask: &mut Ask,
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
    let overwrite = output.exists();
    if overwrite {
        allow(ask, Problem::OutputExists(output))?;
    }
    let before = live::list(memory, mode, image_base)?;
    let lookup = |kind: &str, name: &str| -> Result<&live::LiveAsset> {
        let matches: Vec<_> = before
            .iter()
            .filter(|a| a.kind == kind && a.name == name)
            .collect();
        ensure!(
            matches.len() == 1,
            "Expected one loaded {kind} named {name:?}, found {}. Use list --kind model or animation, or the interface, first",
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
    // The game hangs the gun on the hands; without this the gun stays at the view origin.
    bundle.attach("j_gun", "tag_weapon")?;
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
    let mut no_textures = options.no_textures;
    if !no_textures && sources.is_empty() {
        allow(ask, Problem::NoTextureArchives)?;
        no_textures = true;
    }
    let archives = if no_textures {
        vec![]
    } else {
        archive::catalog(sources)?
    };
    let bytes = gltf::encode(&bundle, &clips, &archives, no_textures, ask)?;
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
        archive::atomic_write(output, &bytes, overwrite).context("Cannot publish GLB output")?;
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
