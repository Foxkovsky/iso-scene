//! Flat material palettes used by the pixel-art backends.

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
