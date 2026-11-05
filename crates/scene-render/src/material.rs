//! Flat material palettes used by the pixel-art backends.

use std::fs;
use std::path::Path;

/// One tone per visible face plus an outline colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub top: [u8; 3],
    pub right: [u8; 3],
    pub left: [u8; 3],
    pub line: [u8; 3],
}

/// Built-in material palette by name.
#[must_use]
pub fn builtin(name: &str) -> Option<Palette> {
    let palette = match name {
        "wood" => Palette {
            top: [0xB0, 0xB2, 0xB6],
            right: [0x8A, 0x8D, 0x92],
            left: [0x68, 0x6B, 0x70],
            line: [0x26, 0x28, 0x2E],
        },
        "metal" => Palette {
            top: [0x96, 0x9A, 0xA0],
            right: [0x75, 0x79, 0x7F],
            left: [0x58, 0x5C, 0x62],
            line: [0x22, 0x24, 0x2A],
        },
        "light" => Palette {
            top: [0xCE, 0xD0, 0xD4],
            right: [0xA8, 0xAB, 0xB0],
            left: [0x84, 0x87, 0x8D],
            line: [0x2C, 0x2E, 0x34],
        },
        "glass" => Palette {
            top: [0x96, 0xBE, 0xC8],
            right: [0x78, 0x9E, 0xA8],
            left: [0x5E, 0x80, 0x8A],
            line: [0x28, 0x34, 0x3A],
        },
        "dark" => Palette {
            top: [0x6E, 0x70, 0x76],
            right: [0x56, 0x58, 0x5E],
            left: [0x40, 0x42, 0x48],
            line: [0x1E, 0x20, 0x26],
        },
        "red" => Palette {
            top: [0xA4, 0x18, 0x1A],
            right: [0x84, 0x12, 0x16],
            left: [0x6C, 0x10, 0x14],
            line: [0x28, 0x19, 0x1B],
        },
        "white" => Palette {
            top: [0xF4, 0xF4, 0xF0],
            right: [0xE6, 0xE7, 0xE4],
            left: [0xD7, 0xD9, 0xD7],
            line: [0x26, 0x28, 0x2E],
        },
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

    Ok(tone_pair(base, outline))
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
    }
}

#[cfg(test)]
mod tests {
    use super::{load, parse_hex};

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
}
