//! Flat material palettes used by the pixel-art backends.

use std::fs;
use std::path::{Component, Path};
use std::sync::Arc;

use crate::image::{Image, read_png};

/// Procedural surface texture, evaluated per pixel in world coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub kind: TextureKind,
    /// `0` — x, `1` — y, `2` — z.
    pub axis: u8,
    pub scale: f64,
    pub strength: f64,
    pub seed: f64,
    /// Optional albedo image; when present it replaces the procedural pattern.
    pub image: Option<Arc<Image>>,
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
    /// An albedo image sampled in world space.
    Image,
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
            TextureKind::Image => 0.0,
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
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub top: [u8; 3],
    pub right: [u8; 3],
    pub left: [u8; 3],
    pub line: [u8; 3],
    pub texture: Option<Texture>,
    /// When set, visible faces use this colour instead of the shaded tones.
    pub emission: Option<[u8; 3]>,
    /// Bloom mask written with the face, 0..=255.
    pub glow: u8,
    /// Surface roughness, 0..=1: lower makes the specular highlight sharper.
    pub roughness: f64,
    /// Metallic tint of the highlight and a slight diffuse darkening.
    pub metallic: f64,
    /// Strength of the sun highlight on lit faces; 0 keeps flat shading.
    pub specular: f64,
    /// Outline thickness in screen pixels; the reference is 1.
    pub outline_width: f64,
}

/// Scene lighting: the sun position and the ambient floor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub sun: [f64; 2],
    pub ambient: f64,
}

impl Default for Light {
    fn default() -> Self {
        Self {
            sun: [135.0, 45.0],
            ambient: 0.35,
        }
    }
}

impl Light {
    /// Unit vector towards the sun from an azimuth and altitude in degrees.
    #[must_use]
    pub fn sun_direction(self) -> [f64; 3] {
        let azimuth = self.sun[0].to_radians();
        let altitude = self.sun[1].to_radians();
        [
            altitude.cos() * azimuth.cos(),
            altitude.cos() * azimuth.sin(),
            altitude.sin(),
        ]
    }

    /// Lambert term of a face normal: ambient plus the diffuse sun.
    fn lambert(self, normal: [f64; 3]) -> f64 {
        let sun = self.sun_direction();
        let dot = normal[0] * sun[0] + normal[1] * sun[1] + normal[2] * sun[2];
        self.ambient + (1.0 - self.ambient) * dot.max(0.0)
    }

    /// How much this light changes a face relative to the reference palette:
    /// exactly `1.0` at the default `[135, 45]` / `0.35`, so default scenes
    /// keep their pixels.
    #[must_use]
    pub fn face_factor(self, normal: [f64; 3]) -> f64 {
        let reference = Self::default().lambert(normal);
        if reference <= 1e-6 {
            return 1.0;
        }
        (self.lambert(normal) / reference).clamp(0.0, 4.0)
    }
}

/// Apply the scene light and material response to one flat face.
#[must_use]
pub fn apply_light(color: [u8; 3], normal: [f64; 3], palette: &Palette, light: Light) -> [u8; 3] {
    use std::ops::Mul;
    // Metal has little diffuse light; the energy moves to the highlight.
    let diffuse = light.face_factor(normal) * (1.0 - 0.25 * palette.metallic);
    let mut shaded = [
        tone(color[0], diffuse),
        tone(color[1], diffuse),
        tone(color[2], diffuse),
    ];
    let sun = light.sun_direction();
    let dot = normal[0].mul(sun[0]) + normal[1].mul(sun[1]) + normal[2].mul(sun[2]);
    if palette.specular > 0.0 && dot > 0.0 {
        let gloss = (1.0 - palette.roughness).clamp(0.0, 1.0);
        let strength = palette.specular * (0.35 + 0.65 * gloss) * dot.powf(1.0 + 4.0 * gloss);
        // A metal tints its highlight with the base colour, a dielectric with white.
        let tint = [
            tone(255, 1.0 - palette.metallic) + tone(color[0], palette.metallic),
            tone(255, 1.0 - palette.metallic) + tone(color[1], palette.metallic),
            tone(255, 1.0 - palette.metallic) + tone(color[2], palette.metallic),
        ];
        for (channel, target) in shaded.iter_mut().zip(tint) {
            *channel = tone(*channel, 1.0 - strength.clamp(0.0, 1.0))
                + tone(target, strength.clamp(0.0, 1.0));
        }
    }
    shaded
}

