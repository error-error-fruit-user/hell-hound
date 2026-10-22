//! Original, read-only WaW asset-pool inspection. This is NOT model/anim export.
//! Layout facts: 32-bit pointers, pool indices 4/5, strides 88/228, and stock
//! SP/MP table addresses documented by public WaW reverse-engineering research.
//! No upstream implementation is included. Unknown layouts fail closed.

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

const READ_LIMIT: usize = 16 * 1024 * 1024;
const DEFAULT_IMAGE_BASE: u64 = 0x400000;

pub trait Memory {
    fn read(&self, address: u64, size: usize) -> Result<Vec<u8>>;
}

#[cfg(target_os = "linux")]
pub struct Process {
    file: std::fs::File,
}

#[cfg(target_os = "linux")]
impl Process {
    pub fn open(pid: u32) -> Result<Self> {
        ensure!(pid > 0, "PID must be positive");
        let file = std::fs::File::open(format!("/proc/{pid}/mem")).with_context(||
            format!("Cannot read PID {pid}. Linux process-access policy may deny unrelated processes. No permissions were changed; do not run as root or disable ptrace policy blindly. An alternative to test is the Windows build inside this game's existing Proton environment; see README"))?;
        Ok(Self { file })
    }
}

#[cfg(target_os = "linux")]
impl Memory for Process {
    fn read(&self, address: u64, size: usize) -> Result<Vec<u8>> {
        use std::os::unix::fs::FileExt;
        ensure!(size <= READ_LIMIT, "Memory read exceeds limit");
        address
            .checked_add(size as u64)
            .context("Address overflow")?;
        let mut bytes = vec![0; size];
        let mut offset = 0;
        while offset < size {
            let count = self
                .file
                .read_at(&mut bytes[offset..], address + offset as u64)
                .with_context(|| {
                    format!("Cannot read process address {:#x}", address + offset as u64)
                })?;
            ensure!(
                count > 0,
                "Short process-memory read at {address:#x}; game may have exited"
            );
            offset += count;
        }
        Ok(bytes)
    }
}

#[cfg(target_os = "windows")]
pub struct Process {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(target_os = "windows")]
impl Process {
    pub fn open(pid: u32) -> Result<Self> {
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
        };
        ensure!(pid > 0, "PID must be positive");
        // SAFETY: read/query rights only; no memory writes, injection, or privilege changes.
        let handle = unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error())
                .context("Cannot open game process for read-only access");
        }
        Ok(Self { handle })
    }
}

