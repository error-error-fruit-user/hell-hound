//! Original, read-only reading of WaW's loaded sound aliases. An alias (what weapon files and
//! notetracks name, like `weap_thompson_fire_plr`) is a list of variants; each variant's file is
//! either loaded into memory as a whole RIFF WAV, or streamed from `sound/` in the IWDs.
//! Layout facts (single player, measured on the stock Steam build): alias pool index 9, list
//! stride 12 (name, variants, count), variant stride 184 with its sound file at 20; a sound file
//! starts with its type (1 loaded, 2 or 3 streamed), loaded ones point at a (name, data, size) record
//! at 4, streamed ones name their folder at 8 and file at 12. Unknown layouts fail closed.

use crate::{
    archive::{self, Asset},
    live::{self, Memory, Mode, Pools},
};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

const ALIAS_POOL: u64 = 9;
const LIST_STRIDE: u64 = 12;
const VARIANT_STRIDE: u64 = 184;
const MAX_VARIANTS: u64 = 256;
/// The biggest loaded WAV accepted, as a sanity bound.
const MAX_LOADED: u32 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SoundFile {
    /// A whole WAV in game memory.
    Loaded {
        name: String,
        address: u64,
        size: u32,
    },
    /// A file under `sound/` in the IWDs.
    Streamed { path: String },
}