fn palette(top: [u8; 3], right: [u8; 3], left: [u8; 3], line: [u8; 3]) -> Palette {
    Palette {
        top,
        right,
        left,
        line,
        texture: None,
        emission: None,
        glow: 0,
        roughness: 0.5,
        metallic: 0.0,
        specular: 0.0,
        outline_width: 1.0,
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
/// `color` and `outline` are used; `top`, `right` and `left` override the
/// derived face tones. `roughness`, `metallic` and `specular` are validated but
/// flat shading does not use them yet. Texture blocks are accepted and used for
/// the isometric raster.
///
/// # Errors
///
/// Returns a human-readable message for missing files, invalid TOML, bad
/// colours or out-of-range material properties.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
pub fn load(path: &Path) -> Result<Palette, String> {
    let text = fs::read_to_string(path)
        .map_err(|cause| format!("cannot read `{}`: {cause}", path.display()))?;
    let value: toml::Value = text
        .parse()
        .map_err(|cause| format!("invalid TOML: {cause}"))?;
    let table = value
        .as_table()
        .ok_or_else(|| "material must be a TOML table".to_owned())?;

    let mut properties = [0.5_f64, 0.0, 0.0];
    for (index, key) in ["roughness", "metallic", "specular"].iter().enumerate() {
        if let Some(value) = table.get(*key) {
            let number = value
                .as_float()
                .ok_or_else(|| format!("`{key}` must be a number"))?;
            if !(0.0..=1.0).contains(&number) {
                return Err(format!("`{key}` must be in 0..=1"));
            }
            properties[index] = number;
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
    // Optional explicit face tones; by default they are derived from `color`.
    for (key, tone) in [
        ("top", &mut palette.top),
        ("right", &mut palette.right),
        ("left", &mut palette.left),
    ] {
        if let Some(value) = table.get(key).and_then(toml::Value::as_str) {
            *tone = parse_hex(value)?;
        }
    }
    palette.roughness = properties[0];
    palette.metallic = properties[1];
    palette.specular = properties[2];
    palette.outline_width = match table.get("outline_width") {
        Some(value) => {
            let number = value
                .as_float()
                .or_else(|| value.as_integer().map(|value| value as f64))
                .ok_or_else(|| "`outline_width` must be a number".to_owned())?;
            if !(0.0..=8.0).contains(&number) {
                return Err("`outline_width` must be in 0..=8 pixels".to_owned());
            }
            number
        }
        None => 1.0,
    };
    palette.texture = parse_texture(table.get("texture"), path.parent())?;
    palette.emission = match table.get("emission").and_then(toml::Value::as_str) {
        Some(value) => Some(parse_hex(value)?),
        None => None,
    };
    palette.glow = match table.get("glow") {
        Some(value) => {
            let number = value
                .as_float()
                .or_else(|| value.as_integer().map(|value| value as f64))
                .ok_or_else(|| "`glow` must be a number".to_owned())?;
            if !(0.0..=1.0).contains(&number) {
                return Err("`glow` must be in 0..=1".to_owned());
            }
            (number * 255.0).round() as u8
        }
        None => u8::from(palette.emission.is_some()) * 153,
    };
    Ok(palette)
}

#[allow(clippy::cast_precision_loss)]
fn parse_texture(
    value: Option<&toml::Value>,
    base: Option<&Path>,
) -> Result<Option<Texture>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let table = value
        .as_table()
        .ok_or_else(|| "`texture` must be a table".to_owned())?;
    let image = match table.get("image").and_then(toml::Value::as_str) {
        Some(relative) => {
            let relative_path = Path::new(relative);
            if relative_path.is_absolute()
                || relative_path
                    .components()
                    .any(|component| matches!(component, Component::ParentDir))
            {
                return Err(format!(
                    "texture image `{relative}` must stay in the material directory"
                ));
            }
            let full = base.unwrap_or_else(|| Path::new(".")).join(relative_path);
            Some(Arc::new(read_png(&full).map_err(|message| {
                format!("texture `{relative}`: {message}")
            })?))
        }
        None => None,
    };
    let kind = if image.is_some() {
        TextureKind::Image
    } else {
        match table
            .get("type")
            .and_then(toml::Value::as_str)
            .unwrap_or("none")
        {
            "none" => return Ok(None),
            "wood" => TextureKind::Wood,
            "fabric" => TextureKind::Fabric,
            "glass" => TextureKind::Glass,
            other => return Err(format!("unknown texture type `{other}`")),
        }
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
        image,
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
        emission: None,
        glow: 0,
        roughness: 0.5,
        metallic: 0.0,
        specular: 0.0,
        outline_width: 1.0,
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
    fn an_emission_and_glow_are_parsed() {
        let path = std::env::temp_dir().join(format!(
            "iso-scene-material-glow-{}-{}.toml",
            std::process::id(),
            std::line!()
        ));
        std::fs::write(
            &path,
            "color = \"#BD8953\"\nemission = \"#FFD9A0\"\nglow = 0.8\n",
        )
        .expect("write material");
        let palette = load(&path).expect("load material");
        assert_eq!(palette.emission, Some([0xFF, 0xD9, 0xA0]));
        assert_eq!(palette.glow, 204);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_emission_without_glow_gets_a_default_halo() {
        let path = std::env::temp_dir().join(format!(
            "iso-scene-material-glow-default-{}-{}.toml",
            std::process::id(),
            std::line!()
        ));
        std::fs::write(&path, "color = \"#BD8953\"\nemission = \"#FFD9A0\"\n")
            .expect("write material");
        let palette = load(&path).expect("load material");
        assert_eq!(palette.glow, 153);
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
            image: None,
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

#[cfg(test)]
mod face_tone_tests {
    use super::load;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "iso-scene-material-tones-{name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create dir");
        dir
    }

    #[test]
    fn explicit_face_tones_override_the_derived_ones() {
        let dir = scratch("explicit");
        let path = dir.join("m.toml");
        fs::write(
            &path,
            "color = \"#BD8953\"\ntop = \"#FFFFFF\"\nright = \"#808080\"\nleft = \"#202020\"\n",
        )
        .expect("write");
        let palette = load(&path).expect("load");
        assert_eq!(palette.top, [0xFF, 0xFF, 0xFF]);
        assert_eq!(palette.right, [0x80, 0x80, 0x80]);
        assert_eq!(palette.left, [0x20, 0x20, 0x20]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_face_tones_keep_the_derivation() {
        let dir = scratch("derived");
        let path = dir.join("m.toml");
        fs::write(&path, "color = \"#BD8953\"\nright = \"#000000\"\n").expect("write");
        let palette = load(&path).expect("load");
        assert_eq!(palette.top, [0xBD, 0x89, 0x53]);
        assert_eq!(palette.right, [0, 0, 0]);
        assert_eq!(palette.left, [0x70, 0x51, 0x31]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_bad_face_tone_is_an_error() {
        let dir = scratch("bad");
        let path = dir.join("m.toml");
        fs::write(&path, "color = \"#BD8953\"\ntop = \"#GGGGGG\"\n").expect("write");
        assert!(load(&path).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod light_tests {
    use super::{Light, apply_light, builtin};

    #[test]
    fn the_reference_light_keeps_the_palette_tones() {
        let palette = builtin("wood").expect("wood");
        let light = Light::default();
        for (normal, tone) in [
            ([0.0, 0.0, 1.0], palette.top),
            ([1.0, 0.0, 0.0], palette.right),
            ([0.0, 1.0, 0.0], palette.left),
        ] {
            assert_eq!(apply_light(tone, normal, &palette, light), tone);
        }
    }

    #[test]
    fn another_sun_and_ambient_change_the_faces() {
        let palette = builtin("wood").expect("wood");
        let bright = Light {
            sun: [0.0, 90.0],
            ambient: 0.9,
        };
        let default = apply_light(palette.top, [0.0, 0.0, 1.0], &palette, Light::default());
        let lit = apply_light(palette.top, [0.0, 0.0, 1.0], &palette, bright);
        assert_ne!(lit, default);
        assert!(lit[0] > default[0], "{lit:?} vs {default:?}");
    }

    #[test]
    fn specular_and_metallic_light_a_lit_face() {
        let mut palette = builtin("wood").expect("wood");
        palette.specular = 1.0;
        palette.metallic = 0.5;
        palette.roughness = 0.2;
        let lit = apply_light(palette.top, [0.0, 0.0, 1.0], &palette, Light::default());
        assert_ne!(lit, palette.top);
    }
}
