//! What the language server knows about a document: diagnostics, symbols,
//! hover text, definitions and completions. Pure functions of the text and
//! the files it includes, so they are tested without a client.

use std::fs;
use std::path::{Component, Path, PathBuf};

use scene_syntax::ast::{ItemKind, Program};
use scene_syntax::{Diagnostic, parse};

use crate::help::SPEC;

/// Byte offsets of line starts, for LSP positions (line, UTF-16 column).
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(index, _)| index + 1));
        Self { starts }
    }

    /// The LSP position of byte `offset` (clamped to the text).
    #[must_use]
    pub fn position(&self, text: &str, offset: usize) -> (u32, u32) {
        let offset = floor_boundary(text, offset.min(text.len()));
        let line = self.starts.partition_point(|&start| start <= offset) - 1;
        let column: usize = text[self.starts[line]..offset]
            .chars()
            .map(char::len_utf16)
            .sum();
        (
            u32::try_from(line).unwrap_or(u32::MAX),
            u32::try_from(column).unwrap_or(u32::MAX),
        )
    }

    /// The byte offset of an LSP position (clamped to the line and text).
    #[must_use]
    pub fn offset(&self, text: &str, line: u32, column: u32) -> usize {
        let Some(&start) = self.starts.get(line as usize) else {
            return text.len();
        };
        let end = self
            .starts
            .get(line as usize + 1)
            .map_or(text.len(), |&next| next - 1);
        let mut units = 0_u32;
        for (index, character) in text[start..end].char_indices() {
            if units >= column {
                return start + index;
            }
            units += u32::try_from(character.len_utf16()).unwrap_or(2);
        }
        end
    }
}

fn floor_boundary(text: &str, mut offset: usize) -> usize {
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// A diagnostic with a byte range in the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub start: usize,
    pub end: usize,
    pub code: &'static str,
    pub message: String,
}

/// What kind of file a path is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Scene,
    Model,
    Walls,
    Other,
}

impl FileKind {
    #[must_use]
    pub fn of(path: &Path) -> Self {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("scene") => Self::Scene,
            Some("model") => Self::Model,
            Some("walls") => Self::Walls,
            _ => Self::Other,
        }
    }
}

/// Parse and evaluate the document, as the CLI would, and report the first
/// problem. `root` is the directory includes must stay inside.
#[must_use]
pub fn check(path: &Path, text: &str, root: &Path) -> Vec<Problem> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let result: Result<(), Diagnostic> = match FileKind::of(path) {
        FileKind::Scene => parse(text)
            .and_then(|program| scene_core::compile_scene(&program, directory, root).map(drop)),
        FileKind::Model => {
            parse(text).and_then(|program| scene_core::compile_model(&program, directory).map(drop))
        }
        FileKind::Walls => scene_walls::parse(text, directory, root).map(drop),
        FileKind::Other => Ok(()),
    };
    match result {
        Ok(()) => Vec::new(),
        Err(diagnostic) => vec![problem(text, &diagnostic)],
    }
}

