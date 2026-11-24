//! Shared raster image and PNG writer.

use std::fs::File;
use std::io::{BufWriter, Cursor};
use std::path::Path;

use scene_core::{MAX_DECAL_PIXELS, MAX_DECAL_SIDE};

/// Rendered RGBA image.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let index = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.rgba[index],
            self.rgba[index + 1],
            self.rgba[index + 2],
            self.rgba[index + 3],
        ]
    }
}

/// Pixel rectangle `[x, y, width, height]` used to clip [`composite_over`].
pub type Rect = [u32; 4];

/// Draw `src` over `dst` with its top-left pixel at `(x, y)`.
///
/// Uses straight-alpha "over": a pixel copied onto an empty background keeps
/// its alpha, so soft shadows and antialiasing stay translucent. Nothing is
/// written outside `clip` (itself clamped to `dst`).
pub fn composite_over(dst: &mut Image, src: &Image, x: i64, y: i64, clip: Rect) {
    let left = i64::from(clip[0]);
    let top = i64::from(clip[1]);
    let right = (left + i64::from(clip[2])).min(i64::from(dst.width));
    let bottom = (top + i64::from(clip[3])).min(i64::from(dst.height));
    for sy in 0..src.height {
        let ty = y + i64::from(sy);
        if ty < top || ty >= bottom {
            continue;
        }
        for sx in 0..src.width {
            let tx = x + i64::from(sx);
            if tx < left || tx >= right {
                continue;
            }
            let (Ok(tx), Ok(ty)) = (usize::try_from(tx), usize::try_from(ty)) else {
                continue;
            };
            let source = (sy as usize * src.width as usize + sx as usize) * 4;
            let target = (ty * dst.width as usize + tx) * 4;
            let (Some(from), Some(to)) = (
                src.rgba.get(source..source + 4),
                dst.rgba.get(target..target + 4),
            ) else {
                continue;
            };
            let blended = over(
                [from[0], from[1], from[2], from[3]],
                [to[0], to[1], to[2], to[3]],
            );
            dst.rgba[target..target + 4].copy_from_slice(&blended);
        }
    }
}

/// Straight-alpha `src` over `dst`.
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn over(src: [u8; 4], dst: [u8; 4]) -> [u8; 4] {
    let sa = u32::from(src[3]);
    let da = u32::from(dst[3]);
    if sa == 0 {
        return dst;
    }
    if sa == 255 || da == 0 {
        return src;
    }
    // Alphas scaled by 255: out_a = sa + da * (1 - sa).
    let dst_weight = da * (255 - sa);
    let out_a = sa * 255 + dst_weight;
    let mut out = [0; 4];
    for channel in 0..3 {
        let colour = u32::from(src[channel]) * sa * 255 + u32::from(dst[channel]) * dst_weight;
        out[channel] = ((colour + out_a / 2) / out_a) as u8;
    }
    out[3] = ((out_a + 127) / 255) as u8;
    out
}

/// Decode an RGBA PNG, expanding palette and low-bit-depth images.
///
/// # Errors
///
/// Returns a human-readable message if the file is missing, malformed or
/// larger than the decal limits in [`scene_core`].
pub fn read_png(path: &Path) -> Result<Image, String> {
    let bytes = scene_fs::read(path)
        .map_err(|cause| format!("cannot open `{}`: {cause}", path.display()))?;
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(
        png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
    );
    let mut reader = decoder
        .read_info()
        .map_err(|cause| format!("cannot read PNG `{}`: {cause}", path.display()))?;
    let (width, height) = reader.info().size();
    if width > MAX_DECAL_SIDE
        || height > MAX_DECAL_SIDE
        || u64::from(width) * u64::from(height) > MAX_DECAL_PIXELS
    {
        return Err(format!(
            "PNG `{}` is {width}x{height}; the decal limit is {MAX_DECAL_SIDE} per side and {MAX_DECAL_PIXELS} pixels",
            path.display()
        ));
    }
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|cause| format!("cannot decode PNG `{}`: {cause}", path.display()))?;
    let pixels = &buffer[..info.buffer_size()];
    let mut rgba = Vec::with_capacity((width as usize) * (height as usize) * 4);
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(pixels),
        png::ColorType::Rgb => {
            for pixel in pixels.chunks_exact(3) {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for pixel in pixels.chunks_exact(2) {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
        }
        png::ColorType::Grayscale => {
            for &value in pixels {
                rgba.extend_from_slice(&[value, value, value, 255]);
            }
        }
        png::ColorType::Indexed => {
            return Err(format!(
                "PNG `{}` is still indexed after expansion",
                path.display()
            ));
        }
    }
    Ok(Image {
        width,
        height,
        rgba,
    })
}

/// Encode an image as an RGBA PNG.
///
/// # Errors
///
/// Returns a human-readable message if the file cannot be written.
pub fn write_png(image: &Image, path: &Path) -> Result<(), String> {
    let file = File::create(path)
        .map_err(|cause| format!("cannot create `{}`: {cause}", path.display()))?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    // Fast deflate keeps encoding cheap; the Paeth row filter suits flat
    // pixel-art areas and makes files about a third smaller than the default
    // filter at the same speed.
    encoder.set_compression(png::Compression::Fast);
    encoder.set_filter(png::FilterType::Paeth);
    let mut writer = encoder
        .write_header()
        .map_err(|cause| format!("cannot write PNG header: {cause}"))?;
    writer
        .write_image_data(&image.rgba)
        .map_err(|cause| format!("cannot write PNG data: {cause}"))?;
    writer
        .finish()
        .map_err(|cause| format!("cannot finish PNG: {cause}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, pixel: [u8; 4]) -> Image {
        Image {
            width,
            height,
            rgba: pixel.repeat((width * height) as usize),
        }
    }

    #[test]
    fn translucent_pixel_on_empty_background_keeps_its_alpha() {
        let mut dst = image(1, 1, [0, 0, 0, 0]);
        composite_over(&mut dst, &image(1, 1, [10, 20, 30, 92]), 0, 0, [0, 0, 1, 1]);
        assert_eq!(dst.pixel(0, 0), [10, 20, 30, 92]);
    }

    #[test]
    fn translucent_over_opaque_blends_and_stays_opaque() {
        let mut dst = image(1, 1, [200, 200, 200, 255]);
        composite_over(&mut dst, &image(1, 1, [0, 0, 0, 128]), 0, 0, [0, 0, 1, 1]);
        let [r, g, b, a] = dst.pixel(0, 0);
        assert_eq!(a, 255);
        assert!((99..=101).contains(&r) && r == g && g == b, "got {r}");
    }

    #[test]
    fn translucent_over_translucent_accumulates_alpha() {
        let mut dst = image(1, 1, [255, 0, 0, 128]);
        composite_over(&mut dst, &image(1, 1, [0, 0, 255, 128]), 0, 0, [0, 0, 1, 1]);
        let [r, _, b, a] = dst.pixel(0, 0);
        assert!((191..=192).contains(&a), "alpha {a}");
        assert!(b > r, "source dominates: r={r} b={b}");
    }

    #[test]
    fn nothing_is_written_outside_the_clip() {
        let mut dst = image(4, 1, [0, 0, 0, 0]);
        composite_over(&mut dst, &image(4, 1, [9, 9, 9, 255]), -1, 0, [1, 0, 2, 1]);
        assert_eq!(dst.pixel(0, 0)[3], 0);
        assert_eq!(dst.pixel(1, 0)[3], 255);
        assert_eq!(dst.pixel(2, 0)[3], 255);
        assert_eq!(dst.pixel(3, 0)[3], 0);
    }
}
