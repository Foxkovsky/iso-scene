//! Every limit from `docs/spec.md` §9, read from the constants the code
//! enforces.

use std::fmt::Write;

/// One limit as `limits` prints it.
pub struct Limit {
    /// What is limited, in English.
    pub what: &'static str,
    /// The value, formatted from the constant the code enforces.
    pub value: String,
    /// The diagnostic code reported when the limit is exceeded.
    pub code: &'static str,
    /// The row label in the `docs/spec.md` §9 table.
    pub spec_row: &'static str,
}

/// Group digits by three with spaces, as the spec does: `1 000 000`.
fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(' ');
        }
        out.push(digit);
    }
    out
}

fn count(value: usize) -> String {
    grouped(u64::try_from(value).unwrap_or(u64::MAX))
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn whole(value: f64) -> String {
    grouped(value as u64)
}

#[allow(clippy::too_many_lines)]
#[must_use]
pub fn all() -> Vec<Limit> {
    use scene_core::{
        DEFAULT_FUEL, MAX_CANVAS_BLOCKS, MAX_COORD, MAX_INCLUDE_DEPTH, MAX_INCLUDED_BYTES,
        MAX_INSTANCES, MAX_LOOP_BOUND, MAX_OBJECTS, MAX_SCALE, MAX_SHAPES,
    };
    use scene_syntax::lexer::{MAX_SOURCE_BYTES, MAX_STRING_BYTES, MAX_TOKENS};
    use scene_syntax::parser::{MAX_EXPR_DEPTH, MAX_NESTING};
    let limit = |what, value, code, spec_row| Limit {
        what,
        value,
        code,
        spec_row,
    };
    vec![
        limit(
            "source file",
            format!("{} MiB", MAX_SOURCE_BYTES >> 20),
            "E0402",
            "размер одного файла",
        ),
        limit(
            "all included files together",
            format!("{} MiB", MAX_INCLUDED_BYTES >> 20),
            "E0402",
            "суммарный размер include",
        ),
        limit(
            "include depth",
            count(MAX_INCLUDE_DEPTH),
            "E0403",
            "глубина include",
        ),
        limit("tokens per file", count(MAX_TOKENS), "E0402", "токенов"),
        limit(
            "nesting of blocks, parentheses, unary operators",
            count(MAX_NESTING),
            "E0402",
            "вложенность блоков, скобок и унарных операторов",
        ),
        limit(
            "expression depth (each chained operator is a level)",
            count(MAX_EXPR_DEPTH),
            "E0402",
            "глубина выражения (каждый оператор цепочки — уровень)",
        ),
        limit(
            "string literal",
            format!("{} bytes", count(MAX_STRING_BYTES)),
            "E0402",
            "строка",
        ),
        limit(
            "object declarations",
            count(MAX_OBJECTS),
            "E0402",
            "объектов",
        ),
        limit("shapes", count(MAX_SHAPES), "E0402", "фигур"),
        limit("placements", count(MAX_INSTANCES), "E0402", "размещений"),
        limit(
            "evaluation steps (fuel)",
            grouped(DEFAULT_FUEL),
            "E0401",
            "шагов вычисления (fuel)",
        ),
        limit(
            "loop bound",
            format!("±{}", whole(MAX_LOOP_BOUND)),
            "E0309",
            "граница цикла",
        ),
        limit(
            "coordinate",
            format!("±{} blocks", whole(MAX_COORD)),
            "E0304",
            "координата",
        ),
        limit(
            "canvas side",
            format!("{} blocks", whole(MAX_CANVAS_BLOCKS)),
            "E0402",
            "сторона canvas",
        ),
        limit(
            "scale",
            format!("1..{MAX_SCALE} px/block"),
            "E0304",
            "`scale`",
        ),
        limit(
            "frames in one animation",
            grouped(u64::from(scene_core::MAX_ANIMATION_FRAMES)),
            "E0304",
            "`animation`: кадров",
        ),
        limit(
            "rendered image side",
            format!("{} px", count(scene_backend_iso::MAX_OUTPUT_SIDE)),
            "E0602",
            "выходное изображение: сторона",
        ),
        limit(
            "rendered image area",
            format!("{} px", count(scene_backend_iso::MAX_OUTPUT_PIXELS)),
            "E0602",
            "выходное изображение: площадь",
        ),
        limit(
            "sprite sheet cells (views + frames)",
            count(scene_export_sprites::MAX_CELLS),
            "E0602",
            "лист спрайтов: клеток (виды + кадры)",
        ),
        limit(
            "sprite sheet animation frames",
            grouped(u64::from(scene_export_sprites::MAX_FRAMES)),
            "E0602",
            "лист спрайтов: кадров анимации",
        ),
        limit(
            "sprite sheet side",
            format!(
                "{} px",
                grouped(u64::from(scene_export_sprites::MAX_SHEET_SIDE))
            ),
            "E0602",
            "лист спрайтов: сторона",
        ),
        limit(
            "sprite sheet area",
            format!("{} px", grouped(scene_export_sprites::MAX_SHEET_PIXELS)),
            "E0602",
            "лист спрайтов: площадь",
        ),
        limit(
            "voxels along one axis",
            grouped(u64::from(scene_backend_voxel::MAX_AXIS)),
            "E0602",
            "воксели: по одной оси",
        ),
        limit(
            "voxels in the grid (256³)",
            grouped(u64::from(scene_backend_voxel::MAX_GRID)),
            "E0602",
            "воксели: всего",
        ),
        limit(
            "voxel colours",
            count(scene_backend_voxel::MAX_COLORS),
            "E0602",
            "воксели: цветов",
        ),
        limit(
            "Tiled map cells",
            grouped(scene_export_tiled::MAX_MAP_CELLS),
            "E0602",
            "карта Tiled: клеток",
        ),
        limit(
            "Tiled cells under object footprints, summed",
            grouped(scene_export_tiled::MAX_FOOTPRINT_CELLS),
            "E0602",
            "карта Tiled: клеток под следами объектов (сумма)",
        ),
        limit(
            "Tiled objects in one cell with --tile-layer",
            count(scene_export_tiled::MAX_OBJECT_TILE_LAYERS),
            "E0602",
            "карта Tiled, `--tile-layer`: объектов в одной клетке",
        ),
    ]
}

/// The limits as an aligned text table.
#[must_use]
pub fn text() -> String {
    let limits = all();
    let width = limits
        .iter()
        .map(|limit| limit.what.chars().count())
        .max()
        .unwrap_or(0);
    let value_width = limits
        .iter()
        .map(|limit| limit.value.chars().count())
        .max()
        .unwrap_or(0);
    let mut out =
        String::from("Limits (docs/spec.md §9); exceeding one is a diagnostic, never a crash.\n\n");
    for limit in &limits {
        let pad = width - limit.what.chars().count();
        let value_pad = value_width - limit.value.chars().count();
        let _ = writeln!(
            out,
            "  {}{}  {}{}  {}",
            limit.what,
            " ".repeat(pad),
            " ".repeat(value_pad),
            limit.value,
            limit.code
        );
    }
    out
}

/// The limits as JSON rows for agents.
#[must_use]
pub fn json() -> serde_json::Value {
    all()
        .iter()
        .map(|limit| {
            serde_json::json!({
                "limit": limit.what,
                "value": limit.value,
                "code": limit.code,
                "spec_row": limit.spec_row
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{all, grouped};

    #[test]
    fn digits_are_grouped_like_the_spec() {
        assert_eq!(grouped(7), "7");
        assert_eq!(grouped(4_096), "4 096");
        assert_eq!(grouped(1_000_000), "1 000 000");
        assert_eq!(grouped(16_777_216), "16 777 216");
    }

    /// Every limit the code enforces has a §9 row with the same value and
    /// code, so the spec and `iso-scene limits` cannot drift apart.
    #[test]
    fn every_limit_matches_its_row_in_the_spec() {
        let spec = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec.md"),
        )
        .expect("read docs/spec.md");
        let table: Vec<&str> = spec
            .split("## 9.")
            .nth(1)
            .and_then(|rest| rest.split("\n## ").next())
            .expect("§9")
            .lines()
            .filter(|line| line.starts_with("| "))
            .collect();
        let limits = all();
        for limit in &limits {
            let row = table
                .iter()
                .find(|line| line.starts_with(&format!("| {} |", limit.spec_row)))
                .unwrap_or_else(|| panic!("no §9 row `{}`", limit.spec_row));
            let cells: Vec<&str> = row.split('|').map(str::trim).collect();
            let number = limit
                .value
                .trim_end_matches(|c: char| c.is_alphabetic() || c == '/' || c == ' ');
            assert!(
                cells[2].contains(number),
                "§9 `{}` says {}, the code {}",
                limit.spec_row,
                cells[2],
                limit.value
            );
            assert_eq!(cells[3], format!("`{}`", limit.code), "{}", limit.spec_row);
        }
        // And every row of the table is a limit the code knows.
        assert_eq!(table.len() - 1, limits.len(), "§9 rows vs `limits`");
    }
}
