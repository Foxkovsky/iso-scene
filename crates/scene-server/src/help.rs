//! One section of the spec per topic, for `iso-scene help`, hovers and agents.
//!
//! The text is the spec itself, embedded at build time, so the help never
//! drifts from it; a topic only picks which section to print.

use std::fmt::Write;

pub const SPEC: &str = include_str!("../../../docs/spec.md");
const WALLS: &str = include_str!("../../../docs/walls.md");
const GRAMMAR: &str = include_str!("../../../docs/grammar.ebnf");

/// The scene shown by `iso-scene help`; a test compiles it.
const EXAMPLE: &str = "version 1;
object crate (1, 1) {
    box at (0, 0, 0) size (w, d, 1) material wood;
}
render iso;
scale 32;
place crate at (0, 0, 0) rot 0;";

/// Where a topic's text comes from.
enum Source {
    /// A spec section: the heading line's prefix, and whether its
    /// subsections are included.
    Spec(&'static str, bool),
    /// A whole document.
    Document(&'static str),
    /// `iso-scene limits`.
    Limits,
    /// The command-line help.
    Commands,
}

struct Topic {
    name: &'static str,
    about: &'static str,
    source: Source,
}

const TOPICS: &[Topic] = &[
    Topic {
        name: "files",
        about: "file kinds: .model, .scene, .walls",
        source: Source::Spec("## 2.", true),
    },
    Topic {
        name: "lexical",
        about: "comments, numbers, strings, colours, keywords",
        source: Source::Spec("## 3.", true),
    },
    Topic {
        name: "values",
        about: "types, let/var, scopes",
        source: Source::Spec("## 4.", true),
    },
    Topic {
        name: "scene",
        about: "render, scale, canvas, sun, place ... anchor",
        source: Source::Spec("### 5.1", true),
    },
    Topic {
        name: "model",
        about: "object name (w, d) { ... }, origin",
        source: Source::Spec("### 5.2", true),
    },
    Topic {
        name: "shapes",
        about: "box, cyl, sphere, slantbox, slantcyl, ramp",
        source: Source::Spec("## 6.", false),
    },
    Topic {
        name: "materials",
        about: "built-in materials and TOML material files",
        source: Source::Spec("### 6.1", true),
    },
    Topic {
        name: "mirror",
        about: "mirror x / mirror y",
        source: Source::Spec("### 6.2 Зеркало", true),
    },
    Topic {
        name: "merge",
        about: "reusable parts (use) and merged surfaces (merge)",
        source: Source::Spec("### 6.2 Типовые", true),
    },
    Topic {
        name: "expressions",
        about: "operators and their precedence",
        source: Source::Spec("### 7.1", true),
    },
    Topic {
        name: "statements",
        about: "let, var, assignment, for, if",
        source: Source::Spec("### 7.2", true),
    },
    Topic {
        name: "evaluation",
        about: "evaluation order and fuel",
        source: Source::Spec("## 8.", true),
    },
    Topic {
        name: "limits",
        about: "every limit and its error code",
        source: Source::Limits,
    },
    Topic {
        name: "errors",
        about: "diagnostic codes",
        source: Source::Spec("## 10.", true),
    },
    Topic {
        name: "backends",
        about: "iso, top/front/side, voxel and Tiled output",
        source: Source::Spec("## 11.", true),
    },
    Topic {
        name: "walls",
        about: "the .walls format and Tiled wall layers",
        source: Source::Document(WALLS),
    },
    Topic {
        name: "grammar",
        about: "the normative EBNF grammar",
        source: Source::Document(GRAMMAR),
    },
    Topic {
        name: "commands",
        about: "command-line commands and options",
        source: Source::Commands,
    },
];

/// The section of `document` under the heading that starts with `heading`,
/// up to the next heading of the same or a higher level (or of any level
/// when `subsections` is false).
#[must_use]
pub fn section<'a>(document: &'a str, heading: &str, subsections: bool) -> Option<&'a str> {
    let start = document
        .match_indices('\n')
        .map(|(index, _)| index + 1)
        .chain(std::iter::once(0))
        .find(|&index| document[index..].starts_with(heading))?;
    let level = heading.chars().take_while(|&c| c == '#').count();
    let body = &document[start..];
    let first_line = body.find('\n').map_or(body.len(), |end| end + 1);
    let end = body[first_line..]
        .match_indices("\n#")
        .map(|(index, _)| first_line + index + 1)
        .find(|&index| {
            let hashes = body[index..].chars().take_while(|&c| c == '#').count();
            !subsections || hashes <= level
        })
        .unwrap_or(body.len());
    Some(body[..end].trim_end())
}