/// A problem whose range covers at least the word at the diagnostic, so an
/// editor has something to underline.
fn problem(text: &str, diagnostic: &Diagnostic) -> Problem {
    let start = floor_boundary(text, (diagnostic.span.start as usize).min(text.len()));
    let mut end = floor_boundary(text, (diagnostic.span.end as usize).min(text.len())).max(start);
    if end == start {
        end = start
            + text[start..]
                .find(|c: char| !is_word(c))
                .unwrap_or(text.len() - start);
        if end == start {
            end = text[start..]
                .chars()
                .next()
                .map_or(start, |c| start + c.len_utf8());
        }
    }
    Problem {
        start,
        end,
        code: diagnostic.code,
        message: diagnostic.message.clone(),
    }
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The identifier around byte `offset`, with its byte range.
#[must_use]
pub fn word_at(text: &str, offset: usize) -> Option<(&str, usize, usize)> {
    let offset = floor_boundary(text, offset.min(text.len()));
    let start = text[..offset]
        .rfind(|c: char| !is_word(c))
        .map_or(0, |index| index + 1);
    let end = offset
        + text[offset..]
            .find(|c: char| !is_word(c))
            .unwrap_or(text.len() - offset);
    (start < end).then(|| (&text[start..end], start, end))
}

/// Parse `text`, or, while it is being edited, the longest prefix ending at
/// a `;` or `}` before the first error, so declarations above an unfinished
/// line still hover, complete and resolve.
#[must_use]
pub fn parse_lenient(text: &str) -> Option<Program> {
    let mut end = text.len();
    for _ in 0..64 {
        match parse(&text[..end]) {
            Ok(program) => return Some(program),
            Err(error) => {
                let limit = floor_boundary(text, (error.span.start as usize).min(end));
                end = text[..limit].rfind([';', '}']).map(|at| at + 1)?;
                if end == limit {
                    end = text[..limit.saturating_sub(1)]
                        .rfind([';', '}'])
                        .map(|at| at + 1)?;
                }
            }
        }
    }
    None
}

/// Kinds of declarations the server lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    Object,
    Material,
    Animation,
}

/// A declaration with the byte ranges of the whole item and of its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub start: usize,
    pub end: usize,
    pub name_start: usize,
    pub name_end: usize,
    /// The declaration's header, e.g. `object desk (5, 2)`.
    pub detail: String,
}

/// Top-level declarations of a program, in source order.
#[must_use]
pub fn symbols(text: &str, program: &Program) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    for item in &program.items {
        let (name, kind) = match &item.kind {
            ItemKind::Object(object) => (&object.name, SymbolKind::Object),
            ItemKind::Material(material) => (&material.name, SymbolKind::Material),
            ItemKind::Animation(animation) => (&animation.name, SymbolKind::Animation),
            _ => continue,
        };
        let start = (item.span.start as usize).min(text.len());
        let end = (item.span.end as usize).min(text.len()).max(start);
        let Some(item_text) = text.get(start..end) else {
            continue;
        };
        // The name is the first whole-word match after the keyword.
        let keyword_end = item_text.find(char::is_whitespace).unwrap_or(0);
        let name_offset =
            find_word(&item_text[keyword_end..], name).map_or(0, |at| keyword_end + at);
        let header = item_text
            .split(['{', ';'])
            .next()
            .unwrap_or(item_text)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        symbols.push(Symbol {
            name: name.clone(),
            kind,
            start,
            end,
            name_start: start + name_offset,
            name_end: start + name_offset + name.len(),
            detail: header,
        });
    }
    symbols
}

fn find_word(text: &str, word: &str) -> Option<usize> {
    text.match_indices(word).map(|(at, _)| at).find(|&at| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

/// A declaration in another file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct External {
    pub path: PathBuf,
    pub text: String,
    pub symbol: Symbol,
}

/// Declarations of the `.model` files a scene includes. Paths that would
/// leave the project (absolute or with `..`) are skipped, as the evaluator
/// rejects them.
#[must_use]
pub fn included_symbols(path: &Path, program: &Program) -> Vec<External> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let mut found = Vec::new();
    for item in &program.items {
        let ItemKind::Include(include) = &item.kind else {
            continue;
        };
        let relative = Path::new(&include.path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|part| matches!(part, Component::ParentDir | Component::Prefix(_)))
        {
            continue;
        }
        let file = directory.join(relative);
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let Ok(included) = parse(&text) else {
            continue;
        };
        for symbol in symbols(&text, &included) {
            found.push(External {
                path: file.clone(),
                text: text.clone(),
                symbol,
            });
        }
    }
    found
}

