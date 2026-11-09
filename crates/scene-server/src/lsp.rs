//! A language server for `.scene`, `.model` and `.walls` over stdio.
//!
//! Full-text sync; diagnostics on open, change and save; hover from the
//! spec; document symbols; go to definition (also into included models);
//! completion of keywords, shapes, materials, objects and anchors.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::analysis::{
    self, BUILTIN_MATERIALS, LineIndex, SHAPE_SNIPPETS, Symbol, SymbolKind, check, hover,
    included_symbols, keywords, parse_lenient, symbols, word_at,
};
use crate::rpc::{self, Framing, Incoming};

struct Server {
    /// Open documents by URI.
    documents: BTreeMap<String, String>,
    /// Directory includes must stay inside.
    root: PathBuf,
}

/// Serve LSP requests from `input` until `exit` or the end of the stream.
///
/// # Errors
///
/// Returns I/O errors of the streams.
pub fn run(mut input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    let mut server = Server {
        documents: BTreeMap::new(),
        root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    loop {
        let message = match rpc::read(&mut input, Framing::Headers)? {
            Incoming::End => return Ok(()),
            Incoming::Malformed(error) => {
                rpc::write(
                    &mut output,
                    Framing::Headers,
                    &rpc::error(&Value::Null, rpc::PARSE_ERROR, error),
                )?;
                continue;
            }
            Incoming::Message(message) => message,
        };
        let method = message["method"].as_str().unwrap_or_default();
        if method == "exit" {
            return Ok(());
        }
        let params = &message["params"];
        let replies = server.handle(method, params);
        if let Some(id) = message.get("id") {
            let reply = match replies.response {
                Some(Ok(result)) => rpc::response(id, &result),
                Some(Err((code, text))) => rpc::error(id, code, text),
                None => rpc::error(
                    id,
                    rpc::METHOD_NOT_FOUND,
                    format!("unknown method `{method}`"),
                ),
            };
            rpc::write(&mut output, Framing::Headers, &reply)?;
        }
        for notification in replies.notifications {
            rpc::write(&mut output, Framing::Headers, &notification)?;
        }
    }
}

#[derive(Default)]
struct Replies {
    response: Option<Result<Value, (i64, String)>>,
    notifications: Vec<Value>,
}

impl Server {
    fn handle(&mut self, method: &str, params: &Value) -> Replies {
        let mut replies = Replies::default();
        match method {
            "initialize" => {
                if let Some(root) = params["rootUri"].as_str().and_then(uri_to_path) {
                    self.root = root;
                }
                replies.response = Some(Ok(json!({
                    "capabilities": {
                        "textDocumentSync": {"openClose": true, "change": 1, "save": {"includeText": false}},
                        "hoverProvider": true,
                        "definitionProvider": true,
                        "documentSymbolProvider": true,
                        "completionProvider": {"triggerCharacters": [" "]}
                    },
                    "serverInfo": {"name": "iso-scene", "version": env!("CARGO_PKG_VERSION")}
                })));
            }
            "shutdown" => replies.response = Some(Ok(Value::Null)),
            "textDocument/didOpen" => {
                let document = &params["textDocument"];
                if let (Some(uri), Some(text)) =
                    (document["uri"].as_str(), document["text"].as_str())
                {
                    self.documents.insert(uri.to_owned(), text.to_owned());
                    replies.notifications.push(self.diagnostics(uri));
                }
            }
            "textDocument/didChange" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let text = params["contentChanges"]
                    .as_array()
                    .and_then(|changes| changes.last())
                    .and_then(|change| change["text"].as_str());
                if let (Some(document), Some(text)) = (self.documents.get_mut(uri), text) {
                    text.clone_into(document);
                    replies.notifications.push(self.diagnostics(uri));
                }
            }
            "textDocument/didSave" => {
                // A saved model can change every scene that includes it.
                let uris: Vec<String> = self.documents.keys().cloned().collect();
                replies.notifications = uris.iter().map(|uri| self.diagnostics(uri)).collect();
            }
            "textDocument/didClose" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                self.documents.remove(uri);
                replies.notifications.push(rpc::notification(
                    "textDocument/publishDiagnostics",
                    &json!({"uri": uri, "diagnostics": []}),
                ));
            }
            "textDocument/hover" => replies.response = Some(self.at(params).map(|at| at.hover())),
            "textDocument/definition" => {
                replies.response = Some(self.at(params).map(|at| at.definition()));
            }
            "textDocument/documentSymbol" => {
                replies.response = Some(self.document(params).map(|(_, text)| {
                    let index = LineIndex::new(text);
                    let Some(program) = parse_lenient(text) else {
                        return json!([]);
                    };
                    symbols(text, &program)
                        .iter()
                        .map(|symbol| document_symbol(&index, text, symbol))
                        .collect()
                }));
            }
            "textDocument/completion" => {
                replies.response = Some(self.at(params).map(|at| at.completion()));
            }
            // Notifications we do not need (`initialized`, `$/...`) are ignored;
            // unknown requests get METHOD_NOT_FOUND from `run`.
            _ => {}
        }
        replies
    }

    fn document<'a>(&'a self, params: &Value) -> Result<(&'a str, &'a str), (i64, String)> {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
        self.documents
            .get_key_value(uri)
            .map(|(uri, text)| (uri.as_str(), text.as_str()))
            .ok_or_else(|| (rpc::INVALID_PARAMS, format!("`{uri}` is not open")))
    }

    fn at<'a>(&'a self, params: &Value) -> Result<At<'a>, (i64, String)> {
        let (uri, text) = self.document(params)?;
        let line = u32::try_from(params["position"]["line"].as_u64().unwrap_or(0)).unwrap_or(0);
        let column =
            u32::try_from(params["position"]["character"].as_u64().unwrap_or(0)).unwrap_or(0);
        let index = LineIndex::new(text);
        let offset = index.offset(text, line, column);
        Ok(At {
            uri,
            path: uri_to_path(uri).unwrap_or_else(|| PathBuf::from(uri)),
            text,
            index,
            offset,
        })
    }

    fn diagnostics(&self, uri: &str) -> Value {
        let text = self.documents.get(uri).map_or("", String::as_str);
        let path = uri_to_path(uri).unwrap_or_else(|| PathBuf::from(uri));
        let index = LineIndex::new(text);
        let diagnostics: Vec<Value> = check(&path, text, &self.root)
            .iter()
            .map(|problem| {
                json!({
                    "range": range(&index, text, problem.start, problem.end),
                    "severity": 1,
                    "code": problem.code,
                    "source": "iso-scene",
                    "message": problem.message
                })
            })
            .collect();
        rpc::notification(
            "textDocument/publishDiagnostics",
            &json!({"uri": uri, "diagnostics": diagnostics}),
        )
    }
}

