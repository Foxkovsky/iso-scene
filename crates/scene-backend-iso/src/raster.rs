#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

//! Small z-buffered rasterizer for flat-coloured polygons.

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

        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let px = x as f64 + 0.5;
                let py = y as f64 + 0.5;
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