/// Hover text for the word at `offset`: a declaration's header, or the spec
/// bullet or table row for a keyword.
#[must_use]
pub fn hover(path: &Path, text: &str, offset: usize) -> Option<(String, usize, usize)> {
    let (word, start, end) = word_at(text, offset)?;
    if let Some(program) = parse_lenient(text) {
        if let Some(symbol) = symbols(text, &program)
            .into_iter()
            .find(|symbol| symbol.name == word)
        {
            return Some((format!("```\n{}\n```", symbol.detail), start, end));
        }
        if let Some(external) = included_symbols(path, &program)
            .into_iter()
            .find(|external| external.symbol.name == word)
        {
            let file = external
                .path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            return Some((
                format!("```\n{}\n```\nfrom `{file}`", external.symbol.detail),
                start,
                end,
            ));
        }
    }
    keyword_doc(word).map(|doc| (doc, start, end))
}

/// The spec's words on a keyword: its row of the shape table, or the bullet
/// that starts with it.
#[must_use]
pub fn keyword_doc(word: &str) -> Option<String> {
    let lines: Vec<&str> = SPEC.lines().collect();
    if let Some(row) = lines
        .iter()
        .find(|line| line.starts_with(&format!("| `{word}` |")))
    {
        let cells: Vec<&str> = row
            .split('|')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .collect();
        if let [name, at, parameters] = cells.as_slice() {
            return Some(format!("{name} — `at`: {at}; {parameters}"));
        }
    }
    let prefixes = [
        format!("- `{word} "),
        format!("- `{word};"),
        format!("- `{word}`"),
        format!("- `place ... {word} "),
    ];
    let index = lines.iter().position(|line| {
        prefixes
            .iter()
            .any(|prefix| line.starts_with(prefix.as_str()))
    })?;
    let mut bullet = vec![lines[index].trim_start_matches("- ")];
    bullet.extend(
        lines[index + 1..]
            .iter()
            .take_while(|line| line.starts_with("  "))
            .map(|line| line.trim()),
    );
    Some(bullet.join(" "))
}

/// Keywords from the spec's lexical section, plus the contextual words.
#[must_use]
pub fn keywords() -> Vec<&'static str> {
    let Some(start) = SPEC.find("- Ключевые слова:") else {
        return Vec::new();
    };
    let section = &SPEC[start..];
    let end = section.find(".\n").unwrap_or(section.len());
    let mut words: Vec<&'static str> = section[..end]
        .split('`')
        .skip(1)
        .step_by(2)
        .flat_map(str::split_whitespace)
        .collect();
    words.extend(["animation", "frames", "duration", "once", "turn"]);
    words
}

/// Built-in material names.
pub const BUILTIN_MATERIALS: [&str; 7] =
    ["wood", "metal", "light", "glass", "dark", "red", "white"];

/// Snippets for the shapes, in the LSP snippet syntax.
pub const SHAPE_SNIPPETS: [(&str, &str); 6] = [
    (
        "box",
        "box at (${1:0}, ${2:0}, ${3:0}) size (${4:1}, ${5:1}, ${6:1}) material ${7:wood};",
    ),
    (
        "cyl",
        "cyl at (${1:0}, ${2:0}, ${3:0}) radius ${4:0.5} height ${5:1} material ${6:metal};",
    ),
    (
        "sphere",
        "sphere at (${1:0}, ${2:0}, ${3:0}) radius ${4:0.5} material ${5:glass};",
    ),
    (
        "slantbox",
        "slantbox at (${1:0}, ${2:0}, ${3:0}) size (${4:1}, ${5:1}, ${6:1}) offset (${7:0.2}, ${8:0}) material ${9:wood};",
    ),
    (
        "slantcyl",
        "slantcyl at (${1:0}, ${2:0}, ${3:0}) radius ${4:0.5} height ${5:1} offset (${6:0.2}, ${7:0}) material ${8:metal};",
    ),
    (
        "ramp",
        "ramp at (${1:0}, ${2:0}, ${3:0}) size (${4:1}, ${5:1}) rise (${6:0}, ${7:1}) material ${8:wood};",
    ),
];

