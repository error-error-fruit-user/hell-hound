//! Original, read-only reading of WaW's loaded weapon definitions: the models, animations and
//! sound aliases a weapon uses and the numbers behind it. Layout facts (single player, measured
//! on the stock Steam build against the IWDs' text weapon files): weapon pool index 24, stride
//! 0x9ac; only the fields below are read, and a definition whose names don't resolve fails.

use crate::live::{self, Memory, Mode, Pools};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::BTreeMap;

const WEAPON_POOL: u64 = 24;
const STRIDE: u64 = 0x9ac;
const MODEL_POOL: u64 = 5;
const ALIAS_POOL: u64 = 9;

/// Animation fields by offset, named as in the weapon files (without `Anim`).
const ANIMATIONS: &[(usize, &str)] = &[
    (0x54, "idle"),
    (0x58, "emptyIdle"),
    (0x5c, "fire"),
    (0x64, "lastShot"),
    (0x68, "rechamber"),
    (0x6c, "melee"),
    (0x70, "meleeCharge"),
    (0x74, "reload"),
    (0x78, "reloadEmpty"),
    (0x7c, "reloadStart"),
    (0x80, "reloadEnd"),
    (0x84, "raise"),
    (0x88, "firstRaise"),
    (0x8c, "drop"),
    (0x90, "altRaise"),
    (0x94, "altDrop"),
    (0x98, "quickRaise"),
    (0x9c, "quickDrop"),
    (0xa0, "emptyRaise"),
    (0xa4, "emptyDrop"),
    (0xc8, "adsFire"),
    (0xcc, "adsLastShot"),
    (0xd0, "adsRechamber"),
    (0xd4, "adsUp"),
    (0xd8, "adsDown"),
];

/// Sound fields by offset, named as in the weapon files (without `Sound`); `Player` ones are
/// what the holder hears.
const SOUNDS: &[(usize, &str)] = &[
    (0x184, "pickup"),
    (0x188, "pickupPlayer"),
    (0x18c, "ammoPickup"),
    (0x190, "ammoPickupPlayer"),
    (0x194, "projectile"),
    (0x1a0, "fire"),
    (0x1a4, "firePlayer"),
    (0x1b8, "lastShot"),
    (0x1bc, "lastShotPlayer"),
    (0x1c0, "emptyFire"),
    (0x1c4, "emptyFirePlayer"),
    (0x1d0, "meleeSwipe"),
    (0x1d4, "meleeSwipePlayer"),
    (0x1d8, "meleeHit"),
    (0x1e0, "rechamber"),
    (0x1e4, "rechamberPlayer"),
    (0x1e8, "reload"),
    (0x1ec, "reloadPlayer"),
    (0x1f0, "reloadEmpty"),
    (0x1f4, "reloadEmptyPlayer"),
    (0x1f8, "reloadStart"),
    (0x1fc, "reloadStartPlayer"),
    (0x200, "reloadEnd"),
    (0x204, "reloadEndPlayer"),
    (0x250, "raise"),
    (0x254, "raisePlayer"),
    (0x258, "firstRaise"),
    (0x25c, "firstRaisePlayer"),
    (0x260, "putaway"),
    (0x264, "putawayPlayer"),
];

/// Whole-number fields by offset; times are milliseconds.
const NUMBERS: &[(usize, &str)] = &[
    (0x3ec, "startAmmo"),
    (0x404, "maxAmmo"),
    (0x408, "clipSize"),
    (0x40c, "shotCount"),
    (0x420, "damage"),
    (0x448, "fireTime"),
    (0x464, "reloadTime"),
    (0x46c, "reloadEmptyTime"),
    (0x470, "reloadAddTime"),
    (0x478, "reloadStartTime"),
    (0x47c, "reloadStartAddTime"),
    (0x480, "reloadEndTime"),
    (0x484, "dropTime"),
    (0x488, "raiseTime"),
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponDef {
    pub name: String,
    pub display_name: String,
    /// bullet, grenade, projectile, binoculars or gas
    pub weapon_type: &'static str,
    /// Full Auto or Single Shot, as the weapon files write it
    pub fire_type: &'static str,
    pub gun_model: Option<String>,
    pub hand_model: Option<String>,
    pub animations: BTreeMap<&'static str, String>,
    pub sounds: BTreeMap<&'static str, String>,
    pub numbers: BTreeMap<&'static str, i32>,
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated fixed layout"),
    )
}

