//! Screen-space bloom for emissive faces and decals.
//!
//! The rasterizer writes a per-pixel glow mask; this pass blurs the matching
//! image colours and adds the result back. It is deterministic and does not
//! touch pixels that are not near a glowing surface.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::manual_slice_fill,
    clippy::needless_range_loop,
    clippy::too_many_lines
)]

use crate::image::Image;

/// Apply a box-blur glow to the image.
///
/// `glow` is parallel to the RGBA pixels and holds 0..=255. The pass only
/// allocates for the bounding box of glowing pixels, so a small screen does
/// not pay for the whole canvas.
pub fn apply(image: &mut Image, glow: &[u8], radius: usize, strength: f64) {
    let width = image.width as usize;
    let height = image.height as usize;
    if radius == 0 || strength <= 0.0 || width == 0 || height == 0 {
        return;
    }
    if glow.len() != width * height {
        return;
    }

    let mut left = width;
    let mut top = height;
    let mut right = 0;
    let mut bottom = 0;
    for y in 0..height {
        for x in 0..width {
            if glow[y * width + x] > 0 {
                left = left.min(x);
                right = right.max(x + 1);
                top = top.min(y);
                bottom = bottom.max(y + 1);
            }
        }
    }
    if left >= right || top >= bottom {
        return;
    }
    let pad = radius;
    left = left.saturating_sub(pad);
    top = top.saturating_sub(pad);
    right = (right + pad).min(width);
    bottom = (bottom + pad).min(height);
    let wide = right - left;
    let high = bottom - top;
    if wide == 0 || high == 0 {
        return;
    }

    let mut source = vec![[0_u32; 3]; wide * high];
    for y in top..bottom {
        for x in left..right {
            let glow = glow[y * width + x];
            if glow == 0 {
                continue;
            }
            let pixel = (y * width + x) * 4;
            let weight = u32::from(glow);
            source[(y - top) * wide + (x - left)] = [
                u32::from(image.rgba[pixel]) * weight / 255,
                u32::from(image.rgba[pixel + 1]) * weight / 255,
                u32::from(image.rgba[pixel + 2]) * weight / 255,
            ];
        }
    }

    let mut horizontal = vec![[0_u32; 3]; wide * high];
    for y in 0..high {
        let row = y * wide;
        let mut sum = [0_u32; 3];
        for x in 0..=radius.min(wide - 1) {
            let value = source[row + x];
            sum[0] += value[0];
            sum[1] += value[1];
            sum[2] += value[2];
        }
        for x in 0..wide {
            horizontal[row + x] = sum;
            if x >= radius {
                let value = source[row + x - radius];
                sum[0] -= value[0];
                sum[1] -= value[1];
                sum[2] -= value[2];
            }
            let next = x + radius + 1;
            if next < wide {
                let value = source[row + next];
                sum[0] += value[0];
                sum[1] += value[1];
                sum[2] += value[2];
            }
        }
    }

    let diameter = 2 * radius + 1;
    let area = (diameter * diameter) as f64;
    // Vertical pass writes directly back to the image, one pixel at a time,
    // so only two full-size temporary buffers are needed.
    let mut vertical = vec![[0_u32; 3]; wide];
    for y in 0..high {
        vertical.fill([0; 3]);
        for row in y.saturating_sub(radius)..=(y + radius).min(high - 1) {
            for x in 0..wide {
                let value = horizontal[row * wide + x];
                vertical[x][0] += value[0];
                vertical[x][1] += value[1];
                vertical[x][2] += value[2];
            }
        }
        for x in 0..wide {
            let bloom = [
                (f64::from(vertical[x][0]) / area * strength).round() as u32,
                (f64::from(vertical[x][1]) / area * strength).round() as u32,
                (f64::from(vertical[x][2]) / area * strength).round() as u32,
            ];
            if bloom == [0, 0, 0] {
                continue;
            }
            let global = (top + y) * width + (left + x);
            // Keep the emissive source crisp; the glow is a halo around it.
            if glow[global] > 0 {
                continue;
            }
            let pixel = global * 4;
            let alpha = bloom[0].max(bloom[1]).max(bloom[2]).min(255) as u8;
            for (channel, component) in bloom.iter().enumerate() {
                let value = u32::from(image.rgba[pixel + channel]) + component;
                image.rgba[pixel + channel] = value.min(255) as u8;
            }
            if image.rgba[pixel + 3] < alpha {
                image.rgba[pixel + 3] = alpha;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::apply;
    use crate::image::Image;

    #[test]
    fn a_glowing_pixel_spreads_into_its_neighbours() {
        let mut image = Image {
            width: 9,
            height: 9,
            rgba: vec![0; 9 * 9 * 4],
        };
        let mut glow = vec![0; 9 * 9];
        let center = 4 * 9 + 4;
        image.rgba[center * 4..center * 4 + 4].copy_from_slice(&[255, 200, 100, 255]);
        glow[center] = 255;
        apply(&mut image, &glow, 2, 1.0);
        let neighbour = 4 * 9 + 5;
        assert!(image.rgba[neighbour * 4] > 0);
        assert_eq!(image.rgba[center * 4], 255);
    }

    #[test]
    fn without_glow_the_image_is_unchanged() {
        let mut image = Image {
            width: 4,
            height: 4,
            rgba: vec![10; 4 * 4 * 4],
        };
        let before = image.rgba.clone();
        apply(&mut image, &[0; 16], 3, 1.0);
        assert_eq!(image.rgba, before);
    }
}