#[cfg(test)]
mod tests {
    use super::{
        FileKind, LineIndex, SymbolKind, check, hover, keyword_doc, keywords, parse, symbols,
        word_at,
    };
    use std::path::Path;

    #[test]
    fn positions_count_utf16_units() {
        let text = "ab\nпривет 😀x\n";
        let index = LineIndex::new(text);
        let x = text.find('x').expect("x");
        // "привет " is 7 units, the emoji 2.
        assert_eq!(index.position(text, x), (1, 9));
        assert_eq!(index.offset(text, 1, 9), x);
        assert_eq!(index.offset(text, 1, 999), text.len() - 1);
        assert_eq!(index.offset(text, 9, 0), text.len());
        // A byte inside a character rounds down to it.
        assert_eq!(index.position(text, 4), (1, 0));
    }

    #[test]
    fn a_scene_error_has_a_range_to_underline() {
        let text = "version 1;\nrender iso; scale 8;\nplace nothing at (0, 0, 0);\n";
        let problems = check(Path::new("x.scene"), text, Path::new("."));
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code, "E0301");
        assert!(problems[0].end > problems[0].start);
        let clean = check(
            Path::new("x.scene"),
            "version 1; render iso; scale 8;",
            Path::new("."),
        );
        assert_eq!(clean, []);
        assert_eq!(
            check(Path::new("x.walls"), "version 2;", Path::new(".")).len(),
            1
        );
        assert_eq!(FileKind::of(Path::new("a.txt")), FileKind::Other);
    }

    #[test]
    fn symbols_carry_their_name_range_and_header() {
        let text = "version 1;\nmaterial oak = \"oak.toml\";\nobject desk (5, 2) {\n    box at (0, 0, 0) size (w, d, 1);\n}\nanimation open (t) frames 2 duration 10;\n";
        let program = parse(text).expect("parses");
        let found = symbols(text, &program);
        let names: Vec<(&str, SymbolKind)> =
            found.iter().map(|s| (s.name.as_str(), s.kind)).collect();
        assert_eq!(
            names,
            [
                ("oak", SymbolKind::Material),
                ("desk", SymbolKind::Object),
                ("open", SymbolKind::Animation)
            ]
        );
        let desk = &found[1];
        assert_eq!(&text[desk.name_start..desk.name_end], "desk");
        assert_eq!(desk.detail, "object desk (5, 2)");
    }

    #[test]
    fn hover_explains_keywords_and_declarations() {
        let text = "version 1; object desk (5, 2) { box at (0, 0, 0) size (w, d, 1); } render iso; scale 8; place desk at (0, 0, 0) turn 10;";
        let at = |needle: &str| text.rfind(needle).expect("needle") + 1;
        let (doc, ..) = hover(Path::new("x.scene"), text, at("desk at")).expect("desk");
        assert!(doc.contains("object desk (5, 2)"), "{doc}");
        let (doc, ..) = hover(Path::new("x.scene"), text, at("box")).expect("box");
        assert!(doc.contains("size (w, d, h)"), "{doc}");
        assert!(keyword_doc("turn").expect("turn").contains("градусов"));
        assert!(keyword_doc("animation").is_some());
        assert!(keyword_doc("scale").is_some());
        assert_eq!(word_at("ab cd", 4), Some(("cd", 3, 5)));
        assert_eq!(word_at("ab  cd", 3), None);
    }

    #[test]
    fn an_unfinished_line_does_not_hide_the_declarations_above_it() {
        let text = "version 1; object a (1, 1) { box at (0, 0, 0) size (1, 1, 1); } object b (1, 1) { box at";
        let program = super::parse_lenient(text).expect("prefix parses");
        assert_eq!(symbols(text, &program).len(), 1);
        assert!(super::parse_lenient("version").is_none());
    }

    #[test]
    fn keywords_come_from_the_spec() {
        let words = keywords();
        for word in ["place", "box", "ramp", "rot", "anchor", "turn", "animation"] {
            assert!(words.contains(&word), "{word}");
        }
    }
}
