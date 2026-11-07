#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

//! Small z-buffered rasterizer for flat-coloured polygons.
//!
//! [`Framebuffer::draw`] renders a list of shapes in parallel: the image is
//! cut into horizontal bands and every band replays the whole list, clipped
//! to its rows. A pixel belongs to exactly one band and sees the same writes
//! in the same order as a sequential pass, so the result does not depend on
//! the number of threads.

use rayon::prelude::*;

use crate::material::Texture;

/// Rows per band below which splitting the image is not worth it.
const MIN_BAND_ROWS: usize = 32;
/// Images smaller than this are drawn on the calling thread: waking the
/// thread pool costs more than the bands would save.
const MIN_PARALLEL_PIXELS: usize = 65_536;

/// RGBA framebuffer with a depth channel.
#[derive(Clone)]
pub struct Framebuffer {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    depth: Vec<f64>,
}

/// One drawing command for [`Framebuffer::draw`]. Points are in pixels with
/// the view depth (larger is closer) as the third coordinate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// A flat triangle, optionally shaded by a procedural texture evaluated at
    /// the interpolated world position.
    Triangle {
        points: [[f64; 3]; 3],
        color: [u8; 3],
        shade: Option<(Texture, [[f64; 3]; 3])>,
    },
    /// A depth-tested line; `bias` is in world blocks.
    Line {
        a: [f64; 3],
        b: [f64; 3],
        color: [u8; 3],
        bias: f64,
    },
}

impl Shape {
    /// Pixel rectangle `[left, top, right, bottom)` that contains every pixel
    /// this shape can write, with a pixel of margin.
    #[must_use]
    pub fn pixel_bounds(&self) -> [f64; 4] {
        let points: &[[f64; 3]] = match self {
            Self::Triangle { points, .. } => points,
            Self::Line { a, b, .. } => &[*a, *b][..],
        };
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for point in points {
            bounds[0] = bounds[0].min(point[0]);
            bounds[1] = bounds[1].min(point[1]);
            bounds[2] = bounds[2].max(point[0]);
            bounds[3] = bounds[3].max(point[1]);
        }
        [
            bounds[0].floor() - 1.0,
            bounds[1].floor() - 1.0,
            bounds[2].ceil() + 2.0,
            bounds[3].ceil() + 2.0,
        ]
    }
}

/// Union of [`Shape::pixel_bounds`] clamped to a `width` x `height` image, as
/// `[left, top, right, bottom)`; `None` when nothing is inside.
#[must_use]
pub fn region_of<'a>(
    shapes: impl IntoIterator<Item = &'a Shape>,
    width: usize,
    height: usize,
) -> Option<[usize; 4]> {
    let mut region = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for shape in shapes {
        let bounds = shape.pixel_bounds();
        region[0] = region[0].min(bounds[0]);
        region[1] = region[1].min(bounds[1]);
        region[2] = region[2].max(bounds[2]);
        region[3] = region[3].max(bounds[3]);
    }
    let clamp = |value: f64, limit: usize| value.clamp(0.0, limit as f64) as usize;
    let region = [
        clamp(region[0], width),
        clamp(region[1], height),
        clamp(region[2], width),
        clamp(region[3], height),
    ];
    (region[0] < region[2] && region[1] < region[3]).then_some(region)
}

impl Framebuffer {
    #[must_use]
    pub fn new(width: usize, height: usize, background: [u8; 4]) -> Self {
        Self {
            width,
            height,
            pixels: background.repeat(width * height),
            depth: vec![f64::NEG_INFINITY; width * height],
        }
    }

    #[must_use]
    pub fn into_rgba(self) -> Vec<u8> {
        self.pixels
    }

    /// Fill a triangle. The third coordinate is view depth (larger is closer).
    pub fn triangle(&mut self, a: [f64; 3], b: [f64; 3], c: [f64; 3], color: [u8; 3]) {
        self.whole().triangle(a, b, c, color, None);
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
        self.whole()
            .triangle(a, b, c, color, Some((texture, world)));
    }

    /// Draw a depth-tested line. `bias` is in world blocks.
    pub fn line(&mut self, a: [f64; 3], b: [f64; 3], color: [u8; 3], bias: f64) {
        self.whole().line(a, b, color, bias);
    }

    /// Draw `shapes` in order, in parallel over horizontal bands. The result
    /// is identical to drawing them one by one.
    pub fn draw(&mut self, shapes: &[Shape]) {
        let width = self.width;
        if width == 0 || self.height == 0 {
            return;
        }
        let threads = rayon::current_num_threads().max(1);
        // A few bands per thread balance uneven rows; tiny images stay whole.
        let rows = self.height.div_ceil(threads * 4).max(MIN_BAND_ROWS);
        if rows >= self.height || width * self.height < MIN_PARALLEL_PIXELS {
            let mut band = self.whole();
            for shape in shapes {
                band.shape(shape);
            }
            return;
        }
        self.pixels
            .par_chunks_mut(rows * width * 4)
            .zip(self.depth.par_chunks_mut(rows * width))
            .enumerate()
            .for_each(|(index, (pixels, depth))| {
                let mut band = Band {
                    width,
                    top: index * rows,
                    rows: depth.len() / width,
                    left: 0,
                    right: width,
                    pixels,
                    depth,
                };
                for shape in shapes {
                    band.shape(shape);
                }
            });
    }