/// What a help topic resolves to.
#[derive(Debug, PartialEq, Eq)]
pub enum Help {
    /// Text to show.
    Text(String),
    /// The command-line help, which only the CLI knows.
    Commands,
}

/// Every topic name, in listing order.
#[must_use]
pub fn names() -> Vec<&'static str> {
    TOPICS.iter().map(|topic| topic.name).collect()
}

/// The topic list and a minimal scene.
#[must_use]
pub fn index() -> String {
    let mut out =
        String::from("iso-scene help <topic> prints one section of docs/spec.md.\n\nTopics:\n");
    let width = TOPICS
        .iter()
        .map(|topic| topic.name.len())
        .max()
        .unwrap_or(0);
    for topic in TOPICS {
        let _ = writeln!(out, "  {:width$}  {}", topic.name, topic.about);
    }
    out.push_str("\nA minimal scene:\n\n");
    for line in EXAMPLE.lines() {
        let _ = writeln!(out, "  {line}");
    }
    out.push_str(
        "\nBuild it with `iso-scene render crate.scene -o crate.png`, then look at the\n\
         image before changing the scene again.\n",
    );
    out
}

/// The text of topic `name`.
///
/// # Errors
///
/// Returns a message naming the known topics for an unknown one.
pub fn topic(name: &str) -> Result<Help, String> {
    let Some(topic) = TOPICS.iter().find(|topic| topic.name == name) else {
        return Err(format!(
            "unknown help topic `{name}`; topics: {}",
            names().join(", ")
        ));
    };
    Ok(match topic.source {
        Source::Spec(heading, subsections) => Help::Text(
            section(SPEC, heading, subsections)
                .ok_or_else(|| format!("help topic `{name}` is missing from the spec"))?
                .to_owned(),
        ),
        Source::Document(text) => Help::Text(text.trim_end().to_owned()),
        Source::Limits => Help::Text(crate::limits::text()),
        Source::Commands => Help::Commands,
    })
}

#[cfg(test)]
mod tests {
    use super::{EXAMPLE, SPEC, Source, TOPICS, section};

    #[test]
    fn the_example_compiles() {
        let program = scene_syntax::parse(EXAMPLE).expect("example parses");
        let here = std::path::Path::new(".");
        let scene = scene_core::compile_scene(&program, here, here).expect("example compiles");
        assert_eq!(scene.instances.len(), 1);
    }

    #[test]
    fn every_spec_topic_finds_its_section() {
        for topic in TOPICS {
            if let Source::Spec(heading, subsections) = topic.source {
                let text = section(SPEC, heading, subsections)
                    .unwrap_or_else(|| panic!("no section for `{}`", topic.name));
                assert!(text.starts_with(heading), "{}", topic.name);
                assert!(text.lines().count() > 2, "`{}` is empty", topic.name);
            }
        }
    }

    #[test]
    fn a_section_stops_at_the_next_heading() {
        let document = "# A\nintro\n## 1. One\none\n### 1.1 Sub\nsub\n## 2. Two\ntwo\n";
        assert_eq!(
            section(document, "## 1.", true),
            Some("## 1. One\none\n### 1.1 Sub\nsub")
        );
        assert_eq!(section(document, "## 1.", false), Some("## 1. One\none"));
        assert_eq!(section(document, "## 2.", true), Some("## 2. Two\ntwo"));
        assert_eq!(section(document, "## 3.", true), None);
    }

    #[test]
    fn shapes_lists_every_shape_without_the_material_section() {
        let text = section(SPEC, "## 6.", false).expect("shapes");
        for shape in ["box", "cyl", "sphere", "slantbox", "slantcyl", "ramp"] {
            assert!(text.contains(&format!("`{shape}`")), "{shape}");
        }
        assert!(!text.contains("### 6.1"));
    }
}
