//! Flat material palettes used by the pixel-art backends.

use std::fs;
use std::path::Path;

/// Procedural surface texture, evaluated per pixel in world coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Texture {
    pub kind: TextureKind,
    /// `0` — x, `1` — y, `2` — z.
    pub axis: u8,
    pub scale: f64,
    pub strength: f64,
    pub seed: f64,
}

/// Supported procedural patterns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureKind {
    /// Concentric rings around an axis plus a slow wave along it.
    Wood,
    /// Woven grid.
    Fabric,
    /// Diagonal sheen bands.
    Glass,
}

impl Texture {
    /// Modulate a flat face colour at a world position.
    #[must_use]
    pub fn shade(&self, base: [u8; 3], world: [f64; 3]) -> [u8; 3] {
        use std::f64::consts::{PI, TAU};
        let spacing = self.scale.max(0.1);
        let wave = match self.kind {
            TextureKind::Wood => {
                let axis = usize::from(self.axis.min(2));
                let u = world[(axis + 1) % 3];
                let v = world[(axis + 2) % 3];
                let lungo = world[axis];
                let radius = ((u + 31.0 + self.seed).powi(2) + (v + 13.0).powi(2)).sqrt();
                ((radius + 0.12 * (lungo / 11.0).sin()) * TAU / spacing).sin()
            }
            TextureKind::Fabric => {
                (world[0] * PI / spacing).sin() * ((world[1] + world[2]) * PI / spacing).sin()
            }
            TextureKind::Glass => {
                let distance = (world[0] - world[1]) * 0.35 + world[2] * 0.15;
                0.6 * (distance * TAU / spacing).sin()
                    + 0.4 * ((distance * 2.3 + 1.7) * TAU / spacing).sin()
            }
        };
        let factor = 1.0 + self.strength * wave;
        [
            tone(base[0], factor),
            tone(base[1], factor),
            tone(base[2], factor),
        ]
    }
}

/// One tone per visible face plus an outline colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub top: [u8; 3],
    pub right: [u8; 3],
    pub left: [u8; 3],
    pub line: [u8; 3],
    pub texture: Option<Texture>,
}

fn palette(top: [u8; 3], right: [u8; 3], left: [u8; 3], line: [u8; 3]) -> Palette {
    Palette {
        top,
        right,
        left,
        line,
        texture: None,
    }
}

/// Built-in material palette by name.
#[must_use]
pub fn builtin(name: &str) -> Option<Palette> {
    let palette = match name {
        "wood" => palette(
            [0xB0, 0xB2, 0xB6],
            [0x8A, 0x8D, 0x92],
            [0x68, 0x6B, 0x70],
            [0x26, 0x28, 0x2E],
        ),
        "metal" => palette(
            [0x96, 0x9A, 0xA0],
            [0x75, 0x79, 0x7F],
            [0x58, 0x5C, 0x62],
            [0x22, 0x24, 0x2A],
        ),
        "light" => palette(
            [0xCE, 0xD0, 0xD4],
            [0xA8, 0xAB, 0xB0],
            [0x84, 0x87, 0x8D],
            [0x2C, 0x2E, 0x34],
        ),
        "glass" => palette(
            [0x96, 0xBE, 0xC8],
            [0x78, 0x9E, 0xA8],
            [0x5E, 0x80, 0x8A],
            [0x28, 0x34, 0x3A],
        ),
        "dark" => palette(
            [0x6E, 0x70, 0x76],
            [0x56, 0x58, 0x5E],
            [0x40, 0x42, 0x48],
            [0x1E, 0x20, 0x26],
        ),
        "red" => palette(
            [0xA4, 0x18, 0x1A],
            [0x84, 0x12, 0x16],
            [0x6C, 0x10, 0x14],
            [0x28, 0x19, 0x1B],
        ),
        "white" => palette(
            [0xF4, 0xF4, 0xF0],
            [0xE6, 0xE7, 0xE4],
            [0xD7, 0xD9, 0xD7],
            [0x26, 0x28, 0x2E],
        ),
        _ => return None,
    };
    Some(palette)
}

/// Load a material TOML file and derive flat face tones.
///
/// `color` and `outline` are used; `roughness`, `metallic` and `specular` are
/// validated but flat shading does not use them yet. Texture blocks are
/// accepted and ignored for now.
///
/// # Errors
///
/// Returns a human-readable message for missing files, invalid TOML, bad
/// colours or out-of-range material properties.
pub fn load(path: &Path) -> Result<Palette, String> {
    let text = fs::read_to_string(path)
        .map_err(|cause| format!("cannot read `{}`: {cause}", path.display()))?;
    let value: toml::Value = text
        .parse()
        .map_err(|cause| format!("invalid TOML: {cause}"))?;
    let table = value
        .as_table()
        .ok_or_else(|| "material must be a TOML table".to_owned())?;

    for key in ["roughness", "metallic", "specular"] {
        if let Some(value) = table.get(key) {
            let number = value
                .as_float()
                .ok_or_else(|| format!("`{key}` must be a number"))?;
            if !(0.0..=1.0).contains(&number) {
                return Err(format!("`{key}` must be in 0..=1"));
            }
        }
    }

    let color = table
        .get("color")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "missing `color`".to_owned())?;
    let base = parse_hex(color)?;
    let outline = match table.get("outline").and_then(toml::Value::as_str) {
        Some(value) => parse_hex(value)?,
        None => [0x20, 0x21, 0x26],
    };

    let mut palette = tone_pair(base, outline);
    palette.texture = parse_texture(table.get("texture"))?;
    Ok(palette)
}