#[cfg(target_os = "windows")]
impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: this object owns the valid handle returned by OpenProcess.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(target_os = "windows")]
impl Memory for Process {
    fn read(&self, address: u64, size: usize) -> Result<Vec<u8>> {
        use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
        ensure!(size <= READ_LIMIT, "Memory read exceeds limit");
        address
            .checked_add(size as u64)
            .context("Address overflow")?;
        let address =
            usize::try_from(address).context("Address does not fit this process architecture")?;
        let mut bytes = vec![0; size];
        let mut count = 0;
        // SAFETY: the OS checks the remote address; the destination is an allocated
        // buffer of exactly size bytes. Handle is owned for this call's duration.
        let success = unsafe {
            ReadProcessMemory(
                self.handle,
                address as *const _,
                bytes.as_mut_ptr().cast(),
                size,
                &mut count,
            )
        };
        if success == 0 {
            return Err(std::io::Error::last_os_error()).context("Cannot read game memory");
        }
        ensure!(
            count == size,
            "Short process-memory read; game may have exited"
        );
        Ok(bytes)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub struct Process;

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
impl Process {
    pub fn open(_pid: u32) -> Result<Self> {
        bail!("Live inspection currently targets Windows and Linux")
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
impl Memory for Process {
    fn read(&self, _address: u64, _size: usize) -> Result<Vec<u8>> {
        bail!("Unsupported platform")
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated fixed layout"),
    )
}

pub(crate) fn word(memory: &impl Memory, address: u64) -> Result<u64> {
    Ok(u32_at(&memory.read(address, 4)?, 0).into())
}

pub(crate) fn string(memory: &impl Memory, address: u64) -> Result<String> {
    ensure!(
        (0x10000..=u32::MAX as u64).contains(&address),
        "Invalid 32-bit name pointer: {address:#x}"
    );
    let mut bytes = vec![];
    for offset in 0..1024 {
        let byte = memory.read(address + offset, 1)?[0];
        if byte == 0 {
            let text = String::from_utf8(bytes).context("Asset name is not UTF-8")?;
            ensure!(
                !text.is_empty() && !text.chars().any(char::is_control),
                "Invalid asset name"
            );
            return Ok(text);
        }
        bytes.push(byte);
    }
    bail!("Unterminated asset name exceeds 1024 bytes")
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum Mode {
    Sp,
    Mp,
}

#[derive(Debug, Clone, Serialize)]
pub struct LiveAsset {
    pub kind: &'static str,
    pub name: String,
    pub address: u64,
    pub bones: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frames: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<f32>,
}

/// The validated asset-pool tables: each pool's address and capacity by asset type.
pub(crate) struct Pools<'a, M: Memory> {
    memory: &'a M,
    pools: u64,
    sizes: u64,
    models: u64,
}

impl<'a, M: Memory> Pools<'a, M> {
    /// Finds the stock tables for `mode` and checks the image is 32-bit WaW with a map loaded.
    pub(crate) fn open(memory: &'a M, mode: Mode, image_base: u64) -> Result<Self> {
        ensure!(
            image_base <= u32::MAX as u64,
            "WaW requires a 32-bit image base"
        );
        let relocate = |address: u64| -> Result<u64> {
            image_base
                .checked_add(address - DEFAULT_IMAGE_BASE)
                .context("Relocated address overflow")
        };
        // Verify a PE32/x86 image, not an unrelated process or a 64-bit CoD build.
        let dos = memory.read(image_base, 64)?;
        ensure!(
            &dos[..2] == b"MZ",
            "No PE image at --image-base; select the actual CoDWaW.exe process/base"
        );
        let pe_offset = u32_at(&dos, 0x3c);
        ensure!(pe_offset <= 1024 * 1024, "Invalid PE header offset");
        let pe = memory.read(image_base + pe_offset as u64, 26)?;
        ensure!(
            &pe[..4] == b"PE\0\0" && pe[4..6] == [0x4c, 0x01] && pe[24..26] == [0x0b, 0x01],
            "Expected a 32-bit x86 PE image"
        );
        let (pools, sizes) = match mode {
            Mode::Sp => (relocate(0x8dc828)?, relocate(0x8dc5d0)?),
            Mode::Mp => (relocate(0x8d0958)?, relocate(0x8d06e8)?),
        };
        let models = word(memory, pools + 5 * 4)?;
        let sentinel = string(memory, word(memory, models + 4)?).context(
            "Cannot validate stock WaW asset pools; unsupported build/mode/base or map not loaded",
        )?;
        ensure!(
            ["void", "defaultactor", "defaultweapon"].contains(&sentinel.as_str()),
            "Unrecognized WaW model-pool sentinel {sentinel:?}; refusing to decode unknown layout"
        );
        Ok(Self {
            memory,
            pools,
            sizes,
            models,
        })
    }

    /// Pool `index`'s address and capacity, `kind` naming it in errors.
    pub(crate) fn pool(&self, index: u64, kind: &str) -> Result<(u64, u64)> {
        let pool = word(self.memory, self.pools + index * 4)?;
        let count = word(self.memory, self.sizes + index * 4)?;
        ensure!(
            (0x10000..=u32::MAX as u64).contains(&pool) && (1..=50000).contains(&count),
            "Invalid or excessive {kind} pool"
        );
        Ok((pool, count))
    }

    /// Fails if the pools moved since `open`; live reads are not a coherent snapshot during a
    /// map change.
    pub(crate) fn recheck(&self) -> Result<()> {
        ensure!(
            word(self.memory, self.pools + 5 * 4)? == self.models,
            "Game asset pools changed during inspection; retry on a stable map"
        );
        Ok(())
    }
}

pub fn list(memory: &impl Memory, mode: Mode, image_base: u64) -> Result<Vec<LiveAsset>> {
    let tables = Pools::open(memory, mode, image_base)?;
    let mut result = vec![];
    for (index, stride, kind) in [(4, 88, "animation"), (5, 228, "model")] {
        let (pool, count) = tables.pool(index, kind)?;
        let first = pool + 4;
        let end = first + count * stride;
        ensure!(end <= u32::MAX as u64, "Pool exceeds 32-bit address space");
        for slot in 0..count {
            let address = first + slot * stride;
            let header = memory.read(address, stride as usize)?;
            let name_ptr = u32_at(&header, 0) as u64;
            if name_ptr == 0 || (pool..end).contains(&name_ptr) {
                continue;
            }
            let name = string(memory, name_ptr).with_context(|| {
                format!("Invalid {kind} slot {slot}; stop the game/map changing and retry")
            })?;
            if ["void", "defaultactor", "defaultweapon"].contains(&name.as_str()) {
                continue;
            }
            let (bones, frames, fps) = if kind == "model" {
                (header[4] as u16, None, None)
            } else {
                let frames = u16::from_le_bytes(header[14..16].try_into()?);
                let fps = f32::from_le_bytes(header[40..44].try_into()?);
                ensure!(
                    fps.is_finite() && fps > 0.0 && fps <= 1000.0,
                    "Invalid animation frame rate: {name}"
                );
                (header[27] as u16, Some(frames), Some(fps))
            };
            result.push(LiveAsset {
                kind,
                name,
                address,
                bones,
                frames,
                fps,
            });
        }
    }
    tables.recheck()?;
    result.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Snapshot(BTreeMap<u64, u8>);
    impl Snapshot {
        fn put(&mut self, address: u64, data: &[u8]) {
            for (offset, byte) in data.iter().enumerate() {
                self.0.insert(address + offset as u64, *byte);
            }
        }
        fn word(&mut self, address: u64, value: u32) {
            self.put(address, &value.to_le_bytes());
        }
    }
    impl Memory for Snapshot {
        fn read(&self, address: u64, size: usize) -> Result<Vec<u8>> {
            (0..size)
                .map(|i| {
                    self.0
                        .get(&(address + i as u64))
                        .copied()
                        .context("unmapped fixture address")
                })
                .collect()
        }
    }

    fn fixture() -> Snapshot {
        let mut memory = Snapshot::default();
        memory.put(DEFAULT_IMAGE_BASE, &[0; 256]);
        memory.put(DEFAULT_IMAGE_BASE, b"MZ");
        memory.word(DEFAULT_IMAGE_BASE + 0x3c, 128);
        memory.put(DEFAULT_IMAGE_BASE + 128, b"PE\0\0\x4c\x01");
        memory.put(DEFAULT_IMAGE_BASE + 128 + 24, &[0x0b, 0x01]);
        memory.word(0x8dc828 + 4 * 4, 0x100000);
        memory.word(0x8dc828 + 5 * 4, 0x200000);
        memory.word(0x8dc5d0 + 4 * 4, 1);
        memory.word(0x8dc5d0 + 5 * 4, 2);
        memory.put(0x100004, &[0; 88]);
        memory.word(0x100004, 0x300000);
        memory.put(0x300000, b"viewmodel_colt_fire\0");
        memory.put(0x100004 + 14, &12u16.to_le_bytes());
        memory.put(0x100004 + 27, &[3]);
        memory.put(0x100004 + 40, &30.0f32.to_le_bytes());
        memory.put(0x200004, &[0; 456]);
        memory.word(0x200004, 0x300100);
        memory.put(0x300100, b"void\0");
        memory.word(0x200004 + 228, 0x300200);
        memory.put(0x300200, b"viewmodel_colt\0");
        memory.put(0x200004 + 228 + 4, &[3]);
        memory
    }

    #[test]
    fn names_and_metadata_from_synthetic_pools() {
        let assets = list(&fixture(), Mode::Sp, DEFAULT_IMAGE_BASE).unwrap();
        assert_eq!(assets.len(), 2);
        assert_eq!(assets[0].name, "viewmodel_colt_fire");
        assert_eq!(assets[0].frames, Some(12));
        assert_eq!(assets[0].fps, Some(30.0));
        assert_eq!(assets[1].bones, 3);
    }

    #[test]
    fn unknown_build_or_wrong_process_fails_closed() {
        let mut memory = fixture();
        memory.put(0x300100, b"unknown\0");
        assert!(list(&memory, Mode::Sp, DEFAULT_IMAGE_BASE).is_err());
        assert!(list(&fixture(), Mode::Sp, 0x500000).is_err());
    }

    #[test]
    fn excessive_counts_and_invalid_rates_fail() {
        let mut memory = fixture();
        memory.word(0x8dc5d0 + 4 * 4, 50001);
        assert!(list(&memory, Mode::Sp, DEFAULT_IMAGE_BASE).is_err());
        let mut memory = fixture();
        memory.put(0x100004 + 40, &f32::NAN.to_le_bytes());
        assert!(list(&memory, Mode::Sp, DEFAULT_IMAGE_BASE).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn native_backend_reads_own_process_without_privilege_changes() {
        let bytes = [13u8, 27, 41, 59];
        let process = Process::open(std::process::id()).unwrap();
        assert_eq!(
            process.read(bytes.as_ptr() as u64, bytes.len()).unwrap(),
            bytes
        );
        assert!(process.read(bytes.as_ptr() as u64, READ_LIMIT + 1).is_err());
    }
}
