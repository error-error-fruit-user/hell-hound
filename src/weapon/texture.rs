use super::{u16_at, u32_at};
use anyhow::{Result, bail, ensure};
use image::{ImageEncoder, ImageFormat};

pub fn iwi_png(bytes: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        bytes.len() >= 28 && &bytes[..3] == b"IWi",
        "Invalid/truncated IWI image"
    );
    let (info, mips, header) = match bytes[3] {
        6 => (4, 12, 28),
        8 => (8, 16, 32),
        version => {
            bail!("Unsupported IWI version {version:#x}; not silently decoding an unknown format")
        }
    };
    ensure!(bytes.len() >= header, "Truncated IWI header");
    let format = bytes[info];
    let flags = bytes[info + 1];
    ensure!(
        flags & 4 == 0,
        "Cubemap IWI is not supported by the weapon texture decoder"
    );
    let width = u16_at(bytes, info + 2) as u32;
    let height = u16_at(bytes, info + 4) as u32;
    ensure!(
        width > 0 && height > 0 && u64::from(width) * u64::from(height) <= 16 * 1024 * 1024,
        "Invalid/excessive IWI dimensions"
    );
    let first = u32_at(bytes, mips) as usize;
    let second = u32_at(bytes, mips + 4) as usize;
    let last = u32_at(bytes, mips + 12) as usize;
    let offset = if first == second || first == last {
        header
    } else {
        second
    };
    ensure!(
        offset >= header && offset < bytes.len(),
        "Invalid high-resolution IWI mip offset"
    );
    let pixels = width as usize * height as usize;
    let rgba = match format {
        1 | 2 | 4 | 5 => {
            let channels = match format {
                1 => 4,
                2 => 3,
                _ => 1,
            };
            ensure!(
                offset + pixels * channels <= bytes.len(),
                "Truncated uncompressed IWI image"
            );
            let mut result = Vec::with_capacity(pixels * 4);
            for p in bytes[offset..offset + pixels * channels].chunks_exact(channels) {
                match format {
                    1 => result.extend_from_slice(&[p[2], p[1], p[0], p[3]]),
                    2 => result.extend_from_slice(&[p[2], p[1], p[0], 255]),
                    _ => result.extend_from_slice(&[255, 255, 255, p[0]]),
                }
            }
            result
        }
        0x0b..=0x0d => {
            let block = if format == 0x0b { 8 } else { 16 };
            let size = width.div_ceil(4) as usize * height.div_ceil(4) as usize * block;
            ensure!(
                offset + size <= bytes.len(),
                "Truncated block-compressed IWI image"
            );
            let mut dds = vec![0; 128];
            dds[..4].copy_from_slice(b"DDS ");
            let put = |out: &mut [u8], at, value: u32| {
                out[at..at + 4].copy_from_slice(&value.to_le_bytes())
            };
            for (at, value) in [
                (4, 124),
                (8, 0x81007),
                (12, height),
                (16, width),
                (20, size as u32),
                (28, 1),
                (76, 32),
                (80, 4),
                (108, 0x1000),
            ] {
                put(&mut dds, at, value);
            }
            dds[84..88].copy_from_slice(match format {
                0x0b => b"DXT1",
                0x0c => b"DXT3",
                _ => b"DXT5",
            });
            dds.extend_from_slice(&bytes[offset..offset + size]);
            image::load_from_memory_with_format(&dds, ImageFormat::Dds)?
                .to_rgba8()
                .into_raw()
        }
        _ => bail!("Unsupported IWI pixel format {format:#x}"),
    };
    let mut png = vec![];
    image::codecs::png::PngEncoder::new(&mut png).write_image(
        &rgba,
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bgra_to_png_and_reject_unknown_formats() {
        let mut iwi = vec![0; 36];
        iwi[..4].copy_from_slice(b"IWi\x08");
        iwi[8] = 1;
        iwi[10..12].copy_from_slice(&1u16.to_le_bytes());
        iwi[12..14].copy_from_slice(&1u16.to_le_bytes());
        iwi[32..].copy_from_slice(&[10, 20, 30, 255]);
        let png = iwi_png(&iwi).unwrap();
        let pixel = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(pixel.as_raw(), &[30, 20, 10, 255]);
        iwi[8] = 99;
        assert!(iwi_png(&iwi).is_err());
        iwi[8] = 1;
        iwi[9] = 4;
        assert!(iwi_png(&iwi).is_err());
    }
}
