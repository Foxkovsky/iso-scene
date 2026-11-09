//! An MCP server over stdio, so agents can check, render and export scenes
//! and read the spec and limits without parsing CLI output.
//!
//! Every path is resolved against the server's working directory (the
//! project root) and must stay inside it, for reading and for writing.

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Component, Path, PathBuf};

use scene_syntax::Diagnostic;
use scene_syntax::ast::{Program, RenderKind};
use serde_json::{Value, json};

use crate::help::{self, Help};
use crate::rpc::{self, Framing, Incoming};

/// Protocol revisions this server speaks, newest first.
const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// Images larger than this are written but not returned inline.
const MAX_INLINE_PIXELS: u64 = 4_000_000;

const INSTRUCTIONS: &str = "iso-scene compiles .scene/.model files to pixel art. \
Work in a loop: edit the scene, call `check`, call `render` and look at the image, \
then fix one thing. Use `help` for one section of the spec instead of guessing syntax, \
and `limits` for the limits.";

/// Serve MCP requests from `input` until the stream ends.
///
/// # Errors
///
/// Returns I/O errors of the streams.
pub fn run(mut input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    let root = std::env::current_dir()?;
    serve(&mut input, &mut output, &root)
}

/// [`run`] with an explicit project root.
///
/// # Errors
///
/// Returns I/O errors of the streams.
pub fn serve(input: &mut impl BufRead, output: &mut impl Write, root: &Path) -> io::Result<()> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    loop {
        let message = match rpc::read(input, Framing::Lines)? {
            Incoming::End => return Ok(()),
            Incoming::Malformed(error) => {
                let reply = rpc::error(&Value::Null, rpc::PARSE_ERROR, error);
                rpc::write(output, Framing::Lines, &reply)?;
                continue;
            }
            Incoming::Message(message) => message,
        };
        let Some(id) = message.get("id") else {
            continue; // notifications need no reply
        };
        let method = message["method"].as_str().unwrap_or_default();
        let params = &message["params"];
        let reply = match method {
            "initialize" => {
                let asked = params["protocolVersion"].as_str().unwrap_or_default();
                let version = PROTOCOL_VERSIONS
                    .iter()
                    .find(|version| **version == asked)
                    .unwrap_or(&PROTOCOL_VERSIONS[0]);
                rpc::response(
                    id,
                    &json!({
                        "protocolVersion": version,
                        "capabilities": {"tools": {}},
                        "serverInfo": {"name": "iso-scene", "version": env!("CARGO_PKG_VERSION")},
                        "instructions": INSTRUCTIONS
                    }),
                )
            }
            "ping" => rpc::response(id, &json!({})),
            "tools/list" => rpc::response(id, &json!({"tools": tools()})),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default();
                match call(name, &params["arguments"], &root) {
                    Some(result) => rpc::response(id, &result),
                    None => rpc::error(id, rpc::INVALID_PARAMS, format!("unknown tool `{name}`")),
                }
            }
            _ => rpc::error(
                id,
                rpc::METHOD_NOT_FOUND,
                format!("unknown method `{method}`"),
            ),
        };
        rpc::write(output, Framing::Lines, &reply)?;
    }
}

fn tools() -> Value {
    let path = json!({"type": "string", "description": "a .scene or .model file, relative to the project root"});
    json!([
        {
            "name": "check",
            "description": "Parse and evaluate a .scene, .model or .walls file. Returns `ok` with a summary, or the first error as `file:line:column: error[CODE]: message`.",
            "inputSchema": {"type": "object", "properties": {"path": path}, "required": ["path"]}
        },
        {
            "name": "render",
            "description": "Render a scene or model to PNG and return the image, so you can look at it. `view` overrides the scene's render (iso, top, front, side).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path,
                    "view": {"type": "string", "enum": ["iso", "top", "front", "side"]},
                    "out": {"type": "string", "description": "PNG path; default out/<stem>.png"}
                },
                "required": ["path"]
            }
        },
        {
            "name": "export",
            "description": "Export a scene: `tiled` (isometric Tiled map), `voxel` (MagicaVoxel .vox) or `animate` (the scene's `animation` directives as strips and Tiled tilesets).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path,
                    "format": {"type": "string", "enum": ["tiled", "voxel", "animate"]},
                    "out": {"type": "string", "description": "output directory (tiled, animate) or .vox file; default under out/"},
                    "walls": {"type": "string", "description": "tiled: a .walls file"}
                },
                "required": ["path", "format"]
            }
        },
        {
            "name": "help",
            "description": "One section of the language spec. Without `topic`, the list of topics and a minimal scene.",
            "inputSchema": {"type": "object", "properties": {"topic": {"type": "string", "enum": help::names()}}}
        },
        {
            "name": "limits",
            "description": "Every limit of the language and the exporters, with its error code.",
            "inputSchema": {"type": "object", "properties": {}}
        }
    ])
}

