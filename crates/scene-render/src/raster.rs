#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

//! Small z-buffered rasterizer for flat-coloured polygons.

use crate::material::Texture;

/// RGBA framebuffer with a depth channel.
pub struct Framebuffer {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    depth: Vec<f64>,
}

impl Framebuffer {
    #[must_use]
    pub fn new(width: usize, height: usize, background: [u8; 4]) -> Self {
        let mut pixels = Vec::with_capacity(width * height * 4);
        for _ in 0..width * height {
            pixels.extend_from_slice(&background);
        }
        Self {
            width,
            height,
            pixels,
            depth: vec![f64::NEG_INFINITY; width * height],
        }
    }

    #[must_use]
    pub fn into_rgba(self) -> Vec<u8> {
        self.pixels
    }

    /// Fill a triangle. The third coordinate is view depth (larger is closer).
    pub fn triangle(&mut self, a: [f64; 3], b: [f64; 3], c: [f64; 3], color: [u8; 3]) {
        self.triangle_inner(a, b, c, color, None);
    }

    /// Fill a triangle and shade every pixel with a procedural texture.
    #[allow(clippy::too_many_arguments)]
    pub fn triangle_textured(
        &mut self,
        a: [f64; 3],
        b: [f64; 3],
        c: [f64; 3],
        world: [[f64; 3]; 3],
        color: [u8; 3],
        texture: Texture,
    ) {
        self.triangle_inner(a, b, c, color, Some((texture, world)));
    }

    fn triangle_inner(
        &mut self,
        a: [f64; 3],
        b: [f64; 3],
        c: [f64; 3],
        base: [u8; 3],
        shade: Option<(Texture, [[f64; 3]; 3])>,
    ) {
        let denominator = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if denominator.abs() < 1e-12 {
            return;
        }

        let min_x = a[0].min(b[0]).min(c[0]).floor().max(0.0) as usize;
        let max_x = a[0]
            .max(b[0])
            .max(c[0])
            .ceil()
            .min(self.width.saturating_sub(1) as f64) as usize;
        let min_y = a[1].min(b[1]).min(c[1]).floor().max(0.0) as usize;
        let max_y = a[1]
            .max(b[1])
            .max(c[1])
            .ceil()
            .min(self.height.saturating_sub(1) as f64) as usize;

        if min_x > max_x || min_y > max_y {
            return;
        }

        // Each barycentric coordinate is linear along a row:
        // `lambda(px) = slope * px + offset(py)`. Solving `lambda >= -1e-9`
        // for every coordinate bounds the covered span of the row, so only
        // that span is scanned instead of the whole bounding box.
        let slope_u = (b[1] - c[1]) / denominator;
        let slope_v = (c[1] - a[1]) / denominator;
        let slope_w = -(slope_u + slope_v);
        for y in min_y..=max_y {
            let py = y as f64 + 0.5;
            let offset_u = ((c[0] - b[0]) * (py - c[1]) - (b[1] - c[1]) * c[0]) / denominator;
            let offset_v = ((a[0] - c[0]) * (py - c[1]) - (c[1] - a[1]) * c[0]) / denominator;
            let offset_w = 1.0 - offset_u - offset_v;
            let Some((from, to)) = row_span(
                [
                    (slope_u, offset_u),
                    (slope_v, offset_v),
                    (slope_w, offset_w),
                ],
                min_x,
                max_x,
            ) else {
                continue;
            };
            for x in from..=to {
                let px = x as f64 + 0.5;
                let u = ((b[1] - c[1]) * (px - c[0]) + (c[0] - b[0]) * (py - c[1])) / denominator;
                let v = ((c[1] - a[1]) * (px - c[0]) + (a[0] - c[0]) * (py - c[1])) / denominator;
                let w = 1.0 - u - v;
                if u < -1e-9 || v < -1e-9 || w < -1e-9 {
                    continue;
                }
                let z = u * a[2] + v * b[2] + w * c[2];
                let index = y * self.width + x;
                if z > self.depth[index] + 1e-9 {
                    self.depth[index] = z;
                    let color = match &shade {
                        Some((texture, world)) => {
                            let point = [
                                u * world[0][0] + v * world[1][0] + w * world[2][0],
                                u * world[0][1] + v * world[1][1] + w * world[2][1],
                                u * world[0][2] + v * world[1][2] + w * world[2][2],
                            ];
                            texture.shade(base, point)
                        }
                        None => base,
                    };
                    self.write(index, color);
                }
            }
        }
    }

