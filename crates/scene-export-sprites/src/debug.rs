//! `<name>.debug.png`: cell borders, anchors and labels.

use scene_render::{Image, composite_over};

use crate::layout::{Page, cell_origin};

/// The sheet on a light background with cell borders, anchor crosses and a
/// `name/view` (or `name#frame`) label in each cell, for checking that nothing
/// is clipped and that anchors line up.
pub(crate) fn debug_grid(sheet: &Image, page: &Page, padding: u32) -> Image {
    const BACKGROUND: [u8; 4] = [236, 236, 236, 255];
    const BORDER: [u8; 4] = [220, 40, 160, 255];
    const CROSS: [u8; 4] = [0, 150, 220, 255];
    const TEXT: [u8; 4] = [20, 20, 20, 255];
    let (cells, cell_size, anchor) = (&page.cells, page.cell_size, page.anchor);
    let mut image = Image {
        width: sheet.width,
        height: sheet.height,
        rgba: BACKGROUND.repeat(sheet.width as usize * sheet.height as usize),
    };
    composite_over(&mut image, sheet, 0, 0, [0, 0, sheet.width, sheet.height]);
    for cell in cells {
        let [left, top] = cell_origin(cell, cell_size, padding);
        let right = left + cell_size[0] - 1;
        let bottom = top + cell_size[1] - 1;
        for x in left..=right {
            put(&mut image, x, top, BORDER);
            put(&mut image, x, bottom, BORDER);
        }
        for y in top..=bottom {
            put(&mut image, left, y, BORDER);
            put(&mut image, right, y, BORDER);
        }
        let center = [
            left + anchor[0].floor() as u32,
            top + anchor[1].floor() as u32,
        ];
        for delta in 0..=4_u32 {
            put(&mut image, center[0] + delta, center[1], CROSS);
            put(
                &mut image,
                center[0].saturating_sub(delta),
                center[1],
                CROSS,
            );
            put(&mut image, center[0], center[1] + delta, CROSS);
            put(
                &mut image,
                center[0],
                center[1].saturating_sub(delta),
                CROSS,
            );
        }
        let label = match cell.animation {
            Some(_) => format!("{}#{}", cell.name, cell.column),
            None => format!("{}/{}", cell.name, cell.column),
        };
        draw_text(&mut image, left + 2, top + 2, right, &label, TEXT);
    }
    image
}

pub(crate) fn put(image: &mut Image, x: u32, y: u32, colour: [u8; 4]) {
    if x < image.width && y < image.height {
        let index = (y as usize * image.width as usize + x as usize) * 4;
        if let Some(pixel) = image.rgba.get_mut(index..index + 4) {
            pixel.copy_from_slice(&colour);
        }
    }
}

/// Draw `text` with a 3x5 pixel font, clipped at `right`. Unknown characters
/// become a question mark.
pub(crate) fn draw_text(
    image: &mut Image,
    x: u32,
    y: u32,
    right: u32,
    text: &str,
    colour: [u8; 4],
) {
    let mut pen = x;
    for character in text.chars() {
        if pen + 3 > right {
            break;
        }
        let rows = glyph(character.to_ascii_lowercase());
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..3_u32 {
                if bits & (0b100 >> column) != 0 {
                    put(image, pen + column, y + row as u32, colour);
                }
            }
        }
        pen += 4;
    }
}

/// Rows of a 3x5 glyph, most significant of the three bits on the left.
pub(crate) fn glyph(character: char) -> [u8; 5] {
    match character {
        'a' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'b' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'c' => [0b011, 0b100, 0b100, 0b100, 0b011],
        'd' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'e' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'f' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'g' => [0b011, 0b100, 0b101, 0b101, 0b011],
        'h' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'i' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'j' => [0b001, 0b001, 0b001, 0b101, 0b010],
        'k' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'l' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'm' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'n' => [0b110, 0b101, 0b101, 0b101, 0b101],
        'o' => [0b010, 0b101, 0b101, 0b101, 0b010],
        'p' => [0b110, 0b101, 0b110, 0b100, 0b100],
        'q' => [0b010, 0b101, 0b101, 0b110, 0b011],
        'r' => [0b110, 0b101, 0b110, 0b101, 0b101],
        's' => [0b011, 0b100, 0b010, 0b001, 0b110],
        't' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'u' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'v' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'w' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'x' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        'z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b110, 0b001, 0b010, 0b100, 0b111],
        '3' => [0b110, 0b001, 0b010, 0b001, 0b110],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b110, 0b001, 0b110],
        '6' => [0b011, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b110],
        '_' => [0b000, 0b000, 0b000, 0b000, 0b111],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        '/' => [0b001, 0b001, 0b010, 0b100, 0b100],
        '#' => [0b101, 0b111, 0b101, 0b111, 0b101],
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        ' ' => [0; 5],
        _ => [0b111, 0b001, 0b010, 0b000, 0b010],
    }
}
