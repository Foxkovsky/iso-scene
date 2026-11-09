//! JSON-RPC 2.0 messages over a byte stream.
//!
//! LSP frames each message with a `Content-Length` header; the MCP stdio
//! transport puts one message per line. Both are read with a size limit that
//! is checked before the body is allocated.

use std::io::{self, BufRead, Read, Write};

use serde_json::{Value, json};

/// Largest message accepted, in bytes.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// How messages are delimited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Framing {
    /// `Content-Length: N\r\n\r\n` followed by N bytes (LSP).
    Headers,
    /// One JSON value per line (MCP stdio).
    Lines,
}

/// What reading produced.
#[derive(Debug)]
pub enum Incoming {
    Message(Value),
    /// The bytes were not a JSON value; reply with a parse error.
    Malformed(String),
    /// The stream ended.
    End,
}

/// Read one message.
///
/// # Errors
///
/// Returns I/O errors and an `InvalidData` error for a message over
/// [`MAX_MESSAGE_BYTES`] or broken headers, after which the stream cannot be
/// trusted.
pub fn read(reader: &mut impl BufRead, framing: Framing) -> io::Result<Incoming> {
    let body = match framing {
        Framing::Headers => {
            let mut length: Option<usize> = None;
            loop {
                let mut line = String::new();
                if read_bounded_line(reader, &mut line)? == 0 {
                    return Ok(Incoming::End);
                }
                let line = line.trim_end_matches(['\r', '\n']);
                if line.is_empty() {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().ok();
                    }
                }
            }
            let length = length.ok_or_else(|| invalid("missing Content-Length"))?;
            if length > MAX_MESSAGE_BYTES {
                return Err(invalid("message is too large"));
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body)?;
            body
        }
        Framing::Lines => loop {
            let mut line = String::new();
            if read_bounded_line(reader, &mut line)? == 0 {
                return Ok(Incoming::End);
            }
            if !line.trim().is_empty() {
                break line.into_bytes();
            }
        },
    };
    Ok(match serde_json::from_slice(&body) {
        Ok(value) => Incoming::Message(value),
        Err(error) => Incoming::Malformed(error.to_string()),
    })
}

/// Read a line of at most [`MAX_MESSAGE_BYTES`].
fn read_bounded_line(reader: &mut impl BufRead, line: &mut String) -> io::Result<usize> {
    let limit = u64::try_from(MAX_MESSAGE_BYTES).unwrap_or(u64::MAX) + 1;
    let read = reader.take(limit).read_line(line)?;
    if read > MAX_MESSAGE_BYTES {
        return Err(invalid("message is too large"));
    }
    Ok(read)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Write one message.
///
/// # Errors
///
/// Returns I/O errors.
pub fn write(writer: &mut impl Write, framing: Framing, message: &Value) -> io::Result<()> {
    let body = message.to_string();
    match framing {
        Framing::Headers => write!(writer, "Content-Length: {}\r\n\r\n{body}", body.len())?,
        Framing::Lines => writeln!(writer, "{body}")?,
    }
    writer.flush()
}

/// A successful response to request `id`.
#[must_use]
pub fn response(id: &Value, result: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// An error response to request `id`.
#[must_use]
pub fn error(id: &Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

/// A notification.
#[must_use]
pub fn notification(method: &str, params: &Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
}

pub const PARSE_ERROR: i64 = -32700;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;

#[cfg(test)]
mod tests {
    use super::{Framing, Incoming, MAX_MESSAGE_BYTES, read, write};
    use serde_json::json;

    fn round_trip(framing: Framing) {
        let mut bytes = Vec::new();
        let messages = [json!({"a": 1}), json!({"b": "ü\nx"})];
        for message in &messages {
            write(&mut bytes, framing, message).expect("write");
        }
        let mut reader = bytes.as_slice();
        for message in &messages {
            match read(&mut reader, framing).expect("read") {
                Incoming::Message(value) => assert_eq!(&value, message),
                other => panic!("{other:?}"),
            }
        }
        assert!(matches!(read(&mut reader, framing), Ok(Incoming::End)));
    }

    #[test]
    fn messages_round_trip_in_both_framings() {
        round_trip(Framing::Headers);
        round_trip(Framing::Lines);
    }

    #[test]
    fn bad_input_is_reported_not_panicked_on() {
        let mut reader = "not json\n".as_bytes();
        assert!(matches!(
            read(&mut reader, Framing::Lines),
            Ok(Incoming::Malformed(_))
        ));
        let huge = format!("Content-Length: {}\r\n\r\n", MAX_MESSAGE_BYTES + 1);
        assert!(read(&mut huge.as_bytes(), Framing::Headers).is_err());
        let truncated = "Content-Length: 10\r\n\r\n{}";
        assert!(read(&mut truncated.as_bytes(), Framing::Headers).is_err());
        assert!(read(&mut "\r\n".as_bytes(), Framing::Headers).is_err());
    }
}