    /// Draw a depth-tested line. `bias` is in world blocks.
    pub fn line(&mut self, a: [f64; 3], b: [f64; 3], color: [u8; 3], bias: f64) {
        let delta_x = b[0] - a[0];
        let delta_y = b[1] - a[1];
        let samples = (delta_x.abs().max(delta_y.abs()) * 3.0).ceil().max(1.0) as usize;

        for step in 0..=samples {
            #[allow(clippy::cast_precision_loss)]
            let t = step as f64 / samples as f64;
            let x = a[0] + delta_x * t;
            let y = a[1] + delta_y * t;
            let z = a[2] + (b[2] - a[2]) * t;
            let px = (x + 1e-9).floor();
            let py = (y + 1e-9).floor();
            if px < 0.0 || py < 0.0 {
                continue;
            }
            let (px, py) = (px as usize, py as usize);
            if px >= self.width || py >= self.height {
                continue;
            }
            let index = py * self.width + px;
            if z >= self.depth[index] - bias {
                self.write(index, color);
            }
        }
    }

    fn write(&mut self, index: usize, color: [u8; 3]) {
        let base = index * 4;
        self.pixels[base] = color[0];
        self.pixels[base + 1] = color[1];
        self.pixels[base + 2] = color[2];
        self.pixels[base + 3] = 255;
    }
}

/// Columns `from..=to` of a row that may lie inside a triangle, given each
/// barycentric coordinate as `slope * px + offset` with `px = x + 0.5`.
///
/// The bounds are widened by a pixel on each side so that rounding in the
/// solved bounds can never skip a pixel the exact per-pixel test would accept.
fn row_span(lines: [(f64, f64); 3], min_x: usize, max_x: usize) -> Option<(usize, usize)> {
    let mut low = min_x as f64;
    let mut high = max_x as f64;
    for (slope, offset) in lines {
        if slope == 0.0 {
            if offset < -1e-9 {
                return None;
            }
            continue;
        }
        // `slope * px + offset >= -1e-9` with `px = x + 0.5`.
        let edge = (-1e-9 - offset) / slope - 0.5;
        if slope > 0.0 {
            low = low.max((edge - 1.0).floor());
        } else {
            high = high.min((edge + 1.0).ceil());
        }
    }
    if low.is_nan() || high.is_nan() || low > high {
        return None;
    }
    Some((low as usize, high as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rasterizer before row spans: scan the whole bounding box.
    fn reference(width: usize, height: usize, a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Vec<bool> {
        let mut covered = vec![false; width * height];
        let denominator = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if denominator.abs() < 1e-12 {
            return covered;
        }
        for y in 0..height {
            for x in 0..width {
                let px = x as f64 + 0.5;
                let py = y as f64 + 0.5;
                let u = ((b[1] - c[1]) * (px - c[0]) + (c[0] - b[0]) * (py - c[1])) / denominator;
                let v = ((c[1] - a[1]) * (px - c[0]) + (a[0] - c[0]) * (py - c[1])) / denominator;
                let w = 1.0 - u - v;
                if u >= -1e-9 && v >= -1e-9 && w >= -1e-9 {
                    covered[y * width + x] = true;
                }
            }
        }
        covered
    }

    #[test]
    fn row_spans_cover_exactly_the_pixels_of_a_full_scan() {
        // Deterministic pseudo-random triangles, including iso-like slopes
        // through pixel centres, slivers and off-screen vertices.
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % 10_000) as f64 / 100.0 - 20.0
        };
        let (width, height) = (64, 48);
        for case in 0..2_000 {
            let mut point = || {
                let (x, y) = (next(), next());
                if case % 3 == 0 {
                    // Snap to half pixels so edges pass through pixel centres.
                    [(x * 2.0).round() / 2.0, (y * 2.0).round() / 2.0, 0.0]
                } else {
                    [x, y, 0.0]
                }
            };
            let (a, b, c) = (point(), point(), point());
            let mut framebuffer = Framebuffer::new(width, height, [0, 0, 0, 0]);
            framebuffer.triangle(a, b, c, [255, 255, 255]);
            let drawn: Vec<bool> = framebuffer
                .into_rgba()
                .chunks_exact(4)
                .map(|pixel| pixel[3] == 255)
                .collect();
            assert_eq!(
                drawn,
                reference(width, height, a, b, c),
                "case {case}: {a:?} {b:?} {c:?}"
            );
        }
    }
}