/// The name of the asset at `pointer` when it lies in the pool `range` (assets start with
/// their name), or None for a null pointer.
fn pooled_name(memory: &impl Memory, pointer: u32, range: (u64, u64)) -> Result<Option<String>> {
    if pointer == 0 {
        return Ok(None);
    }
    let pointer = pointer as u64;
    ensure!(
        (range.0..range.1).contains(&pointer),
        "Weapon field points outside its asset pool"
    );
    Ok(Some(live::string(memory, live::word(memory, pointer)?)?))
}

/// Every loaded weapon definition, in pool order. Only single player's layout is known.
pub fn list(memory: &impl Memory, mode: Mode, image_base: u64) -> Result<Vec<WeaponDef>> {
    ensure!(
        matches!(mode, Mode::Sp),
        "Weapon definitions are only decoded in single player (zombies)"
    );
    let tables = Pools::open(memory, mode, image_base)?;
    let (pool, count) = tables.pool(WEAPON_POOL, "weapon")?;
    let span = |index, stride, kind| -> Result<(u64, u64)> {
        let (start, count) = tables.pool(index, kind)?;
        Ok((start, start + 4 + count * stride))
    };
    let models = span(MODEL_POOL, 228, "model")?;
    let aliases = span(ALIAS_POOL, 12, "sound alias")?;
    let first = pool + 4;
    let end = first + count * STRIDE;
    let mut result = vec![];
    for slot in 0..count {
        let entry = memory.read(first + slot * STRIDE, STRIDE as usize)?;
        let name_ptr = u32_at(&entry, 0) as u64;
        if name_ptr == 0 || (pool..end).contains(&name_ptr) {
            continue;
        }
        let name = live::string(memory, name_ptr)
            .with_context(|| format!("Invalid weapon slot {slot}"))?;
        let context = || format!("Weapon {name}");
        // Unset text fields are null or empty.
        let text = |offset: usize| -> Result<Option<String>> {
            match u32_at(&entry, offset) {
                0 => Ok(None),
                pointer if memory.read(pointer as u64, 1)?[0] == 0 => Ok(None),
                pointer => live::string(memory, pointer as u64).map(Some),
            }
        };
        let mut animations = BTreeMap::new();
        for (offset, role) in ANIMATIONS {
            if let Some(clip) = text(*offset).with_context(context)? {
                animations.insert(*role, clip);
            }
        }
        let mut sounds = BTreeMap::new();
        for (offset, role) in SOUNDS {
            if let Some(alias) =
                pooled_name(memory, u32_at(&entry, *offset), aliases).with_context(context)?
            {
                sounds.insert(*role, alias);
            }
        }
        let numbers = NUMBERS
            .iter()
            .map(|(offset, field)| (*field, u32_at(&entry, *offset) as i32))
            .collect();
        let weapon_type = match u32_at(&entry, 0x144) {
            0 => "bullet",
            1 => "grenade",
            2 => "projectile",
            3 => "binoculars",
            4 => "gas",
            other => anyhow::bail!("Weapon {name} has unknown type {other}"),
        };
        let fire_type = match u32_at(&entry, 0x158) {
            0 => "Full Auto",
            1 => "Single Shot",
            2 => "2-Round Burst",
            3 => "3-Round Burst",
            4 => "4-Round Burst",
            other => anyhow::bail!("Weapon {name} has unknown fire type {other}"),
        };
        result.push(WeaponDef {
            display_name: text(4).with_context(context)?.unwrap_or_default(),
            gun_model: pooled_name(memory, u32_at(&entry, 0xc), models).with_context(context)?,
            hand_model: pooled_name(memory, u32_at(&entry, 0x4c), models).with_context(context)?,
            name,
            weapon_type,
            fire_type,
            animations,
            sounds,
            numbers,
        });
    }
    tables.recheck()?;
    Ok(result)
}