impl SoundFile {
    /// The source file's name, for messages and to name extracted copies.
    pub fn name(&self) -> &str {
        match self {
            Self::Loaded { name, .. } => name,
            Self::Streamed { path } => path,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Alias {
    pub name: String,
    /// The variants the game picks from at random; some are empty (no file).
    pub files: Vec<SoundFile>,
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated fixed layout"),
    )
}

/// A name that may be empty (streamed sounds in the game folder itself have no folder).
fn text(memory: &impl Memory, address: u64) -> Result<String> {
    if memory.read(address, 1)?[0] == 0 {
        Ok(String::new())
    } else {
        live::string(memory, address)
    }
}

fn file(memory: &impl Memory, address: u64) -> Result<Option<SoundFile>> {
    let header = memory.read(address, 16)?;
    Ok(match header[0] {
        1 => {
            let record = memory.read(u32_at(&header, 4) as u64, 12)?;
            let name = live::string(memory, u32_at(&record, 0) as u64)?;
            let size = u32_at(&record, 8);
            ensure!(
                (12..=MAX_LOADED).contains(&size),
                "Invalid loaded sound size for {name}"
            );
            Some(SoundFile::Loaded {
                name,
                address: u32_at(&record, 4) as u64,
                size,
            })
        }
        // 3 is laid out as streamed (the few music cues that use it)
        2 | 3 => {
            let folder = text(memory, u32_at(&header, 8) as u64)?;
            let name = live::string(memory, u32_at(&header, 12) as u64)?;
            let path = if folder.is_empty() {
                name
            } else {
                format!("{folder}/{name}")
            };
            Some(SoundFile::Streamed {
                path: path.replace('\\', "/"),
            })
        }
        0 => None,
        kind => bail!("Unknown sound file type {kind}; refusing to decode an unknown layout"),
    })
}

/// Every loaded sound alias, sorted by name. Only single player's layout is known.
pub fn aliases(memory: &impl Memory, mode: Mode, image_base: u64) -> Result<Vec<Alias>> {
    ensure!(
        matches!(mode, Mode::Sp),
        "Sound aliases are only decoded in single player (zombies)"
    );
    let tables = Pools::open(memory, mode, image_base)?;
    let (pool, count) = tables.pool(ALIAS_POOL, "sound alias")?;
    let first = pool + 4;
    let end = first + count * LIST_STRIDE;
    let lists = memory.read(first, (count * LIST_STRIDE) as usize)?;
    let mut result = vec![];
    for slot in 0..count as usize {
        let name_ptr = u32_at(&lists, slot * 12) as u64;
        // Free slots point back into the pool.
        if name_ptr == 0 || (pool..end).contains(&name_ptr) {
            continue;
        }
        let name = live::string(memory, name_ptr)
            .with_context(|| format!("Invalid sound alias slot {slot}"))?;
        let variants = u32_at(&lists, slot * 12 + 8) as u64;
        ensure!(
            variants <= MAX_VARIANTS,
            "Sound alias {name} has {variants} variants"
        );
        let head = u32_at(&lists, slot * 12 + 4) as u64;
        let mut files = vec![];
        for index in 0..variants {
            let variant = memory.read(head + index * VARIANT_STRIDE, VARIANT_STRIDE as usize)?;
            ensure!(
                u32_at(&variant, 0) as u64 == name_ptr,
                "Sound alias {name}'s variant {index} names another alias; unknown layout"
            );
            let address = u32_at(&variant, 20) as u64;
            if address != 0
                && let Some(file) =
                    file(memory, address).with_context(|| format!("Sound alias {name}"))?
            {
                files.push(file);
            }
        }
        result.push(Alias { name, files });
    }
    tables.recheck()?;
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

/// A sound file's WAV bytes: loaded ones from memory, streamed ones from the IWDs (matched
/// ignoring case, as the game does).
pub fn read(memory: &impl Memory, file: &SoundFile, archives: &[Asset]) -> Result<Vec<u8>> {
    let bytes = match file {
        SoundFile::Loaded { address, size, .. } => memory.read(*address, *size as usize)?,
        SoundFile::Streamed { path } => {
            let member = format!("sound/{path}").to_lowercase();
            let asset = archives
                .iter()
                .find(|a| a.member.to_lowercase() == member)
                .with_context(|| format!("Streamed sound sound/{path} is not in the IWDs"))?;
            archive::read_asset(asset, archive::DEFAULT_LIMIT)?
        }
    };
    ensure!(
        extension(&bytes).is_some(),
        "{} is neither a RIFF WAV nor xWMA",
        file.name()
    );
    Ok(bytes)
}

/// The file extension for sound bytes from [`read`]: `wav`, or `xwm` for xWMA (WMA in RIFF,
/// which some loaded sounds are despite their `.wav` names).
pub fn extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" {
        return None;
    }
    match &bytes[8..12] {
        b"WAVE" => Some("wav"),
        b"XWMA" => Some("xwm"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: usize = 0x10000;

    /// Memory from BASE up.
    struct Flat(Vec<u8>);
    impl Flat {
        fn put(&mut self, address: usize, bytes: &[u8]) {
            self.0[address - BASE..address - BASE + bytes.len()].copy_from_slice(bytes);
        }
        fn word(&mut self, address: usize, value: usize) {
            self.put(address, &(value as u32).to_le_bytes());
        }
    }
    impl Memory for Flat {
        fn read(&self, address: u64, size: usize) -> Result<Vec<u8>> {
            let start = (address as usize).checked_sub(BASE).context("unmapped")?;
            self.0
                .get(start..start + size)
                .map(<[u8]>::to_vec)
                .context("unmapped fixture address")
        }
    }

    #[test]
    fn loaded_streamed_and_unknown_files() {
        let mut memory = Flat(vec![0; 0x1000]);
        memory.put(BASE + 0x100, &[1, 1]);
        memory.word(BASE + 0x104, BASE + 0x200);
        memory.word(BASE + 0x200, BASE + 0x300);
        memory.word(BASE + 0x204, BASE + 0x340);
        memory.word(BASE + 0x208, 16);
        memory.put(BASE + 0x300, b"fire.wav\0");
        memory.put(BASE + 0x340, b"RIFF\x08\0\0\0WAVEdata");
        memory.put(BASE + 0x180, &[2, 1]);
        memory.word(BASE + 0x188, BASE + 0x310);
        memory.word(BASE + 0x18c, BASE + 0x320);
        memory.put(BASE + 0x310, b"Stream\\music\0");
        memory.put(BASE + 0x320, b"song.wav\0");
        memory.put(BASE + 0x1c0, &[7]);

        let loaded = file(&memory, (BASE + 0x100) as u64).unwrap().unwrap();
        assert_eq!(loaded.name(), "fire.wav");
        assert_eq!(&read(&memory, &loaded, &[]).unwrap()[8..12], b"WAVE");
        let streamed = file(&memory, (BASE + 0x180) as u64).unwrap().unwrap();
        assert_eq!(streamed.name(), "Stream/music/song.wav");
        assert!(read(&memory, &streamed, &[]).is_err());
        assert!(file(&memory, (BASE + 0x1c0) as u64).is_err());
    }
}
