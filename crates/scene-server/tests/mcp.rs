//! A whole MCP session against the server, over in-memory streams.

use std::fs;
use std::path::{Path, PathBuf};

use scene_server::mcp::serve;
use scene_server::rpc::{self, Framing, Incoming};
use serde_json::{Value, json};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iso-scene-mcp-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn session(root: &Path, requests: &[Value]) -> Vec<Value> {
    let mut input = Vec::new();
    for request in requests {
        rpc::write(&mut input, Framing::Lines, request).expect("write");
    }
    let mut output = Vec::new();
    serve(&mut input.as_slice(), &mut output, root).expect("serve");
    let mut reader = output.as_slice();
    let mut replies = Vec::new();
    while let Incoming::Message(message) = rpc::read(&mut reader, Framing::Lines).expect("read") {
        replies.push(message);
    }
    replies
}

fn call(id: u64, tool: &str, arguments: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": tool, "arguments": arguments}})
}

fn text(reply: &Value) -> &str {
    reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
}

const ROOM: &str = "version 1;
object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; }
render iso;
scale 8;
animation slide (s) frames 3 duration 40;
place b at (s * 2, 0, 0);
";

#[test]
fn an_agent_checks_renders_exports_and_reads_the_spec() {
    let root = scratch("session");
    fs::write(root.join("room.scene"), ROOM).expect("scene");
    fs::write(
        root.join("broken.scene"),
        "version 1;\nrender iso; scale 8;\nplace nothing at (0, 0, 0);\n",
    )
    .expect("scene");
    let replies = session(
        &root,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            call(3, "check", &json!({"path": "room.scene"})),
            call(4, "check", &json!({"path": "broken.scene"})),
            call(5, "render", &json!({"path": "room.scene"})),
            call(
                6,
                "export",
                &json!({"path": "room.scene", "format": "animate", "out": "out/anim"}),
            ),
            call(
                7,
                "export",
                &json!({"path": "room.scene", "format": "tiled"}),
            ),
            call(
                8,
                "export",
                &json!({"path": "room.scene", "format": "voxel"}),
            ),
            call(9, "help", &json!({"topic": "shapes"})),
            call(10, "limits", &json!({})),
            call(11, "render", &json!({"path": "../outside.scene"})),
            call(
                12,
                "render",
                &json!({"path": "room.scene", "out": "/tmp/elsewhere.png"}),
            ),
            call(13, "nope", &json!({})),
            json!({"jsonrpc": "2.0", "id": 14, "method": "ping"}),
        ],
    );
    let reply = |id: u64| {
        replies
            .iter()
            .find(|reply| reply["id"] == id)
            .unwrap_or_else(|| panic!("no reply {id}"))
    };
    assert_eq!(reply(1)["result"]["protocolVersion"], "2025-03-26");
    let tools: Vec<&str> = reply(2)["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(tools, ["check", "render", "export", "help", "limits"]);
    assert!(
        text(reply(3)).starts_with("ok: 1 instances"),
        "{}",
        text(reply(3))
    );
    assert!(text(reply(3)).contains("slide (3 frames)"));
    assert_eq!(reply(4)["result"]["isError"], true);
    assert!(
        text(reply(4)).starts_with("broken.scene:3:1: error[E0301]"),
        "{}",
        text(reply(4))
    );
    // The render comes back as a PNG image the agent can look at.
    let image = &reply(5)["result"]["content"][1];
    assert_eq!(image["type"], "image");
    assert!(
        image["data"]
            .as_str()
            .expect("data")
            .starts_with("iVBORw0KGgo")
    );
    assert!(root.join("out/room.png").exists());
    assert!(
        root.join("out/anim/slide/slide.png").exists(),
        "{}",
        text(reply(6))
    );
    assert!(
        root.join("out/room/scene.tmj").exists(),
        "{}",
        text(reply(7))
    );
    assert!(root.join("out/room.vox").exists(), "{}", text(reply(8)));
    assert!(text(reply(9)).contains("`slantbox`"));
    assert!(text(reply(10)).contains("E0602"));
    // Paths outside the project are refused, for reading and writing.
    for id in [11, 12] {
        assert_eq!(reply(id)["result"]["isError"], true, "{}", text(reply(id)));
        assert!(text(reply(id)).contains("inside the project root"));
    }
    assert_eq!(reply(13)["error"]["code"], rpc::INVALID_PARAMS);
    assert_eq!(reply(14)["result"], json!({}));
    let _ = fs::remove_dir_all(root);
}