    /// Reset the pixels `[left, top, right, bottom)` to `background` and redraw
    /// `shapes` clipped to them. Every pixel of the region ends up exactly as if
    /// the whole image had been drawn from scratch with `shapes`.
    pub fn draw_region(&mut self, shapes: &[Shape], region: [usize; 4], background: [u8; 4]) {
        let [left, top, right, bottom] = region;
        let (right, bottom) = (right.min(self.width), bottom.min(self.height));
        if left >= right || top >= bottom {
            return;
        }
        for y in top..bottom {
            let row = y * self.width;
            for x in left..right {
                self.depth[row + x] = f64::NEG_INFINITY;
                self.pixels[(row + x) * 4..(row + x) * 4 + 4].copy_from_slice(&background);
            }
        }
        let width = self.width;
        let mut band = Band {
            width,
            top,
            rows: bottom - top,
            left,
            right,
            pixels: &mut self.pixels[top * width * 4..bottom * width * 4],
            depth: &mut self.depth[top * width..bottom * width],
        };
        for shape in shapes {
            band.shape(shape);
        }
    }

    fn whole(&mut self) -> Band<'_> {
        Band {
            width: self.width,
            top: 0,
            rows: self.height,
            left: 0,
            right: self.width,
            pixels: &mut self.pixels,
            depth: &mut self.depth,
        }
    }
}

/// Rows `top..top + rows` of a framebuffer, drawn only in columns
/// `left..right`.
struct Band<'a> {
    width: usize,
    top: usize,
    rows: usize,
    left: usize,
    right: usize,
    pixels: &'a mut [u8],
    depth: &'a mut [f64],
}