#[allow(clippy::cast_precision_loss)]
fn parse_texture(value: Option<&toml::Value>) -> Result<Option<Texture>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let table = value
        .as_table()
        .ok_or_else(|| "`texture` must be a table".to_owned())?;
    let kind = match table
        .get("type")
        .and_then(toml::Value::as_str)
        .unwrap_or("none")
    {
        "none" => return Ok(None),
        "wood" => TextureKind::Wood,
        "fabric" => TextureKind::Fabric,
        "glass" => TextureKind::Glass,
        other => return Err(format!("unknown texture type `{other}`")),
    };
    let axis = match table
        .get("axis")
        .and_then(toml::Value::as_str)
        .unwrap_or("z")
    {
        "x" => 0,
        "y" => 1,
        "z" => 2,
        other => return Err(format!("texture axis must be x, y or z, got `{other}`")),
    };
    let number = |key: &str, default: f64| -> Result<f64, String> {
        match table.get(key) {
            Some(value) => value
                .as_float()
                .or_else(|| value.as_integer().map(|value| value as f64))
                .ok_or_else(|| format!("`texture.{key}` must be a number")),
            None => Ok(default),
        }
    };
    let scale = number("scale", 4.0)?;
    if scale <= 0.0 {
        return Err("`texture.scale` must be positive".to_owned());
    }
    let strength = number("strength", 0.08)?;
    if strength < 0.0 {
        return Err("`texture.strength` must not be negative".to_owned());
    }
    Ok(Some(Texture {
        kind,
        axis,
        scale,
        strength,
        seed: number("seed", 0.0)?,
    }))
}

fn parse_hex(value: &str) -> Result<[u8; 3], String> {
    let bytes = value.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        return Err(format!("colour `{value}` must have the form #RRGGBB"));
    }
    let mut rgb = [0_u8; 3];
    for (index, channel) in rgb.iter_mut().enumerate() {
        let high = hex_value(bytes[index * 2 + 1])?;
        let low = hex_value(bytes[index * 2 + 2])?;
        *channel = (high << 4) | low;
    }
    Ok(rgb)
}

fn hex_value(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("colour must contain hex digits".to_owned()),
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn tone(channel: u8, factor: f64) -> u8 {
    (f64::from(channel) * factor).round().clamp(0.0, 255.0) as u8
}

fn tone_pair(base: [u8; 3], line: [u8; 3]) -> Palette {
    Palette {
        top: base,
        right: [
            tone(base[0], 0.78),
            tone(base[1], 0.78),
            tone(base[2], 0.78),
        ],
        left: [
            tone(base[0], 0.59),
            tone(base[1], 0.59),
            tone(base[2], 0.59),
        ],
        line,
        texture: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Texture, TextureKind, load, parse_hex};

    #[test]
    fn hex_colours_are_parsed() {
        assert_eq!(parse_hex("#BD8953").expect("hex"), [0xBD, 0x89, 0x53]);
        assert!(parse_hex("BD8953").is_err());
        assert!(parse_hex("#GG8953").is_err());
    }

    #[test]
    fn material_file_is_loaded() {
        let path = std::env::temp_dir().join(format!(
            "iso-scene-material-{}-{}.toml",
            std::process::id(),
            std::line!()
        ));
        std::fs::write(
            &path,
            "name = \"Oak\"\ncolor = \"#BD8953\"\nroughness = 0.65\noutline = \"#4B3528\"\n",
        )
        .expect("write material");
        let palette = load(&path).expect("load material");
        assert_eq!(palette.top, [0xBD, 0x89, 0x53]);
        assert_eq!(palette.line, [0x4B, 0x35, 0x28]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn missing_colour_is_an_error() {
        let path = std::env::temp_dir().join(format!(
            "iso-scene-material-bad-{}-{}.toml",
            std::process::id(),
            std::line!()
        ));
        std::fs::write(&path, "name = \"Bad\"\n").expect("write material");
        assert!(load(&path).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn wood_texture_modulates_the_base_colour() {
        let texture = Texture {
            kind: TextureKind::Wood,
            axis: 2,
            scale: 4.0,
            strength: 0.2,
            seed: 0.0,
        };
        let first = texture.shade([200, 150, 100], [0.0, 0.0, 0.0]);
        let second = texture.shade([200, 150, 100], [2.0, 0.0, 0.0]);
        assert_ne!(first, second);
    }

    #[test]
    fn a_texture_block_is_parsed() {
        let dir = std::env::temp_dir().join(format!("iso-scene-material-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("oak.toml");
        std::fs::write(
            &path,
            "color = \"#BD8953\"\noutline = \"#4B3528\"\n[texture]\ntype = \"wood\"\naxis = \"z\"\nscale = 4.0\nstrength = 0.12\nseed = 7\n",
        )
        .expect("write");
        let palette = load(&path).expect("load");
        let texture = palette.texture.expect("texture");
        assert_eq!(texture.kind, TextureKind::Wood);
        assert_eq!(texture.axis, 2);
        assert!((texture.scale - 4.0).abs() < 1e-9);
        assert!((texture.seed - 7.0).abs() < 1e-9);
        let _ = std::fs::remove_dir_all(dir);
    }
}