/// The result of tool `name`, or `None` for an unknown tool.
fn call(name: &str, arguments: &Value, root: &Path) -> Option<Value> {
    let result = match name {
        "check" => check(arguments, root),
        "render" => render(arguments, root),
        "export" => export(arguments, root),
        "help" => match arguments["topic"].as_str() {
            None => Ok(text(help::index())),
            Some(topic) => match help::topic(topic) {
                Ok(Help::Text(body)) => Ok(text(body)),
                Ok(Help::Commands) => Ok(text(
                    "Run `iso-scene --help` for the command-line help.".to_owned(),
                )),
                Err(message) => Err(message),
            },
        },
        "limits" => Ok(text(crate::limits::text())),
        _ => return None,
    };
    Some(result.unwrap_or_else(
        |message| json!({"content": [{"type": "text", "text": message}], "isError": true}),
    ))
}

fn text(body: String) -> Value {
    json!({"content": [{"type": "text", "text": Value::String(body)}]})
}

/// A path argument resolved inside the project root.
fn inside(root: &Path, argument: Option<&str>, what: &str) -> Result<PathBuf, String> {
    let argument = argument.ok_or_else(|| format!("`{what}` is required"))?;
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let path = Path::new(argument);
    let relative = path.strip_prefix(&root).unwrap_or(path);
    if relative.is_absolute()
        || relative.has_root()
        || relative.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return Err(format!(
            "`{argument}` must be inside the project root `{}`",
            root.display()
        ));
    }
    let joined = root.join(relative);
    // A symlink inside the root must not lead outside it. The file itself may
    // not exist yet (an export target), so check its nearest existing ancestor.
    let mut existing = joined.as_path();
    loop {
        match existing.canonicalize() {
            Ok(canonical) => {
                if !canonical.starts_with(&root) {
                    return Err(format!(
                        "`{argument}` resolves outside the project root `{}`",
                        root.display()
                    ));
                }
                break;
            }
            Err(_) => {
                existing = existing
                    .parent()
                    .ok_or_else(|| format!("`{argument}` has no ancestor in the project root"))?;
            }
        }
    }
    Ok(joined)
}

/// A loaded source file.
struct Source {
    path: PathBuf,
    shown: String,
    text: String,
}

impl Source {
    fn load(root: &Path, arguments: &Value) -> Result<Self, String> {
        let path = inside(root, arguments["path"].as_str(), "path")?;
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("cannot read `{}`: {error}", path.display()))?;
        let shown = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        Ok(Self { path, shown, text })
    }

    fn directory(&self) -> &Path {
        self.path.parent().unwrap_or_else(|| Path::new("."))
    }

    /// `file:line:column: error[CODE]: message`, as the CLI prints it.
    fn report(&self, diagnostic: &Diagnostic) -> String {
        let offset = (diagnostic.span.start as usize).min(self.text.len());
        let before = self.text.get(..offset).unwrap_or(&self.text);
        let line = before.matches('\n').count() + 1;
        let column = before
            .rsplit('\n')
            .next()
            .map_or(0, |line| line.chars().count())
            + 1;
        format!(
            "{}:{line}:{column}: error[{}]: {}",
            self.shown, diagnostic.code, diagnostic.message
        )
    }

    fn program(&self) -> Result<Program, String> {
        scene_syntax::parse(&self.text).map_err(|error| self.report(&error))
    }

    /// The scene, or a model laid out as a preview scene.
    fn scene(&self, root: &Path) -> Result<scene_core::SceneGraph, String> {
        let program = self.program()?;
        match crate::analysis::FileKind::of(&self.path) {
            crate::analysis::FileKind::Scene => {
                scene_core::compile_scene(&program, self.directory(), root)
                    .map_err(|error| self.report(&error))
            }
            crate::analysis::FileKind::Model => {
                let model = scene_core::compile_model(&program, self.directory())
                    .map_err(|error| self.report(&error))?;
                crate::preview::model_scene(&model)
            }
            _ => Err(format!("`{}` is not a .scene or .model file", self.shown)),
        }
    }

    fn default_output(&self, root: &Path, extension: &str) -> PathBuf {
        let stem = self.path.file_stem().map_or_else(
            || "scene".into(),
            |stem| stem.to_string_lossy().into_owned(),
        );
        let name = if extension.is_empty() {
            stem
        } else {
            format!("{stem}.{extension}")
        };
        root.join("out").join(name)
    }
}