/// A position in an open document.
struct At<'a> {
    uri: &'a str,
    path: PathBuf,
    text: &'a str,
    index: LineIndex,
    offset: usize,
}

impl At<'_> {
    fn hover(&self) -> Value {
        match hover(&self.path, self.text, self.offset) {
            Some((markdown, start, end)) => json!({
                "contents": {"kind": "markdown", "value": markdown},
                "range": range(&self.index, self.text, start, end)
            }),
            None => Value::Null,
        }
    }

    fn definition(&self) -> Value {
        let Some((word, ..)) = word_at(self.text, self.offset) else {
            return Value::Null;
        };
        let Some(program) = parse_lenient(self.text) else {
            return Value::Null;
        };
        if let Some(symbol) = symbols(self.text, &program)
            .into_iter()
            .find(|symbol| symbol.name == word)
        {
            return location(self.uri, &self.index, self.text, &symbol);
        }
        included_symbols(&self.path, &program)
            .into_iter()
            .find(|external| external.symbol.name == word)
            .map_or(Value::Null, |external| {
                location(
                    &path_to_uri(&external.path),
                    &LineIndex::new(&external.text),
                    &external.text,
                    &external.symbol,
                )
            })
    }

    fn completion(&self) -> Value {
        let line_start = self.text[..self.offset].rfind('\n').map_or(0, |at| at + 1);
        let before = self.text[line_start..self.offset]
            .trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
        let previous = before.split_whitespace().next_back().unwrap_or_default();
        let mut items = Vec::new();
        let mut push = |label: &str, kind: u32, detail: &str| {
            items.push(json!({"label": label, "kind": kind, "detail": detail}));
        };
        match previous {
            "anchor" => {
                for anchor in ["back", "center", "front", "left", "right", "cell"] {
                    push(anchor, 20, "anchor");
                }
            }
            "render" => {
                for view in ["iso", "top", "front", "side"] {
                    push(view, 20, "view");
                }
            }
            "material" => {
                for name in BUILTIN_MATERIALS {
                    push(name, 21, "built-in material");
                }
                for symbol in self.declared(SymbolKind::Material) {
                    push(&symbol, 21, "material");
                }
            }
            "place" | "use" => {
                for symbol in self.declared(SymbolKind::Object) {
                    push(&symbol, 7, "object");
                }
            }
            _ => {
                // Shapes come once, as snippets below.
                for word in keywords()
                    .into_iter()
                    .filter(|word| SHAPE_SNIPPETS.iter().all(|(shape, _)| shape != word))
                {
                    push(word, 14, "keyword");
                }
                for (shape, snippet) in SHAPE_SNIPPETS {
                    items.push(json!({
                        "label": shape,
                        "kind": 15,
                        "detail": analysis::keyword_doc(shape).unwrap_or_default(),
                        "insertText": snippet,
                        "insertTextFormat": 2
                    }));
                }
            }
        }
        Value::Array(items)
    }

    /// Names of the declarations of `kind` here and in included models.
    fn declared(&self, kind: SymbolKind) -> Vec<String> {
        let Some(program) = parse_lenient(self.text) else {
            return Vec::new();
        };
        let mut names: Vec<String> = symbols(self.text, &program)
            .into_iter()
            .filter(|symbol| symbol.kind == kind)
            .map(|symbol| symbol.name)
            .collect();
        names.extend(
            included_symbols(&self.path, &program)
                .into_iter()
                .filter(|external| external.symbol.kind == kind)
                .map(|external| external.symbol.name),
        );
        names
    }
}

