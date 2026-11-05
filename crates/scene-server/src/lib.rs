//! JSON-lines preview server used by the VS Code extension.
//!
//! One request per line, one response per line. The protocol is independent
//! of the language and mirrors `scene-cli` outputs as base64 payloads.