fn check(arguments: &Value, root: &Path) -> Result<Value, String> {
    let source = Source::load(root, arguments)?;
    if crate::analysis::FileKind::of(&source.path) == crate::analysis::FileKind::Walls {
        return scene_walls::parse(&source.text, source.directory(), root)
            .map(|layout| {
                text(format!(
                    "ok: {}x{} cells, {} walls",
                    layout.columns,
                    layout.rows,
                    layout.cells.len()
                ))
            })
            .map_err(|error| source.report(&error));
    }
    let scene = source.scene(root)?;
    let size = scene.bounds.size();
    let animations: Vec<String> = scene
        .animations
        .iter()
        .map(|animation| format!("{} ({} frames)", animation.name, animation.frames))
        .collect();
    Ok(text(format!(
        "ok: {} instances, {:?} render, bounds {:.2} x {:.2} x {:.2} blocks{}",
        scene.instances.len(),
        scene.render,
        size[0],
        size[1],
        size[2],
        if animations.is_empty() {
            String::new()
        } else {
            format!(", animations: {}", animations.join(", "))
        }
    )))
}

fn render(arguments: &Value, root: &Path) -> Result<Value, String> {
    let source = Source::load(root, arguments)?;
    let mut scene = source.scene(root)?;
    if let Some(view) = arguments["view"].as_str() {
        scene.render = match view {
            "iso" => RenderKind::Iso,
            "top" => RenderKind::Top,
            "front" => RenderKind::Front,
            "side" => RenderKind::Side,
            other => {
                return Err(format!(
                    "unknown view `{other}`; use iso, top, front or side"
                ));
            }
        };
    }
    let out = match arguments["out"].as_str() {
        Some(out) => inside(root, Some(out), "out")?,
        None => source.default_output(root, "png"),
    };
    let image = match scene.render {
        RenderKind::Iso => scene_backend_iso::render(&scene),
        _ => scene_backend_ortho::render(&scene),
    }
    .map_err(|error| source.report(&error))?;
    create_parent(&out)?;
    scene_render::write_png(&image, &out)?;
    let summary = format!(
        "wrote {} ({}x{}, {} instances)",
        out.strip_prefix(root).unwrap_or(&out).display(),
        image.width,
        image.height,
        scene.instances.len()
    );
    if u64::from(image.width) * u64::from(image.height) > MAX_INLINE_PIXELS {
        return Ok(text(format!("{summary}; too large to show inline")));
    }
    let png =
        fs::read(&out).map_err(|error| format!("cannot read `{}`: {error}", out.display()))?;
    Ok(json!({"content": [
        {"type": "text", "text": summary},
        {"type": "image", "data": base64(&png), "mimeType": "image/png"}
    ]}))
}

