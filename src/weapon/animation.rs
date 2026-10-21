use super::{Cursor, bone_name, f32_at, u16_at, u32_at};
use crate::live::{self, Memory, Mode};
use anyhow::{Result, ensure};
use glam::{Quat, Vec3};

#[derive(Debug, Clone)]
pub struct Keys<T> {
    pub values: Vec<(u16, T)>,
}
#[derive(Debug, Clone, Default)]
pub struct Track {
    pub name: String,
    pub rotations: Vec<(u16, Quat)>,
    pub translations: Vec<(u16, Vec3)>,
}
#[derive(Debug, Clone)]
pub struct Clip {
    pub name: String,
    pub frames: u16,
    pub fps: f32,
    pub looping: bool,
    pub viewmodel: bool,
    pub tracks: Vec<Track>,
    pub events: Vec<(String, f32)>,
}

fn rotation(bytes: &[u8], planar: bool) -> Result<Quat> {
    let component = |index| {
        i16::from_le_bytes(
            bytes[index..index + 2]
                .try_into()
                .expect("rotation component"),
        ) as f32
            / 32768.0
    };
    let q = if planar {
        Quat::from_xyzw(0.0, 0.0, component(0), component(2))
    } else {
        Quat::from_xyzw(component(0), component(2), component(4), component(6))
    };
    ensure!(
        q.is_finite() && q.length_squared() > 0.1,
        "Invalid packed animation quaternion"
    );
    Ok(q.normalize())
}

fn frame<M: Memory>(
    bytes: &mut Cursor<'_, M>,
    shorts: &mut Cursor<'_, M>,
    indices: &mut Cursor<'_, M>,
    frames: u16,
    count: u16,
) -> Result<u16> {
    if frames <= 255 {
        Ok(bytes.byte()? as u16)
    } else if count >= 64 && indices.position != 0 {
        indices.short()
    } else {
        shorts.short()
    }
}

fn skip_index_table<M: Memory>(shorts: &mut Cursor<'_, M>, frames: u16, count: u16) -> Result<()> {
    if frames > 255 && count >= 64 {
        shorts.bytes((((count as usize - 1) >> 8) + 2) * 2)?;
    }
    Ok(())
}

fn valid_keys<T>(keys: &[(u16, T)], frames: u16) -> Result<()> {
    ensure!(
        keys.iter().all(|(f, _)| *f <= frames),
        "Animation frame index exceeds clip length"
    );
    ensure!(
        keys.windows(2).all(|p| p[0].0 < p[1].0),
        "Animation key frames are not strictly increasing"
    );
    Ok(())
}