impl Band<'_> {
    fn shape(&mut self, shape: &Shape) {
        match *shape {
            Shape::Triangle {
                points: [a, b, c],
                color,
                shade,
            } => self.triangle(a, b, c, color, shade),
            Shape::Line { a, b, color, bias } => self.line(a, b, color, bias),
        }
    }

    fn triangle(
        &mut self,
        a: [f64; 3],
        b: [f64; 3],
        c: [f64; 3],
        base: [u8; 3],
        shade: Option<(Texture, [[f64; 3]; 3])>,
    ) {
        let denominator = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if denominator.abs() < 1e-12 || self.rows == 0 {
            return;
        }

        let bottom = self.top + self.rows - 1;
        let min_x = a[0].min(b[0]).min(c[0]).floor().max(0.0) as usize;
        let max_x = a[0]
            .max(b[0])
            .max(c[0])
            .ceil()
            .min(self.width.saturating_sub(1) as f64) as usize;
        if self.right == 0 {
            return;
        }
        // Clip the scan to this band's columns; spans below still start from
        // the unclipped bounds, so the covered pixels are unchanged.
        let (scan_from, scan_to) = (self.left, self.right - 1);
        let min_y = (a[1].min(b[1]).min(c[1]).floor().max(0.0) as usize).max(self.top);
        let max_y = (a[1].max(b[1]).max(c[1]).ceil().max(0.0) as usize).min(bottom);

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
            for x in from.max(scan_from)..=to.min(scan_to) {
                let px = x as f64 + 0.5;
                let u = ((b[1] - c[1]) * (px - c[0]) + (c[0] - b[0]) * (py - c[1])) / denominator;
                let v = ((c[1] - a[1]) * (px - c[0]) + (a[0] - c[0]) * (py - c[1])) / denominator;
                let w = 1.0 - u - v;
                if u < -1e-9 || v < -1e-9 || w < -1e-9 {
                    continue;
                }
                let z = u * a[2] + v * b[2] + w * c[2];
                let index = (y - self.top) * self.width + x;
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

    fn line(&mut self, a: [f64; 3], b: [f64; 3], color: [u8; 3], bias: f64) {
        let delta_x = b[0] - a[0];
        let delta_y = b[1] - a[1];
        let samples = (delta_x.abs().max(delta_y.abs()) * 3.0).ceil().max(1.0) as usize;

        // Only the steps whose row can fall inside this band; the bounds are
        // widened by two steps and every step is still checked exactly.
        let (first, last) = if delta_y == 0.0 {
            (0, samples)
        } else {
            let top = self.top as f64;
            let bottom = (self.top + self.rows) as f64;
            let enter = (top - a[1]) / delta_y * samples as f64;
            let leave = (bottom - a[1]) / delta_y * samples as f64;
            let low = (enter.min(leave).floor() - 2.0).max(0.0);
            let high = (enter.max(leave).ceil() + 2.0).min(samples as f64);
            if low.is_nan() || high.is_nan() || low > high {
                return;
            }
            (low as usize, high as usize)
        };
        for step in first..=last {
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
            if px < self.left || px >= self.right || py < self.top || py >= self.top + self.rows {
                continue;
            }
            let index = (py - self.top) * self.width + px;
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

    #[test]
    fn parallel_draw_matches_sequential_drawing() {
        let mut state = 0x1234_5678_9abc_def1_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % 100_000) as f64 / 100.0 - 50.0
        };
        let (width, height) = (301, 517);
        let texture = Texture {
            kind: crate::material::TextureKind::Wood,
            axis: 0,
            scale: 0.4,
            strength: 0.1,
            seed: 3.0,
        };
        let mut shapes = Vec::new();
        for index in 0..600 {
            let mut point = || [next() * 4.0 + 150.0, next() * 6.0 + 250.0, next()];
            let (a, b, c) = (point(), point(), point());
            shapes.push(if index % 5 == 4 {
                Shape::Line {
                    a,
                    b,
                    color: [200, 10, 10],
                    bias: 0.05,
                }
            } else {
                Shape::Triangle {
                    points: [a, b, c],
                    color: [(index % 255) as u8, 90, 160],
                    shade: (index % 3 == 0).then_some((texture, [a, b, c])),
                }
            });
        }
        let mut sequential = Framebuffer::new(width, height, [0, 0, 0, 0]);
        for shape in &shapes {
            match *shape {
                Shape::Triangle {
                    points: [a, b, c],
                    color,
                    shade: None,
                } => {
                    sequential.triangle(a, b, c, color);
                }
                Shape::Triangle {
                    points: [a, b, c],
                    color,
                    shade: Some((texture, world)),
                } => {
                    sequential.triangle_textured(a, b, c, world, color, texture);
                }
                Shape::Line { a, b, color, bias } => sequential.line(a, b, color, bias),
            }
        }
        let mut parallel = Framebuffer::new(width, height, [0, 0, 0, 0]);
        parallel.draw(&shapes);
        let (sequential, parallel) = (sequential.into_rgba(), parallel.into_rgba());
        let differing = sequential
            .iter()
            .zip(&parallel)
            .filter(|(left, right)| left != right)
            .count();
        assert_eq!(differing, 0, "parallel bands changed {differing} bytes");
    }

    #[test]
    fn redrawing_a_region_matches_a_full_draw() {
        let mut state = 0x0bad_cafe_dead_beef_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % 100_000) as f64 / 100.0 - 50.0
        };
        let (width, height) = (240, 260);
        for round in 0..20 {
            // A scene of static shapes with a few moving ones in the middle of
            // the list, as an animated instance would be.
            let mut shapes = Vec::new();
            let mut moving = Vec::new();
            for index in 0..120 {
                let spread = if (40..50).contains(&index) { 0.6 } else { 2.5 };
                let centre = if (40..50).contains(&index) {
                    [60.0 + f64::from(round), 90.0]
                } else {
                    [120.0, 130.0]
                };
                let mut point = || {
                    [
                        centre[0] + next() * spread,
                        centre[1] + next() * spread,
                        next(),
                    ]
                };
                let (a, b, c) = (point(), point(), point());
                shapes.push(if index % 4 == 3 {
                    Shape::Line {
                        a,
                        b,
                        color: [250, 0, 0],
                        bias: 0.05,
                    }
                } else {
                    Shape::Triangle {
                        points: [a, b, c],
                        color: [(index * 2) as u8, 99, 7],
                        shade: None,
                    }
                });
                moving.push((40..50).contains(&index));
            }
            let background = [10, 20, 30, 0];
            let mut full = Framebuffer::new(width, height, background);
            full.draw(&shapes);
            let still: Vec<Shape> = shapes
                .iter()
                .zip(&moving)
                .filter(|(_, moving)| !**moving)
                .map(|(shape, _)| *shape)
                .collect();
            let mut base = Framebuffer::new(width, height, background);
            base.draw(&still);
            let region = region_of(
                shapes
                    .iter()
                    .zip(&moving)
                    .filter(|(_, moving)| **moving)
                    .map(|(shape, _)| shape),
                width,
                height,
            )
            .expect("moving shapes are on screen");
            let mut frame = base.clone();
            frame.draw_region(&shapes, region, background);
            assert!(frame.depth == full.depth, "round {round}: depth differs");
            let differing = frame
                .into_rgba()
                .iter()
                .zip(&full.into_rgba())
                .filter(|(left, right)| left != right)
                .count();
            assert_eq!(differing, 0, "round {round}");
        }
    }
}