#[allow(clippy::too_many_lines)]
fn export(arguments: &Value, root: &Path) -> Result<Value, String> {
    let source = Source::load(root, arguments)?;
    let format = arguments["format"].as_str().unwrap_or_default();
    let out = |default: &str| match arguments["out"].as_str() {
        Some(out) => inside(root, Some(out), "out"),
        None => Ok(source.default_output(root, default)),
    };
    let shown = |path: &Path| {
        path.strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string()
    };
    match format {
        "tiled" => {
            let scene = source.scene(root)?;
            let walls = match arguments["walls"].as_str() {
                Some(walls) => {
                    let file = inside(root, Some(walls), "walls")?;
                    Some(scene_walls::load(&file, root).map_err(|error| {
                        format!("{}: error[{}]: {}", shown(&file), error.code, error.message)
                    })?)
                }
                None => None,
            };
            let directory = out("")?;
            let report = scene_export_tiled::export_with(
                &scene,
                walls.as_ref(),
                false,
                scene_export_tiled::WallOrder::default(),
                &directory,
            )
            .map_err(|error| source.report(&error))?;
            let mut lines = vec![format!(
                "wrote {} ({}x{} cells, {} instances)",
                shown(&report.map),
                report.cells[0],
                report.cells[1],
                report.instances
            )];
            lines.extend(
                report
                    .collisions
                    .iter()
                    .map(|collision| format!("warning: {collision:?}")),
            );
            Ok(text(lines.join("\n")))
        }
        "voxel" => {
            let scene = source.scene(root)?;
            let size = scene.bounds.size();
            let extent = size[0].max(size[1]).max(size[2]).max(1.0);
            let model = scene_backend_voxel::voxelize(&scene, (128.0 / extent).clamp(0.05, 8.0))
                .map_err(|error| source.report(&error))?;
            let file = out("vox")?;
            create_parent(&file)?;
            scene_backend_voxel::write_vox(&model, &file).map_err(|error| source.report(&error))?;
            Ok(text(format!(
                "wrote {} ({}x{}x{}, {} voxels)",
                shown(&file),
                model.size[0],
                model.size[1],
                model.size[2],
                model.voxels.len()
            )))
        }
        "animate" => {
            let program = source.program()?;
            let scene = source.scene(root)?;
            if scene.animations.is_empty() {
                return Err("the scene declares no `animation`".to_owned());
            }
            let directory = out("")?;
            let mut lines = Vec::new();
            for animation in &scene.animations {
                let frames = (0..animation.frames)
                    .map(|frame| {
                        scene_core::compile_scene_frame(
                            &program,
                            source.directory(),
                            root,
                            &animation.name,
                            frame,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| source.report(&error))?;
                let report = scene_export_sprites::animate::export_scene_animation(
                    &frames,
                    &animation.name,
                    animation.duration,
                    animation.looped,
                    &directory.join(&animation.name),
                )
                .map_err(|error| format!("error[{}]: {}", error.code, error.message))?;
                lines.push(format!(
                    "wrote {}: {} frames {}x{}",
                    shown(&report.image),
                    report.frames,
                    report.frame_size[0],
                    report.frame_size[1]
                ));
            }
            Ok(text(lines.join("\n")))
        }
        other => Err(format!(
            "unknown format `{other}`; use tiled, voxel or animate"
        )),
    }
}

fn create_parent(path: &Path) -> Result<(), String> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create `{}`: {error}", parent.display())),
        _ => Ok(()),
    }
}

/// Standard base64 with padding.
#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |value, (index, &byte)| {
                value | u32::from(byte) << (16 - 8 * index)
            });
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(char::from(
                    ALPHABET[(value >> (18 - 6 * index) & 63) as usize],
                ));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{base64, inside};
    use std::fs;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "iso-scene-mcp-inside-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch");
        dir.canonicalize().expect("canonical scratch")
    }

    #[test]
    fn base64_matches_the_rfc_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected);
        }
    }

    #[test]
    fn a_root_relative_path_is_refused() {
        let root = scratch("root-relative");
        // On Windows `/tmp` has a root but no prefix, so `is_absolute()` is
        // false; it still escapes the project when joined.
        #[cfg(windows)]
        let argument = r"\tmp\elsewhere.png";
        #[cfg(unix)]
        let argument = "/tmp/elsewhere.png";
        assert!(inside(&root, Some(argument), "out").is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_parent_component_is_refused() {
        let root = scratch("parent");
        assert!(inside(&root, Some("../outside.png"), "out").is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_absolute_path_inside_the_root_is_allowed() {
        let root = scratch("absolute");
        let file = root.join("scene.scene");
        fs::write(&file, "").expect("write");
        let resolved = inside(&root, file.to_str(), "path").expect("inside");
        assert_eq!(resolved, file);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_outside_the_root_is_refused() {
        use std::os::unix::fs::symlink;
        let root = scratch("symlink-root");
        let outside = scratch("symlink-outside");
        symlink(&outside, root.join("link")).expect("symlink");
        assert!(inside(&root, Some("link/evil.png"), "out").is_err());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }
}
