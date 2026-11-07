//! A whole LSP session against the server, over in-memory streams.

use std::fs;
use std::path::PathBuf;

use scene_server::lsp::{path_to_uri, run};
use scene_server::rpc::{self, Framing, Incoming};
use serde_json::{Value, json};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iso-scene-lsp-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch");
    dir
}

/// Send `requests`, return every message the server wrote.
fn session(requests: &[Value]) -> Vec<Value> {
    let mut input = Vec::new();
    for request in requests {
        rpc::write(&mut input, Framing::Headers, request).expect("write");
    }
    let mut output = Vec::new();
    run(input.as_slice(), &mut output).expect("run");
    let mut reader = output.as_slice();
    let mut messages = Vec::new();
    while let Incoming::Message(message) = rpc::read(&mut reader, Framing::Headers).expect("read") {
        messages.push(message);
    }
    messages
}

fn request(id: u64, method: &str, params: Value) -> Value {
    let mut message = json!({"jsonrpc": "2.0", "id": id, "method": method});
    message["params"] = params;
    message
}

fn notify(method: &str, params: Value) -> Value {
    let mut message = json!({"jsonrpc": "2.0", "method": method});
    message["params"] = params;
    message
}

fn reply(messages: &[Value], id: u64) -> &Value {
    messages
        .iter()
        .find(|message| message["id"] == id)
        .unwrap_or_else(|| panic!("no reply {id}"))
}

#[test]
fn a_session_checks_hovers_completes_and_jumps_into_models() {
    let dir = scratch("session");
    fs::write(
        dir.join("desk.model"),
        "version 1;\nobject desk (5, 2) {\n    box at (0, 0, 0) size (w, d, 1);\n}\n",
    )
    .expect("model");
    let scene = dir.join("room.scene");
    let uri = path_to_uri(&scene);
    let broken =
        "version 1;\ninclude \"desk.model\";\nrender iso; scale 8;\nplace sofa at (0, 0, 0);\n";
    let fixed = "version 1;\ninclude \"desk.model\";\nrender iso; scale 8;\nplace desk at (0, 0, 0);\nplace ";
    let at = |line: u32, character: u32| json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}});
    let messages = session(&[
        request(
            1,
            "initialize",
            json!({"rootUri": path_to_uri(&dir), "capabilities": {}}),
        ),
        notify("initialized", json!({})),
        notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": "iso-scene", "version": 1, "text": broken}}),
        ),
        notify(
            "textDocument/didChange",
            json!({"textDocument": {"uri": uri, "version": 2}, "contentChanges": [{"text": fixed}]}),
        ),
        request(2, "textDocument/hover", at(3, 7)),
        request(3, "textDocument/definition", at(3, 7)),
        request(4, "textDocument/completion", at(4, 6)),
        request(
            5,
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri}}),
        ),
        request(6, "textDocument/hover", at(2, 13)),
        request(7, "no/such/method", json!({})),
        request(8, "shutdown", Value::Null),
        notify("exit", Value::Null),
    ]);

    assert_eq!(
        reply(&messages, 1)["result"]["capabilities"]["hoverProvider"],
        true
    );
    let diagnostics: Vec<&Value> = messages
        .iter()
        .filter(|message| message["method"] == "textDocument/publishDiagnostics")
        .collect();
    // Open: unknown object `sofa` on line 3. Change: `sofa` is fixed and the
    // unfinished `place` on line 4 is the only problem.
    let first = &diagnostics[0]["params"]["diagnostics"][0];
    assert_eq!(first["code"], "E0301");
    assert_eq!(first["range"]["start"]["line"], 3);
    let second = &diagnostics[1]["params"]["diagnostics"][0];
    assert_eq!(second["code"], "E0201");
    assert_eq!(second["range"]["start"]["line"], 4);

    let hover = &reply(&messages, 2)["result"]["contents"]["value"];
    assert!(
        hover
            .as_str()
            .expect("hover")
            .contains("object desk (5, 2)"),
        "{hover}"
    );
    let definition = &reply(&messages, 3)["result"];
    assert_eq!(definition["uri"], path_to_uri(&dir.join("desk.model")));
    assert_eq!(
        definition["range"]["start"],
        json!({"line": 1, "character": 7})
    );
    let completion = reply(&messages, 4)["result"].as_array().expect("items");
    assert!(
        completion.iter().any(|item| item["label"] == "desk"),
        "{completion:?}"
    );
    assert_eq!(reply(&messages, 5)["result"], json!([]));
    let scale = &reply(&messages, 6)["result"]["contents"]["value"];
    assert!(scale.as_str().expect("scale").contains("1..256"), "{scale}");
    assert_eq!(reply(&messages, 7)["error"]["code"], rpc::METHOD_NOT_FOUND);
    assert_eq!(reply(&messages, 8)["result"], Value::Null);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn garbage_gets_a_parse_error_and_the_server_keeps_going() {
    let mut input = b"Content-Length: 3\r\n\r\n{x}".to_vec();
    rpc::write(
        &mut input,
        Framing::Headers,
        &request(1, "shutdown", Value::Null),
    )
    .expect("write");
    let mut output = Vec::new();
    run(input.as_slice(), &mut output).expect("run");
    let mut reader = output.as_slice();
    let Incoming::Message(first) = rpc::read(&mut reader, Framing::Headers).expect("read") else {
        panic!("no reply");
    };
    assert_eq!(first["error"]["code"], rpc::PARSE_ERROR);
    let Incoming::Message(second) = rpc::read(&mut reader, Framing::Headers).expect("read") else {
        panic!("no reply");
    };
    assert_eq!(second["id"], 1);
}