fn range(index: &LineIndex, text: &str, start: usize, end: usize) -> Value {
    let (start_line, start_column) = index.position(text, start);
    let (end_line, end_column) = index.position(text, end);
    json!({
        "start": {"line": start_line, "character": start_column},
        "end": {"line": end_line, "character": end_column}
    })
}

fn location(uri: &str, index: &LineIndex, text: &str, symbol: &Symbol) -> Value {
    json!({"uri": uri, "range": range(index, text, symbol.name_start, symbol.name_end)})
}

fn document_symbol(index: &LineIndex, text: &str, symbol: &Symbol) -> Value {
    let kind = match symbol.kind {
        SymbolKind::Object => 5,
        SymbolKind::Material => 14,
        SymbolKind::Animation => 24,
    };
    json!({
        "name": symbol.name,
        "detail": symbol.detail,
        "kind": kind,
        "range": range(index, text, symbol.start, symbol.end),
        "selectionRange": range(index, text, symbol.name_start, symbol.name_end)
    })
}

/// The path of a `file://` URI, percent-decoded.
#[must_use]
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = rest.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    let path = String::from_utf8(decoded).ok()?;
    // `file:///C:/x` on Windows.
    let path = match path.as_bytes() {
        [b'/', drive, b':', ..] if drive.is_ascii_alphabetic() => path[1..].to_owned(),
        _ => path,
    };
    Some(PathBuf::from(path))
}

/// The `file://` URI of a path, percent-encoding what URIs reserve.
#[must_use]
pub fn path_to_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut uri = String::from("file://");
    if !text.starts_with('/') {
        uri.push('/');
    }
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~:".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    uri
}
