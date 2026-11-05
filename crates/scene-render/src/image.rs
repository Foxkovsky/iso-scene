//! Shared raster image and PNG writer.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

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