pub fn decode(memory: &impl Memory, mode: Mode, base: u64, address: u64) -> Result<Clip> {
    let header = memory.read(address, 88)?;
    let name = live::string(memory, u32_at(&header, 0) as u64)?;
    ensure!(name != "void", "Placeholder animation cannot be exported");
    let frames = u16_at(&header, 14);
    let fps = f32_at(&header, 40);
    ensure!(
        fps.is_finite() && fps > 0.0 && fps <= 1000.0,
        "Invalid animation frame rate"
    );
    ensure!(
        header[17] == 0,
        "Delta/root-motion animation is not supported yet; no partial animation will be exported"
    );
    let delta = u32_at(&header, 84);
    if delta != 0 {
        let parts = memory.read(delta as u64, 8)?;
        ensure!(
            u32_at(&parts, 0) == 0 && u32_at(&parts, 4) == 0,
            "Delta/root-motion channels are not supported yet"
        );
    }
    let bones = header[27] as usize;
    ensure!(
        bones > 0 && header[18..23].iter().map(|v| *v as usize).sum::<usize>() == bones,
        "Animation rotation categories do not match bone count"
    );
    ensure!(
        header[23..27].iter().map(|v| *v as usize).sum::<usize>() <= bones,
        "Invalid translation categories"
    );
    let ids = memory.read(u32_at(&header, 48) as u64, bones * 2)?;
    let mut tracks = vec![];
    for index in 0..bones {
        let name = bone_name(memory, mode, base, u16_at(&ids, index * 2))?;
        ensure!(
            !tracks.iter().any(|t: &Track| t.name == name),
            "Duplicate animation bone name: {name}"
        );
        tracks.push(Track {
            name,
            ..Track::default()
        });
    }
    let mut bytes = Cursor::new(memory, u32_at(&header, 52));
    let mut shorts = Cursor::new(memory, u32_at(&header, 56));
    let mut ints = Cursor::new(memory, u32_at(&header, 60));
    let mut random_shorts = Cursor::new(memory, u32_at(&header, 64));
    let mut random_bytes = Cursor::new(memory, u32_at(&header, 68));
    let mut indices = Cursor::new(memory, u32_at(&header, 76));
    let mut bone = 0;
    for _ in 0..header[18] {
        tracks[bone].rotations.push((0, Quat::IDENTITY));
        bone += 1;
    }
    for category in [19, 20] {
        let planar = category == 19;
        for _ in 0..header[category] {
            let count = shorts.short()?;
            ensure!(count <= frames, "Rotation key count exceeds clip frames");
            skip_index_table(&mut shorts, frames, count)?;
            let width = if planar { 4 } else { 8 };
            let data = random_shorts.bytes((count as usize + 1) * width)?;
            for key in 0..=count as usize {
                let f = frame(&mut bytes, &mut shorts, &mut indices, frames, count)?;
                tracks[bone]
                    .rotations
                    .push((f, rotation(&data[key * width..(key + 1) * width], planar)?));
            }
            bone += 1;
        }
    }
    for category in [21, 22] {
        let planar = category == 21;
        for _ in 0..header[category] {
            let data = shorts.bytes(if planar { 4 } else { 8 })?;
            tracks[bone].rotations.push((0, rotation(&data, planar)?));
            bone += 1;
        }
    }
    for category in [23, 24] {
        let precise = category == 24;
        for _ in 0..header[category] {
            let target = bytes.byte()? as usize;
            ensure!(
                target < bones && tracks[target].translations.is_empty(),
                "Invalid/duplicate translated bone index"
            );
            let count = shorts.short()?;
            ensure!(count <= frames, "Translation key count exceeds clip frames");
            skip_index_table(&mut shorts, frames, count)?;
            let minimum = ints.vector()?;
            let scale = ints.vector()?;
            let width = if precise { 6 } else { 3 };
            let data = if precise {
                random_shorts.bytes((count as usize + 1) * width)?
            } else {
                random_bytes.bytes((count as usize + 1) * width)?
            };
            for key in 0..=count as usize {
                let value = if precise {
                    Vec3::new(
                        u16_at(&data, key * 6) as f32,
                        u16_at(&data, key * 6 + 2) as f32,
                        u16_at(&data, key * 6 + 4) as f32,
                    )
                } else {
                    Vec3::new(
                        data[key * 3] as f32,
                        data[key * 3 + 1] as f32,
                        data[key * 3 + 2] as f32,
                    )
                };
                let f = frame(&mut bytes, &mut shorts, &mut indices, frames, count)?;
                let translation = minimum + scale * value;
                ensure!(translation.is_finite(), "Non-finite animation translation");
                tracks[target].translations.push((f, translation));
            }
        }
    }
    for _ in 0..header[25] {
        let value = ints.vector()?;
        let target = bytes.byte()? as usize;
        ensure!(
            target < bones && tracks[target].translations.is_empty(),
            "Invalid/duplicate static translated bone"
        );
        tracks[target].translations.push((0, value));
    }
    for track in &mut tracks {
        valid_keys(&track.rotations, frames)?;
        valid_keys(&track.translations, frames)?;
        // glTF interpolates shortest arcs; make quaternion signs continuous.
        for index in 1..track.rotations.len() {
            if track.rotations[index - 1].1.dot(track.rotations[index].1) < 0.0 {
                track.rotations[index].1 = -track.rotations[index].1;
            }
        }
    }
    let mut events = vec![];
    let notifications = u32_at(&header, 80) as u64;
    for index in 0..header[28] {
        let item = memory.read(notifications + index as u64 * 8, 8)?;
        let time = f32_at(&item, 4);
        ensure!(
            time.is_finite() && (0.0..=1.0).contains(&time),
            "Invalid animation notification time"
        );
        events.push((
            bone_name(memory, mode, base, u16_at(&item, 0))?,
            time * frames as f32 / fps,
        ));
    }
    Ok(Clip {
        viewmodel: name.starts_with("viewmodel_"),
        name,
        frames,
        fps,
        looping: header[16] != 0,
        tracks,
        events,
    })
}
